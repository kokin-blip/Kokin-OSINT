//! Separate loopback inference capability. Public SSRF policies are untouched.
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("local inference endpoint or request is not permitted")]
    Refused,
    #[error("local inference was cancelled")]
    Cancelled,
    #[error("Ollama unavailable, response invalid, or request timed out")]
    Unavailable,
}
#[derive(Debug, Clone)]
pub enum Call {
    Version,
    Tags,
    Running,
    Show {
        model: String,
    },
    Generate {
        model: String,
        system: String,
        prompt: String,
        tokens: u16,
        benchmark: bool,
    },
}
pub trait InferenceCapability {
    fn send(&self, call: Call, cancel: &AtomicBool) -> Result<Value, Error>;
}
pub struct LoopbackInference {
    endpoint: url::Url,
}
impl LoopbackInference {
    pub fn new(endpoint: &str) -> Result<Self, Error> {
        let parsed = url::Url::parse(endpoint).map_err(|_| Error::Refused)?;
        if parsed.scheme() != "http"
            || !matches!(parsed.host(),Some(url::Host::Ipv4(ip)) if ip==std::net::Ipv4Addr::LOCALHOST)
                && !matches!(parsed.host(),Some(url::Host::Ipv6(ip)) if ip==std::net::Ipv6Addr::LOCALHOST)
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || parsed.port_or_known_default().is_none_or(|p| p < 1024)
        {
            return Err(Error::Refused);
        }
        Ok(Self { endpoint: parsed })
    }
}
fn model_valid(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 160
        && model
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-/.:".contains(&c))
        && !model.contains("cloud")
        && !model.contains("..")
}
fn request(call: Call) -> Result<(&'static str, Option<Value>), Error> {
    match call {
        Call::Version => Ok(("api/version", None)),
        Call::Tags => Ok(("api/tags", None)),
        Call::Running => Ok(("api/ps", None)),
        Call::Show { model } if model_valid(&model) => {
            Ok(("api/show", Some(json!({"model":model}))))
        }
        Call::Generate {
            model,
            system,
            prompt,
            tokens,
            benchmark,
        } if model_valid(&model)
            && system.len() <= 8192
            && prompt.len() <= 64 * 1024
            && tokens > 0
            && tokens <= 512 =>
        {
            Ok((
                "api/generate",
                Some(
                    json!({"model":model,"system":system,"prompt":prompt,"format":"json","stream":false,"think":false,"keep_alive":if benchmark {300}else{0},"options":{"num_ctx":4096,"num_predict":tokens,"temperature":0}}),
                ),
            ))
        }
        _ => Err(Error::Refused),
    }
}
impl InferenceCapability for LoopbackInference {
    fn send(&self, call: Call, cancel: &AtomicBool) -> Result<Value, Error> {
        if std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny") {
            return Err(Error::Refused);
        }
        let available = AtomicBool::new(true);
        let _permit = crate::scheduler::shared()
            .acquire("127.0.0.1", cancel, &available)
            .map_err(|_| Error::Cancelled)?;
        let (path, body) = request(call)?;
        let url = self.endpoint.join(path).map_err(|_| Error::Refused)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::Unavailable)?;
        runtime.block_on(async{
   let transport=async{
    let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(if path == "api/generate" {180} else {10})).build().map_err(|_|Error::Unavailable)?;
    let mut outgoing=if body.is_some(){client.post(url)}else{client.get(url)};
    if let Some(body)=body{outgoing=outgoing.header("content-type","application/json").body(body.to_string());}
    let mut response=outgoing.send().await.map_err(|_|Error::Unavailable)?;
    if !response.status().is_success()||response.content_length().is_some_and(|v|v>8*1024*1024){return Err(Error::Unavailable);}
    let mut bytes=Vec::new();while let Some(chunk)=response.chunk().await.map_err(|_|Error::Unavailable)?{if bytes.len()+chunk.len()>8*1024*1024{return Err(Error::Unavailable);}bytes.extend_from_slice(&chunk);}
    serde_json::from_slice(&bytes).map_err(|_|Error::Unavailable)
   };
   tokio::pin!(transport);
   loop{if cancel.load(Ordering::SeqCst){return Err(Error::Cancelled);}tokio::select!{result=&mut transport=>return result,_=tokio::time::sleep(Duration::from_millis(50))=>{}}}
  })
    }
}

