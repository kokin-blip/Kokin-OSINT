//! Explicit text-only SearXNG capability. A fixed literal loopback address;
//! no redirects, proxies, arbitrary endpoints or attachment uploads.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
pub const ENDPOINT: &str = "http://127.0.0.1:8088/";
#[derive(Debug, thiserror::Error)]
#[error("local web discovery: {0}")]
pub struct Error(pub String);
pub fn query_url(query: &str, page: u16) -> Result<String, Error> {
    if query.trim().is_empty() || query.len() > 4096 || page == 0 || page > 100 {
        return Err(Error("enter a query and a page between 1 and 100".into()));
    }
    let mut url = url::Url::parse(ENDPOINT).map_err(|_| Error("endpoint".into()))?;
    url.set_path("search");
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("format", "json")
        .append_pair("pageno", &page.to_string());
    Ok(url.into())
}
/// Compatibility JSON search; research uses search_response to capture errors too.
pub fn search(
    query: &str,
    page: u16,
    cancel: &AtomicBool,
    online: &AtomicBool,
) -> Result<(String, crate::HttpResponse), Error> {
    let (url, mut response) = search_response(query, page, cancel, online)?;
    if !(200..300).contains(&response.status) {
        return Err(Error(format!(
            "SearXNG returned {}; check JSON output is enabled",
            response.status
        )));
    }
    response.body = String::from_utf8(response.body_bytes.take().unwrap_or_default())
        .map_err(|_| Error("response is not UTF-8 JSON".into()))?;
    let _: serde_json::Value = serde_json::from_str(&response.body)
        .map_err(|_| Error("response is not JSON; enable JSON in SearXNG settings".into()))?;
    Ok((url, response))
}
/// Bounded raw reply, including challenges and error pages, for L0 capture.
pub fn search_response(
    query: &str,
    page: u16,
    cancel: &AtomicBool,
    online: &AtomicBool,
) -> Result<(String, crate::HttpResponse), Error> {
    let url = query_url(query, page)?;
    if !online.load(Ordering::SeqCst)
        || cancel.load(Ordering::SeqCst)
        || std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
    {
        return Err(Error(
            "online mode and query disclosure are required".into(),
        ));
    }
    let _permit = crate::scheduler::shared()
        .acquire("127.0.0.1", cancel, online)
        .map_err(Error)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error("runtime".into()))?;
    let (status,headers,bytes)=rt.block_on(async{
  let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(45)).build().map_err(|_|Error("client".into()))?;
  let work=async{let mut response=client.get(&url).header("accept","application/json").send().await.map_err(|_|Error("SearXNG unavailable; start the local Docker service".into()))?;
   if response.status().is_redirection(){return Err(Error("SearXNG redirects are refused".into()));}
   let status=response.status().as_u16();
   let headers=response.headers().iter().filter(|(k,_)|matches!(k.as_str(),"content-type"|"date"|"retry-after")).filter_map(|(k,v)|v.to_str().ok().map(|v|(k.as_str().to_owned(),v.to_owned()))).collect();
   if response.content_length().is_some_and(|n|n>4*1024*1024){return Err(Error("response exceeds 4 MiB".into()));}
   let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_|Error("response interrupted".into()))?{if bytes.len()+chunk.len()>4*1024*1024{return Err(Error("response exceeds 4 MiB".into()));}bytes.extend(chunk);}Ok((status,headers,bytes))
  };
  tokio::select!{r=work=>r,_=async{loop{if cancel.load(Ordering::SeqCst)||!online.load(Ordering::SeqCst){break;}tokio::time::sleep(Duration::from_millis(50)).await;}}=>Err(Error("discovery cancelled or online mode disabled".into()))}
 })?;
    Ok((
        url,
        crate::HttpResponse {
            status,
            headers,
            body: String::new(),
            body_bytes: Some(bytes),
            redirect_chain: vec![],
            from_fixture: false,
        },
    ))
}
pub fn browser_url(provider: &str, query: &str) -> Result<String, Error> {
    if query.len() > 4096 {
        return Err(Error("query too long".into()));
    }
    let base = match provider {
        "google" => "https://www.google.com/search",
        "bing" => "https://www.bing.com/search",
        "pimeyes" => "https://pimeyes.com/en",
        "instantusername" => "https://instantusername.com/",
        "whatsmyname" => "https://whatsmyname.io/",
        "lenso" => "https://lenso.ai/en",
        "osint_industries" | "osint_industries_web" => "https://app.osint.industries/",
        "facecheck" => "https://facecheck.id/",
        _ => return Err(Error("unknown browser search provider".into())),
    };
    let mut url = url::Url::parse(base).map_err(|_| Error("provider URL".into()))?;
    if matches!(provider, "google" | "bing") {
        url.query_pairs_mut().append_pair("q", query);
    }
    Ok(url.into())
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn queries_cannot_change_destination() {
        let u = query_url("&q=x#http://evil.test", 1).unwrap();
        let u = url::Url::parse(&u).unwrap();
        assert_eq!(u.host_str(), Some("127.0.0.1"));
        assert_eq!(u.port(), Some(8088));
        assert_eq!(u.query_pairs().filter(|(k, _)| k == "q").count(), 1);
        assert!(query_url("", 1).is_err());
        assert!(browser_url("file", "x").is_err());
        assert!(!browser_url("pimeyes", "secret").unwrap().contains("secret"));
        assert_eq!(
            browser_url("osint_industries_web", "private-selector").unwrap(),
            "https://app.osint.industries/"
        );
    }
}
