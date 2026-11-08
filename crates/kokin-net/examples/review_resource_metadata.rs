//! Deliberate metadata review, not an installer or fixture recorder.
use kokin_net::{Attribution, EgressPolicy, HttpCapability, HttpRequest, RateLimiter};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let which = std::env::args()
        .nth(1)
        .ok_or("choose embeddings or runtime")?;
    let (url, host) = match which.as_str() {
        "embeddings" => (
            "https://huggingface.co/api/models/Xenova/bge-small-en-v1.5?blobs=true",
            "huggingface.co",
        ),
        "runtime" => (
            "https://api.github.com/repos/microsoft/onnxruntime/releases/tags/v1.22.0",
            "api.github.com",
        ),
        _ => return Err("closed metadata source".into()),
    };
    let p = EgressPolicy {
        allowed_hosts: vec![host.into()],
        max_response_bytes: 1024 * 1024,
        requests_per_second: 1.0,
        burst: 1,
    };
    let rate = Arc::new(Mutex::new(RateLimiter::new("resource_review", &p)));
    let h = kokin_net::live::LiveHttp::new(
        p,
        Attribution {
            connector: "resource_review".into(),
            job_id: "public-metadata".into(),
            case_id: "none".into(),
        },
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(false)),
        rate,
    );
    let response = h.send(&HttpRequest::get(url).with_header("Accept", "application/json"))?;
    let v: serde_json::Value =
        serde_json::from_slice(response.body_bytes.as_deref().unwrap_or_default())?;
    if which == "embeddings" {
        println!(
            "{}",
            serde_json::json!({"revision":v["sha"],"license":v["cardData"]["license"],"files":v["siblings"].as_array().ok_or("files")?.iter().filter(|f|matches!(f["rfilename"].as_str(),Some("onnx/model_quantized.onnx"|"tokenizer.json"))).collect::<Vec<_>>()})
        );
    } else {
        println!(
            "{}",
            v["assets"]
                .as_array()
                .ok_or("assets")?
                .iter()
                .find(|f| f["name"] == "onnxruntime-osx-arm64-1.22.0.tgz")
                .ok_or("runtime asset")?
        );
    }
    Ok(())
}
