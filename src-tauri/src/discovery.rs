//! Disclosed text discovery; raw responses enter Evidence before interpretation.
use crate::{views::*, CommandError, ErrorCode, Session};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};
fn err(e: impl std::fmt::Display) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, e.to_string())
}
pub fn parse(body: &str) -> Result<Vec<DiscoveryHitView>, CommandError> {
    let root: serde_json::Value = serde_json::from_str(body).map_err(err)?;
    let rows = root["results"]
        .as_array()
        .ok_or_else(|| err("SearXNG results missing"))?;
    let mut hits = Vec::new();
    for row in rows.iter().take(100) {
        let url = row["url"].as_str().unwrap_or_default();
        if kokin_net::normalise(url).is_err() {
            continue;
        }
        hits.push(DiscoveryHitView {
            title: row["title"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(1000)
                .collect(),
            url: url.chars().take(4096).collect(),
            snippet: row["content"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(4000)
                .collect(),
        });
    }
    Ok(hits)
}
pub fn run(
    app: &AppHandle,
    job: &str,
    generation: u64,
    request: DiscoveryRequest,
    cancel: &Arc<AtomicBool>,
    online: &AtomicBool,
) -> Result<serde_json::Value, CommandError> {
    if !request.consent {
        return Err(err(
            "review and authorize the text query before web discovery",
        ));
    }
    app.state::<crate::operations::Operations>().update(
        job,
        "Searching through local SearXNG",
        true,
    );
    let (url, response) =
        kokin_net::discovery::search(&request.query, request.page, cancel, online).map_err(err)?;
    let hits = parse(&response.body)?;
    if cancel.load(Ordering::SeqCst) || !online.load(Ordering::SeqCst) {
        return Err(err("discovery cancelled"));
    }
    app.state::<crate::operations::Operations>()
        .begin_publish(job)?;
    app.state::<Session>().with_generation(generation,|open|{
  let blobs=kokin_blob::BlobStore::new(open.paths.blobs());let captured=kokin_ingest::ingest_response(&mut open.conn,&blobs,&open.cmk,&kokin_net::HttpRequest::get(&url),&response,&kokin_ingest::IngestContext{connector:"searxng",job_id:job},"structured_response_only")?;
  let extraction=kokin_extract::json::extract(&mut open.conn,&blobs,&open.cmk,&captured.artifact_id);
  let root:serde_json::Value=serde_json::from_str(&response.body).map_err(err)?;
  let failed=root["unresponsive_engines"].as_array().map(|rows|rows.iter().take(20).map(|r|r.to_string()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
  let limitation=format!("Search snippets are discovery leads; destination pages and attachments have not been collected. At most 100 leads are displayed. {} {}",if extraction.is_ok(){"The raw JSON response and located observations are saved."}else{"The raw JSON is saved; extraction failed and can be retried."},if failed.is_empty(){String::new()}else{format!("Partial upstream coverage: {failed}")});
  serde_json::to_value(DiscoveryView{hits,artifact_id:captured.artifact_id,query:request.query,limitation}).map_err(err)
 })
}
#[tauri::command]
pub async fn discovery_health(app: AppHandle) -> Result<String, CommandError> {
    if !app.state::<crate::operations::Operations>().is_online() {
        return Err(err(
            "enable online mode before checking the web-search service",
        ));
    }
    let online = app.state::<crate::operations::Operations>().online_flag();
    tauri::async_runtime::spawn_blocking(move||{let cancel=AtomicBool::new(false);let(_,response)=kokin_net::discovery::search("SearXNG",1,&cancel,&online).map_err(err)?;parse(&response.body)?;Ok("Local SearXNG JSON search is available. The test query was sent to its upstream engines.".into())}).await.map_err(|_|err("SearXNG health worker failed"))?
}
#[tauri::command]
pub fn research_browser(provider: String, query: String) -> Result<(), CommandError> {
    let url = kokin_net::discovery::browser_url(&provider, &query).map_err(err)?;
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(&url)
            .status()
            .map_err(err)?;
        if !status.success() {
            return Err(err("browser could not be opened"));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err(err(
            "native browser handoff is currently available on macOS",
        ))
    }
}
