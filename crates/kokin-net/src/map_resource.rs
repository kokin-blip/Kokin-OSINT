//! Closed public basemap capability. Never used by the active map renderer.
use crate::{
    live::SystemResolver, reconfirm_before_connect, vet_request, EgressPolicy, NetworkMode,
};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub const WORLD_URL: &str = "https://build.protomaps.com/20261011.pmtiles";
#[derive(Debug, thiserror::Error)]
#[error("basemap metadata/range transfer refused or interrupted")]
pub struct Error;
#[derive(Default)]
pub struct WorldSource {
    etag: Option<String>,
    size: Option<u64>,
    last: Option<Instant>,
    requests: usize,
}
impl WorldSource {
    pub fn resume(etag: String, size: u64) -> Self {
        Self {
            etag: Some(etag),
            size: Some(size),
            last: None,
            requests: 0,
        }
    }
    pub fn identity(&self) -> Option<(&str, u64)> {
        Some((self.etag.as_deref()?, self.size?))
    }
    pub fn read(
        &mut self,
        offset: u64,
        length: u64,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>, Error> {
        if cancel.load(Ordering::SeqCst)
            || std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
            || length == 0
            || length > 4 * 1024 * 1024
            || self.requests >= 512
        {
            return Err(Error);
        }
        let available = AtomicBool::new(true);
        let _permit = crate::scheduler::shared()
            .acquire("build.protomaps.com", cancel, &available)
            .map_err(|_| Error)?;
        let end = offset.checked_add(length - 1).ok_or(Error)?;
        let policy = EgressPolicy {
            allowed_hosts: vec!["build.protomaps.com".into()],
            max_response_bytes: length,
            requests_per_second: 1.0,
            burst: 1,
        };
        let target = vet_request(
            "https",
            "build.protomaps.com",
            443,
            &policy,
            &SystemResolver,
            NetworkMode::Allow,
        )
        .map_err(|_| Error)?;
        reconfirm_before_connect(&target, &SystemResolver).map_err(|_| Error)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error)?;
        let result=runtime.block_on(async {
            let transfer=async {
                if let Some(last)=self.last {tokio::time::sleep(Duration::from_secs(1).saturating_sub(last.elapsed())).await;}
                let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
                    .resolve("build.protomaps.com",SocketAddr::new(target.addr,443)).connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(60)).build().map_err(|_|Error)?;
                let mut request=client.get(WORLD_URL).header("range",format!("bytes={offset}-{end}"))
                    .header("accept-encoding","identity").header("user-agent","Kokin-OSINT explicit basemap installer");
                if let Some(etag)=&self.etag {request=request.header("if-match",etag);}
                let mut response=request.send().await.map_err(|_|Error)?;
                if response.status()!=reqwest::StatusCode::PARTIAL_CONTENT {return Err(Error);}
                let etag=response.headers().get("etag").and_then(|v|v.to_str().ok()).filter(|s|s.starts_with('"')&&s.ends_with('"')&&s.len()<256).ok_or(Error)?.to_owned();
                let range=response.headers().get("content-range").and_then(|v|v.to_str().ok()).ok_or(Error)?;
                let (extent,total)=range.strip_prefix("bytes ").ok_or(Error)?.split_once('/').ok_or(Error)?;
                if extent!=format!("{offset}-{end}") {return Err(Error);}
                let total=total.parse::<u64>().map_err(|_|Error)?;
                if end>=total || total>1024*1024*1024*1024 || self.etag.as_ref().is_some_and(|v|v!=&etag) || self.size.is_some_and(|v|v!=total) {return Err(Error);}
                let mut bytes=Vec::with_capacity(length as usize);
                while let Some(chunk)=response.chunk().await.map_err(|_|Error)? {if bytes.len() as u64+chunk.len() as u64>length{return Err(Error);}bytes.extend_from_slice(&chunk);}
                if bytes.len() as u64!=length {return Err(Error);}
                Ok((bytes,etag,total))
            };
            tokio::pin!(transfer);
            loop{if cancel.load(Ordering::SeqCst){return Err(Error);}tokio::select!{result=&mut transfer=>return result,_=tokio::time::sleep(Duration::from_millis(50))=>{}}}
        });
        self.last = Some(Instant::now());
        self.requests += 1;
        let (bytes, etag, size) = result?;
        self.etag = Some(etag);
        self.size = Some(size);
        Ok(bytes)
    }
}
