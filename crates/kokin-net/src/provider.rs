//! Official-provider capability. Fixed endpoints; credentials never enter request DTOs.
use crate::{
    reconfirm_before_connect, vet_request, Attribution, EgressPolicy, HttpError, HttpRequest,
    HttpResponse, NetworkMode,
};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use zeroize::Zeroizing;
pub fn endpoint(provider: &str) -> Result<&'static str, HttpError> {
    match provider {
        "osint_industries" => Ok("https://api.osint.industries/v2/request"),
        "osint_industries_credits" => Ok("https://api.osint.industries/misc/credits"),
        "lenso" => Ok("https://api.lenso.ai/search"),
        "eyematch" => Ok("https://api.eyematch.ai/search"),
        _ => Err(HttpError::Refused("unknown official provider".into())),
    }
}
fn credential_account(provider: &str) -> &str {
    if provider == "osint_industries_credits" {
        "osint_industries"
    } else {
        provider
    }
}
/// Explicit native credential lookup; callers may disclose only the presence bit.
pub fn credential(provider: &str) -> Result<Zeroizing<String>, HttpError> {
    endpoint(provider)?;
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                "com.kokin.osint",
                "-a",
                credential_account(provider),
                "-w",
            ])
            .output()
            .map_err(|_| HttpError::Refused("macOS Keychain is unavailable".into()))?;
        if !out.status.success() {
            return Err(HttpError::Refused(
                "provider credential not available in macOS Keychain".into(),
            ));
        }
        let key = Zeroizing::new(
            String::from_utf8(out.stdout)
                .map_err(|_| HttpError::Refused("credential is not UTF-8".into()))?,
        );
        if key.trim().is_empty() || key.len() > 8192 || key.trim().contains(['\r', '\n']) {
            return Err(HttpError::Refused("invalid provider credential".into()));
        }
        Ok(key)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(HttpError::Refused(
            "native provider credential storage is not implemented on this platform".into(),
        ))
    }
}
pub struct ProviderHttp {
    pub provider: String,
    pub attribution: Attribution,
    pub online: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
}
impl ProviderHttp {
    pub fn validate(&self, request: &HttpRequest) -> Result<url::Url, HttpError> {
        let usage = self.provider == "osint_industries_credits";
        let invalid_method_body = if usage {
            request.method != "GET" || request.body.is_some()
        } else {
            request.method != "POST"
                || request
                    .body
                    .as_ref()
                    .is_none_or(|s| s.len() > 14 * 1024 * 1024)
        };
        if request.url != endpoint(&self.provider)?
            || invalid_method_body
            || !request.headers.is_empty()
        {
            return Err(HttpError::Refused(
                "provider request exceeds its fixed endpoint/method/body capability".into(),
            ));
        }
        url::Url::parse(&request.url)
            .map_err(|_| HttpError::Refused("provider endpoint invalid".into()))
    }
    fn check(&self) -> Result<(), HttpError> {
        if self.cancel.load(Ordering::SeqCst)
            || !self.online.load(Ordering::SeqCst)
            || std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
        {
            return Err(HttpError::Refused(
                "provider request cancelled or offline".into(),
            ));
        }
        Ok(())
    }
}
impl crate::HttpCapability for ProviderHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.check()?;
        let url = self.validate(request)?;
        let host = url
            .host_str()
            .ok_or_else(|| HttpError::Refused("provider host missing".into()))?;
        let _permit = crate::scheduler::shared()
            .acquire(host, &self.cancel, &self.online)
            .map_err(HttpError::Refused)?;
        let policy = EgressPolicy {
            allowed_hosts: vec![host.into()],
            max_response_bytes: if self.provider == "osint_industries_credits" {
                64 * 1024
            } else {
                8 * 1024 * 1024
            },
            requests_per_second: 1.0,
            burst: 1,
        };
        let target = vet_request(
            "https",
            host,
            443,
            &policy,
            &crate::live::SystemResolver,
            NetworkMode::Allow,
        )
        .map_err(|_| HttpError::Refused("provider destination refused".into()))?;
        reconfirm_before_connect(&target, &crate::live::SystemResolver)
            .map_err(|_| HttpError::Refused("provider destination changed".into()))?;
        let secret = credential(&self.provider)?;
        self.check()?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| HttpError::Refused("provider runtime unavailable".into()))?;
        runtime.block_on(cancel_transport(
            async {
                let client = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(85))
                    .resolve(host, std::net::SocketAddr::new(target.addr, 443))
                    .user_agent("Kokin-OSINT/0.0.1 official-provider")
                    .build()
                    .map_err(|_| HttpError::Refused("provider client unavailable".into()))?;
                let mut outgoing = if self.provider == "osint_industries_credits" {
                    client.get(url.as_str())
                } else {
                    client.post(url.as_str()).header("content-type", "application/json").body(request.body.clone().unwrap_or_default())
                };
                if credential_account(&self.provider) == "osint_industries" {
                    outgoing = outgoing.header("api-key", secret.trim());
                } else {
                    outgoing = outgoing.bearer_auth(secret.trim());
                }
                // No automatic retries: a lost response may already have consumed credits.
                let mut response = outgoing.send().await.map_err(|_| {
                    HttpError::Refused(
                "provider failed or timed out; billing outcome may be unknown; no retry occurred"
                    .into(),
            )
                })?;
                let status = response.status().as_u16();
                let headers = response
                    .headers()
                    .iter()
                    .filter(|(k, _)| matches!(k.as_str(), "content-type" | "date" | "retry-after"))
                    .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().into(), v.into())))
                    .collect::<BTreeMap<_, _>>();
                if response
                    .content_length()
                    .is_some_and(|n| n > policy.max_response_bytes)
                {
                    return Err(HttpError::Refused(
                        "provider response exceeds its cap".into(),
                    ));
                }
                let mut body = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(|_| {
                    HttpError::Refused(
                        "provider response interrupted; billing outcome may be unknown; no retry"
                            .into(),
                    )
                })? {
                    if body.len() + chunk.len() > policy.max_response_bytes as usize {
                        return Err(HttpError::Refused(
                            "provider response exceeds its cap".into(),
                        ));
                    }
                    body.extend_from_slice(&chunk);
                }
                if credential_echo(&body,secret.trim()){return Err(HttpError::Refused("provider echoed a credential; response withheld from evidence; billing outcome may be unknown; no retry".into()));}
                Ok(HttpResponse {
                    status,
                    headers,
                    body: String::new(),
                    body_bytes: Some(body),
                    redirect_chain: vec![],
                    from_fixture: false,
                })
            },
            &self.cancel,
            &self.online,
        ))
    }
}
fn credential_echo(body: &[u8], credential: &str) -> bool {
    if credential.is_empty() {
        return false;
    }
    let raw = credential.as_bytes();
    let encoded = serde_json::to_vec(credential).unwrap_or_default();
    body.windows(raw.len()).any(|v| v == raw)
        || (!encoded.is_empty() && body.windows(encoded.len()).any(|v| v == encoded))
}
async fn cancel_transport<T>(
    work: impl std::future::Future<Output = Result<T, HttpError>>,
    cancel: &AtomicBool,
    online: &AtomicBool,
) -> Result<T, HttpError> {
    tokio::pin!(work);
    loop {
        if cancel.load(Ordering::SeqCst)
            || !online.load(Ordering::SeqCst)
            || std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
        {
            return Err(HttpError::Refused(
                "provider transport cancelled; an accepted request may still be billed; no retry"
                    .into(),
            ));
        }
        tokio::select! {result=&mut work=>return result,_=tokio::time::sleep(Duration::from_millis(50))=>{}}
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn credit_capability_is_only_the_fixed_get_without_body_headers_or_selector() {
        let mut http = ProviderHttp {
            provider: "osint_industries_credits".into(),
            attribution: Attribution {
                connector: "osint_industries_credits".into(),
                job_id: "fixture".into(),
                case_id: "fixture".into(),
            },
            online: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let request = HttpRequest::get(endpoint(&http.provider).unwrap());
        assert!(http.validate(&request).is_ok());
        assert_eq!(credential_account(&http.provider), "osint_industries");
        use crate::HttpCapability;
        assert!(http.send(&request).is_err()); // Offline exits before Keychain/DNS/transport.
        let mut invalid = request.clone();
        invalid.method = "POST".into();
        assert!(http.validate(&invalid).is_err());
        invalid = request.clone();
        invalid.body = Some(String::new());
        assert!(http.validate(&invalid).is_err());
        invalid = request.clone().with_header("api-key", "synthetic");
        assert!(http.validate(&invalid).is_err());
        for url in [
            "https://api.osint.industries/misc/credits?query=example",
            "https://api.osint.industries/misc/credits/",
            "https://api.osint.industries/v2/request",
            "https://127.0.0.1/misc/credits",
            "https://api.osint.industries/misc/usage",
        ] {
            assert!(http.validate(&HttpRequest::get(url)).is_err());
        }
        http.provider = "osint_industries".into();
        assert!(http
            .validate(&HttpRequest::get(endpoint(&http.provider).unwrap()))
            .is_err());
        http.provider = "lenso".into();
        assert!(http
            .validate(&HttpRequest::get(endpoint(&http.provider).unwrap()))
            .is_err());
    }
    #[test]
    fn an_echoed_credential_cannot_enter_case_evidence() {
        assert!(credential_echo(
            b"{\"echo\":\"synthetic-credential\"}",
            "synthetic-credential"
        ));
        assert!(credential_echo(
            br#"{"echo":"synthetic\"quoted"}"#,
            "synthetic\"quoted"
        ));
        assert!(!credential_echo(
            b"{\"results\":[]}",
            "synthetic-credential"
        ));
    }
    #[test]
    fn cancellation_drops_a_transport_waiting_for_a_response() {
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let online = AtomicBool::new(true);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            signal.store(true, Ordering::SeqCst);
        });
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        let response = rt.block_on(cancel_transport(
            async {
                std::future::pending::<()>().await;
                Ok(())
            },
            &cancel,
            &online,
        ));
        thread.join().unwrap();
        assert!(response.is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn no_credentials_are_read_for_invalid_or_offline_requests() {
        let http = ProviderHttp {
            provider: "lenso".into(),
            attribution: Attribution {
                connector: "lenso".into(),
                job_id: "fixture".into(),
                case_id: "fixture".into(),
            },
            online: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let mut r = HttpRequest::get("https://api.lenso.ai/search");
        r.method = "POST".into();
        r.body = Some("{}".into());
        assert!(http.validate(&r).is_ok());
        use crate::HttpCapability;
        assert!(http.send(&r).is_err());
        r.url = "https://api.lenso.ai/account/delete".into();
        assert!(http.validate(&r).is_err());
        r.url = "https://127.0.0.1/search".into();
        assert!(http.validate(&r).is_err());
    }
}