impl LoopbackInference {
    /// Upload a locally verified file, then create from that digest. Never remote pull.
    pub fn import_file(
        &self,
        model: &str,
        path: &std::path::Path,
        digest: &str,
        bytes: u64,
        cancel: std::sync::Arc<AtomicBool>,
    ) -> Result<(), Error> {
        if !model_valid(model)
            || digest.len() != 64
            || !digest.bytes().all(|c| c.is_ascii_hexdigit())
            || std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
        {
            return Err(Error::Refused);
        }
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or(Error::Refused)?
            .to_owned();
        let metadata = std::fs::metadata(path).map_err(|_| Error::Unavailable)?;
        if metadata.len() != bytes {
            return Err(Error::Refused);
        }
        let available = AtomicBool::new(true);
        let _permit = crate::scheduler::shared()
            .acquire("127.0.0.1", &cancel, &available)
            .map_err(|_| Error::Cancelled)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::Unavailable)?;
        runtime.block_on(async {
            let transport=async {
                let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(1800)).build().map_err(|_|Error::Unavailable)?;
                let file=tokio::fs::File::open(path).await.map_err(|_|Error::Unavailable)?;
                let stream=tokio_util::io::ReaderStream::new(file);
                let upload=self.endpoint.join(&format!("api/blobs/sha256:{digest}")).map_err(|_|Error::Refused)?;
                let result=client.post(upload).header("content-length",bytes).body(reqwest::Body::wrap_stream(stream)).send().await.map_err(|_|Error::Unavailable)?;
                if !result.status().is_success(){return Err(Error::Unavailable);}
                let body=json!({"model":model,"files":{name:format!("sha256:{digest}")},"stream":false});
                let mut result=client.post(self.endpoint.join("api/create").map_err(|_|Error::Refused)?)
                    .header("content-type","application/json").body(body.to_string()).send().await.map_err(|_|Error::Unavailable)?;
                if !result.status().is_success() {return Err(Error::Unavailable);}
                let mut response=Vec::new();while let Some(chunk)=result.chunk().await.map_err(|_|Error::Unavailable)? {
                    if response.len()+chunk.len()>65536{return Err(Error::Unavailable);}response.extend_from_slice(&chunk);
                }
                if serde_json::from_slice::<Value>(&response).map_err(|_|Error::Unavailable)?["status"]!="success"{return Err(Error::Unavailable);}
                Ok(())
            };
            tokio::pin!(transport);
            loop {if cancel.load(Ordering::SeqCst){return Err(Error::Cancelled);}tokio::select!{result=&mut transport=>return result,_=tokio::time::sleep(Duration::from_millis(50))=>{}}}
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_does_not_resolve_names_or_allow_redirect_targets() {
        for endpoint in [
            "http://localhost:11434/",
            "http://127.0.0.2:11434/",
            "http://192.168.1.1:11434/",
            "https://127.0.0.1:11434/",
            "http://127.0.0.1:11434/api/",
            "http://user@127.0.0.1:11434/",
            "http://127.0.0.1:11434/?target=evil",
        ] {
            assert!(LoopbackInference::new(endpoint).is_err());
        }
        assert!(LoopbackInference::new("http://127.0.0.1:11434/").is_ok());
        assert!(LoopbackInference::new("http://[::1]:11434/").is_ok());
    }
    #[test]
    fn requests_have_no_tools_remote_pull_or_cloud_model_path() {
        assert!(request(Call::Show {
            model: "qwen3:4b-cloud".into()
        })
        .is_err());
        let (_, body) = request(Call::Generate {
            model: "qwen3:4b".into(),
            system: "system".into(),
            prompt: "untrusted evidence".into(),
            tokens: 512,
            benchmark: false,
        })
        .unwrap_or_else(|_| panic!("valid fixed request"));
        let body = body.unwrap_or_else(|| panic!("body"));
        assert!(body.get("tools").is_none());
        assert_eq!(body["stream"], false);
        assert_eq!(body["keep_alive"], 0);
    }
}
