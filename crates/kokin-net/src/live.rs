//! Static HTTP transport. No automatic redirects, proxies, cookies or retries.
//! The connection is pinned to the vetted address while TLS verifies the host.
use crate::http::{Fixture, HttpCapability, HttpError, HttpRequest, HttpResponse, ReplayHttp};
use crate::{
    reconfirm_before_connect, vet_request, Attribution, CappedReader, EgressPolicy, NetworkMode,
    RateLimiter, Resolver,
};
use std::collections::BTreeMap;
use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct SystemResolver;
impl Resolver for SystemResolver {
    fn resolve(&self, host: &str) -> Result<Vec<std::net::IpAddr>, String> {
        (host, 443)
            .to_socket_addrs()
            .map(|rows| rows.map(|r| r.ip()).collect())
            .map_err(|_| "name resolution failed".into())
    }
}

pub struct LiveHttp {
    policy: EgressPolicy,
    pub attribution: Attribution,
    online: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    limiter: Arc<Mutex<RateLimiter>>,
}
impl LiveHttp {
    pub fn new(
        policy: EgressPolicy,
        attribution: Attribution,
        online: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
        limiter: Arc<Mutex<RateLimiter>>,
    ) -> Self {
        Self {
            policy,
            attribution,
            online,
            cancelled,
            limiter,
        }
    }
    fn check(&self) -> Result<(), HttpError> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(HttpError::Refused("operation cancelled".into()));
        }
        if !self.online.load(Ordering::SeqCst)
            || std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny")
        {
            return Err(HttpError::Refused("network is offline".into()));
        }
        Ok(())
    }
}
impl HttpCapability for LiveHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.check()?;
        if request.method != "GET" || request.body.is_some() || !request.headers.is_empty() {
            return Err(HttpError::Refused(
                "static collection accepts an unauthenticated GET only".into(),
            ));
        }
        let mut url =
            url::Url::parse(&request.url).map_err(|_| HttpError::Refused("invalid URL".into()))?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(HttpError::Refused(
                "URLs containing credentials are refused".into(),
            ));
        }
        let mut redirects = Vec::new();
        for _ in 0..=5 {
            self.check()?;
            let host = url
                .host_str()
                .ok_or_else(|| HttpError::Refused("URL has no host".into()))?
                .to_string();
            let port = url
                .port_or_known_default()
                .ok_or_else(|| HttpError::Refused("unsupported scheme".into()))?;
            let vetted = vet_request(
                url.scheme(),
                &host,
                port,
                &self.policy,
                &SystemResolver,
                NetworkMode::Allow,
            )
            .map_err(|_| HttpError::Refused("destination refused by network policy".into()))?;
            reconfirm_before_connect(&vetted, &SystemResolver).map_err(|_| {
                HttpError::Refused("destination changed or was refused before connection".into())
            })?;
            self.limiter
                .lock()
                .map_err(|_| HttpError::Refused("rate limiter unavailable".into()))?
                .try_acquire()
                .map_err(|_| {
                    HttpError::Refused("rate limit reached; wait before collecting again".into())
                })?;
            self.check()?;
            let client = reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(30))
                .resolve(&host, SocketAddr::new(vetted.addr, port))
                .user_agent("Kokin-OSINT/0.0.1 static-collector")
                .build()
                .map_err(|_| HttpError::Refused("HTTP client could not start".into()))?;
            let response = client
                .get(url.as_str())
                .send()
                .map_err(|_| HttpError::Refused("request failed or timed out".into()))?;
            self.check()?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        HttpError::Refused("redirect has no usable destination".into())
                    })?;
                let next = url
                    .join(location)
                    .map_err(|_| HttpError::Refused("invalid redirect".into()))?;
                if next.host_str() != url.host_str()
                    || next.scheme() != url.scheme()
                    || next.port_or_known_default() != url.port_or_known_default()
                    || !next.username().is_empty()
                    || next.password().is_some()
                {
                    return Err(HttpError::Refused(
                        "redirect changes destination or scheme; collect its public URL explicitly"
                            .into(),
                    ));
                }
                redirects.push(next.as_str().to_string());
                url = next;
                continue;
            }
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "set-cookie" | "authorization" | "proxy-authenticate"
                    )
                })
                .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().into(), v.into())))
                .collect::<BTreeMap<_, _>>();
            let mut reader = CappedReader::new(response, self.policy.max_response_bytes);
            let mut body = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            loop {
                self.check()?;
                let n = reader.read(&mut chunk).map_err(|_| {
                    HttpError::Refused("response exceeded its byte cap or could not be read".into())
                })?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            return Ok(HttpResponse {
                status,
                headers,
                body: String::new(),
                body_bytes: Some(body),
                redirect_chain: redirects,
                from_fixture: false,
            });
        }
        Err(HttpError::Refused("too many redirects".into()))
    }
}

