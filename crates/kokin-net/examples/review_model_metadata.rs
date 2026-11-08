//! Explicit developer review only: public metadata, never model installation.
use kokin_net::{Attribution, EgressPolicy, HttpCapability, HttpRequest, RateLimiter};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let policy = EgressPolicy {
        allowed_hosts: vec!["huggingface.co".into()],
        max_response_bytes: 1024 * 1024,
        requests_per_second: 1.0,
        burst: 1,
    };
    let rate = Arc::new(Mutex::new(RateLimiter::new("registry_review", &policy)));
    let http = kokin_net::live::LiveHttp::new(
        policy,
        Attribution {
            connector: "registry_review".into(),
            job_id: "explicit-metadata-only".into(),
            case_id: "no-case-public-research".into(),
        },
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(false)),
        rate,
    );
    let size = std::env::args().nth(1).ok_or("choose 4, 8, or 14")?;
    if !["4", "8", "14"].contains(&size.as_str()) {
        return Err("unsupported registry candidate".into());
    }
    let response = http.send(
        &HttpRequest::get(format!(
            "https://huggingface.co/api/models/Qwen/Qwen3-{size}B-GGUF?blobs=true"
        ))
        .with_header("Accept", "application/json"),
    )?;
    let value: serde_json::Value =
        serde_json::from_slice(response.body_bytes.as_deref().unwrap_or_default())?;
    let file = format!("Qwen3-{size}B-Q4_K_M.gguf");
    let artifact = value["siblings"]
        .as_array()
        .ok_or("files missing")?
        .iter()
        .find(|a| a["rfilename"] == file)
        .ok_or("artifact missing")?;
    println!(
        "{}",
        serde_json::json!({"id":format!("qwen3-{size}b-q4km"),"repository":value["id"],"revision":value["sha"],"file":file,"sha256":artifact["lfs"]["sha256"],"bytes":artifact["size"],"license":value["cardData"]["license"],"tasks":["cited_summary","entity_candidate","relationship_candidate"],"benchmark":null})
    );
    Ok(())
}
