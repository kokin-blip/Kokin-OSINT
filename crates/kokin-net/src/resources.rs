//! Explicit pinned resource transfer, separate from public collection redirects.
use crate::{
    live::SystemResolver, reconfirm_before_connect, vet_request, Attribution, EgressPolicy,
    NetworkMode,
};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
#[derive(Debug, thiserror::Error)]
#[error("resource transfer refused, interrupted, or failed verification: {0}")]
pub struct Error(pub String);
pub struct PinnedResource<'a> {
    pub url: &'a str,
    pub hosts: &'a [String],
    pub bytes: u64,
    pub sha256: &'a str,
}
pub fn download(
    resource: PinnedResource<'_>,
    output: &mut dyn Write,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64),
    _attribution: &Attribution,
) -> Result<(), Error> {
    let PinnedResource {
        url,
        hosts,
        bytes,
        sha256,
    } = resource;
    if std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
        || bytes == 0
        || bytes > 32 * 1024 * 1024 * 1024
        || sha256.len() != 64
    {
        return Err(Error("offline or invalid pinned resource".into()));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error("runtime".into()))?;
    runtime.block_on(async {
    let transfer=async {
    let policy = EgressPolicy {
        allowed_hosts: hosts.to_vec(),
        max_response_bytes: bytes,
        requests_per_second: 1.0,
        burst: 1,
    };
    let mut url = url::Url::parse(url).map_err(|_| Error("URL".into()))?;
    for _ in 0..=5 {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error("cancelled".into()));
        }
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port_or_known_default() != Some(443)
        {
            return Err(Error("destination".into()));
        }
        let host = url.host_str().ok_or_else(|| Error("host".into()))?;
        let target = vet_request(
            "https",
            host,
            443,
            &policy,
            &SystemResolver,
            NetworkMode::Allow,
        )
        .map_err(|_| Error("destination denied".into()))?;
        reconfirm_before_connect(&target, &SystemResolver)
            .map_err(|_| Error("destination changed".into()))?;
        let available=AtomicBool::new(true);
        let _permit=crate::scheduler::shared().acquire(host,cancel,&available).map_err(Error)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve(host, SocketAddr::new(target.addr, 443))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(7200))
            .build()
            .map_err(|_| Error("transport".into()))?;
        let mut response = client
            .get(url.clone())
            .header("user-agent", "Kokin-OSINT resource installer")
            .send().await
            .map_err(|_| Error("request failed".into()))?;
        if response.status().is_redirection() {
            let next = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Error("redirect".into()))?;
            url = url.join(next).map_err(|_| Error("redirect".into()))?;
            continue;
        }
        if !response.status().is_success() || response.content_length().is_some_and(|v| v != bytes)
        {
            return Err(Error("response size/status".into()));
        }
        let mut hash = Sha256::new();
        let mut count = 0u64;

        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(Error("cancelled".into()));
            }
            let Some(chunk)=response.chunk().await.map_err(|_|Error("read".into()))? else {break;};
            let n=chunk.len();
            if n == 0 {
                break;
            }
            count = count
                .checked_add(n as u64)
                .ok_or_else(|| Error("size".into()))?;
            if count > bytes {
                return Err(Error("byte cap".into()));
            }
            output
                .write_all(&chunk)
                .map_err(|_| Error("write".into()))?;
            hash.update(&chunk);
            progress(count);
        }
        if count != bytes || format!("{:x}", hash.finalize()) != sha256 {
            return Err(Error("identity".into()));
        }
        return Ok(());
    }
    Err(Error("redirect budget".into()))
    };
    tokio::pin!(transfer);
    loop {if cancel.load(Ordering::SeqCst){return Err(Error("cancelled".into()));}tokio::select!{result=&mut transfer=>return result,_=tokio::time::sleep(Duration::from_millis(50))=>{}}}
    })
}