/// Deliberate developer recording. Nothing automatically commits these files.
/// Sensitive/query-bearing requests are refused, and response headers are scrubbed.
pub struct RecordingHttp<H> {
    inner: H,
    replay: ReplayHttp,
}
impl<H: HttpCapability> RecordingHttp<H> {
    pub fn new(inner: H, directory: impl AsRef<Path>) -> Self {
        Self {
            inner,
            replay: ReplayHttp::new(directory),
        }
    }
}
impl<H: HttpCapability> HttpCapability for RecordingHttp<H> {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let url = url::Url::parse(&request.url)
            .map_err(|_| HttpError::Refused("invalid recording URL".into()))?;
        if url.query().is_some()
            || !request.headers.is_empty()
            || request.body.is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(HttpError::Refused(
                "recording sensitive or query-bearing requests is refused".into(),
            ));
        }
        let mut response = self.inner.send(request)?;
        response
            .headers
            .retain(|k, _| matches!(k.as_str(), "content-type" | "content-length"));
        if response
            .redirect_chain
            .iter()
            .any(|u| url::Url::parse(u).map_or(true, |u| u.query().is_some()))
        {
            return Err(HttpError::Refused(
                "query-bearing redirect requires manual sanitization".into(),
            ));
        }
        let text = String::from_utf8_lossy(
            response
                .body_bytes
                .as_deref()
                .unwrap_or(response.body.as_bytes()),
        )
        .to_ascii_lowercase();
        if ["password", "bearer ", "api_key", "secret_key", "set-cookie"]
            .iter()
            .any(|key| text.contains(key))
        {
            return Err(HttpError::Refused(
                "fixture may contain secrets; sanitize it manually before recording".into(),
            ));
        }
        self.replay.store(&Fixture {
            format_version: 2,
            key: request.fixture_key()?,
            request: request.clone(),
            response: response.clone(),
            recorded_utc: format!(
                "unix:{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ),
            note: Some("raw-byte fixture v2; headers scrubbed; developer review required".into()),
        })?;
        Ok(response)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn offline_and_cancelled_requests_fail_before_dns() {
        let policy = EgressPolicy {
            allowed_hosts: vec!["unresolvable.invalid".into()],
            max_response_bytes: 100,
            requests_per_second: 1.0,
            burst: 1,
        };
        let online = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let limiter = Arc::new(Mutex::new(RateLimiter::new("test", &policy)));
        let http = LiveHttp::new(
            policy,
            Attribution {
                connector: "test".into(),
                job_id: "test".into(),
                case_id: "test".into(),
            },
            online.clone(),
            cancelled.clone(),
            limiter,
        );
        assert_eq!(
            http.send(&HttpRequest::get("https://unresolvable.invalid"))
                .unwrap_err()
                .to_string(),
            "network is offline"
        );
        online.store(true, Ordering::SeqCst);
        cancelled.store(true, Ordering::SeqCst);
        assert_eq!(
            http.send(&HttpRequest::get("https://unresolvable.invalid"))
                .unwrap_err()
                .to_string(),
            "operation cancelled"
        );
    }
    struct Synthetic;
    impl HttpCapability for Synthetic {
        fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
            Ok(HttpResponse {
                status: 200,
                headers: BTreeMap::from([
                    ("set-cookie".into(), "sensitive".into()),
                    ("content-type".into(), "application/octet-stream".into()),
                ]),
                body: String::new(),
                body_bytes: Some(vec![0, 255, 128, 42]),
                redirect_chain: Vec::new(),
                from_fixture: false,
            })
        }
    }
    #[test]
    fn raw_byte_recordings_replay_without_cookie_headers() {
        let dir = std::env::temp_dir().join(format!("kokin-recording-{}", std::process::id()));
        let recorder = RecordingHttp::new(Synthetic, &dir);
        let request = HttpRequest::get("https://example.org/public");
        recorder.send(&request).unwrap();
        let replay = ReplayHttp::new(&dir).send(&request).unwrap();
        assert_eq!(replay.body_bytes, Some(vec![0, 255, 128, 42]));
        assert!(!replay.headers.contains_key("set-cookie"));
        assert!(replay.from_fixture);
        assert!(recorder
            .send(&HttpRequest::get("https://example.org/?token=x"))
            .is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
