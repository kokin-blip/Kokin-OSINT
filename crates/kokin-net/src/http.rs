//! The HTTP capability, and the replay harness that makes the suite offline.
//!
//! Connectors never construct a client. They are handed an [`HttpCapability`],
//! and which implementation they get decides whether anything reaches the
//! network at all:
//!
//! | Implementation | Behaviour |
//! |---|---|
//! | `LiveHttp` | real requests — **not yet implemented**, increment 5b |
//! | `RecordingHttp` | real requests, saved as fixtures — increment 5b |
//! | [`ReplayHttp`] | fixtures only; **a miss is an error, never a live call** |
//!
//! # Why a cache miss must be an error
//!
//! The tempting design falls back to a live request when a fixture is missing.
//! It is also the design that makes the offline guarantee decorative: the suite
//! passes, quietly contacting real services, and nobody notices until a test
//! fails on a plane or a rate limit. So [`ReplayHttp`] returns
//! [`HttpError::FixtureMissing`] and there is no code path from it to a socket —
//! it holds no client to fall back to (attack catalogue A-018).
//!
//! # Fixture keys
//!
//! A fixture is keyed by `BLAKE3` over a canonical request built from the
//! **same normaliser the evidence model uses** for `canonical_locator`
//! ([`crate::url_norm`]). If replay used its own normaliser, a fixture could
//! match a request the evidence model considers a different page, and the two
//! notions of identity would drift apart silently.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::url_norm::{self, UrlError};

/// Request headers that participate in the fixture key.
///
/// Deliberately a short allowlist. `User-Agent`, `Date`, and cookies vary
/// between runs and machines; including them would make fixtures unmatchable
/// and tempt someone to "fix" it by loosening the key until it matched
/// anything.
const KEYED_HEADERS: &[&str] = &["accept", "accept-language", "content-type"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    #[error("url error: {0}")]
    Url(#[from] UrlError),

    #[error(
        "no fixture for {method} {url} (key {key}). Under replay this is a test failure, never a live request - record the fixture deliberately."
    )]
    FixtureMissing {
        method: String,
        url: String,
        key: String,
    },

    #[error("fixture {path} could not be read: {reason}")]
    FixtureUnreadable { path: String, reason: String },

    #[error("live HTTP is not implemented in this build")]
    LiveNotImplemented,
}

/// A request a connector wants to make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    /// Lowercased header names. `BTreeMap` so ordering is deterministic, which
    /// matters because these are hashed.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".to_string(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: None,
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .insert(name.to_ascii_lowercase(), value.to_string());
        self
    }

    /// The fixture key: `BLAKE3` over the canonical request.
    ///
    /// Uses [`url_norm::normalise`] so this key and `source.canonical_locator`
    /// can never disagree about what counts as the same request.
    pub fn fixture_key(&self) -> Result<String, HttpError> {
        let normalised = url_norm::normalise(&self.url)?;

        let mut hasher = blake3::Hasher::new();
        hasher.update(self.method.to_ascii_uppercase().as_bytes());
        hasher.update(b"\n");
        hasher.update(normalised.canonical_locator.as_bytes());
        hasher.update(b"\n");

        for name in KEYED_HEADERS {
            if let Some(value) = self.headers.get(*name) {
                hasher.update(name.as_bytes());
                hasher.update(b":");
                hasher.update(value.as_bytes());
                hasher.update(b"\n");
            }
        }

        if let Some(body) = &self.body {
            hasher.update(body.as_bytes());
        }

        Ok(hasher.finalize().to_hex().to_string())
    }
}

/// A response, as recorded or replayed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: String,
    /// Where this response came from, recorded so a capture can state honestly
    /// that it was replayed rather than fetched.
    #[serde(default)]
    pub from_fixture: bool,
}

/// A stored request/response pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fixture {
    pub key: String,
    pub request: HttpRequest,
    pub response: HttpResponse,
    /// When and how this was recorded, so a stale fixture can be identified
    /// rather than trusted indefinitely.
    pub recorded_utc: String,
    #[serde(default)]
    pub note: Option<String>,
}

/// The capability a connector is handed. It cannot construct one itself.
pub trait HttpCapability {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Serves recorded fixtures. **Holds no client and cannot make a request.**
///
/// That is a structural property, not a policy: there is no field here that
/// could reach a socket, so "fall back to live on a miss" is not something a
/// future edit can do by accident.
#[derive(Debug, Clone)]
pub struct ReplayHttp {
    dir: PathBuf,
}

impl ReplayHttp {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// Write a fixture. Used by test setup and, later, by `RecordingHttp`.
    pub fn store(&self, fixture: &Fixture) -> Result<(), HttpError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| HttpError::FixtureUnreadable {
            path: self.dir.display().to_string(),
            reason: e.to_string(),
        })?;

        let path = self.path_for(&fixture.key);
        let json =
            serde_json::to_string_pretty(fixture).map_err(|e| HttpError::FixtureUnreadable {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        std::fs::write(&path, json).map_err(|e| HttpError::FixtureUnreadable {
            path: path.display().to_string(),
            reason: e.to_string(),
        })
    }
}

impl HttpCapability for ReplayHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let key = request.fixture_key()?;
        let path = self.path_for(&key);

        let text = std::fs::read_to_string(&path).map_err(|_| HttpError::FixtureMissing {
            method: request.method.clone(),
            url: request.url.clone(),
            key: key.clone(),
        })?;

        let fixture: Fixture =
            serde_json::from_str(&text).map_err(|e| HttpError::FixtureUnreadable {
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        let mut response = fixture.response;
        // Set here rather than trusted from the file, so a capture can never
        // claim to be live when it was replayed.
        response.from_fixture = true;
        Ok(response)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("kokin-fixtures-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn fixture_for(request: &HttpRequest, body: &str) -> Fixture {
        Fixture {
            key: request.fixture_key().unwrap(),
            request: request.clone(),
            response: HttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: body.to_string(),
                from_fixture: false,
            },
            recorded_utc: "2026-08-12T00:00:00Z".to_string(),
            note: None,
        }
    }

    #[test]
    fn a_recorded_fixture_replays() {
        let dir = temp_dir("replay");
        let replay = ReplayHttp::new(&dir);
        let request = HttpRequest::get("https://example.com/page");

        replay
            .store(&fixture_for(&request, "<html>hello</html>"))
            .unwrap();

        let response = replay.send(&request).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "<html>hello</html>");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The property the whole harness exists for.
    #[test]
    fn a_missing_fixture_is_an_error_and_not_a_live_request() {
        let dir = temp_dir("miss");
        let replay = ReplayHttp::new(&dir);

        match replay.send(&HttpRequest::get("https://example.com/never-recorded")) {
            Err(HttpError::FixtureMissing { url, .. }) => {
                assert_eq!(url, "https://example.com/never-recorded");
            }
            other => panic!("expected FixtureMissing, got {other:?}"),
        }
    }

    /// A replayed response must never be able to claim it was live.
    #[test]
    fn a_replayed_response_is_marked_as_coming_from_a_fixture() {
        let dir = temp_dir("marked");
        let replay = ReplayHttp::new(&dir);
        let request = HttpRequest::get("https://example.com/p");

        // Stored with from_fixture deliberately false.
        let mut f = fixture_for(&request, "body");
        f.response.from_fixture = false;
        replay.store(&f).unwrap();

        assert!(
            replay.send(&request).unwrap().from_fixture,
            "a replayed response claimed to be live"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Replay identity and evidence identity must not drift apart: a URL that
    /// differs only by tracking parameters is the same request.
    #[test]
    fn tracking_parameters_do_not_change_the_fixture_key() {
        let a = HttpRequest::get("https://example.com/p?id=1");
        let b = HttpRequest::get("https://example.com/p?id=1&utm_source=twitter");
        assert_eq!(a.fixture_key().unwrap(), b.fixture_key().unwrap());
    }

    #[test]
    fn a_fixture_recorded_without_tracking_serves_a_request_with_it() {
        let dir = temp_dir("tracking");
        let replay = ReplayHttp::new(&dir);

        let clean = HttpRequest::get("https://example.com/article?id=7");
        replay.store(&fixture_for(&clean, "article")).unwrap();

        let tracked = HttpRequest::get("https://example.com/article?id=7&fbclid=abc&utm_medium=x");
        assert_eq!(replay.send(&tracked).unwrap().body, "article");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn different_urls_have_different_keys() {
        let a = HttpRequest::get("https://example.com/a");
        let b = HttpRequest::get("https://example.com/b");
        assert_ne!(a.fixture_key().unwrap(), b.fixture_key().unwrap());
    }

    #[test]
    fn the_method_participates_in_the_key() {
        let get = HttpRequest::get("https://example.com/p");
        let mut post = get.clone();
        post.method = "POST".to_string();
        assert_ne!(get.fixture_key().unwrap(), post.fixture_key().unwrap());
    }

    /// Volatile headers must not enter the key, or fixtures become unmatchable
    /// across machines and someone loosens the key until it matches anything.
    #[test]
    fn volatile_headers_do_not_change_the_key() {
        let base = HttpRequest::get("https://example.com/p");
        let with_ua = base
            .clone()
            .with_header("User-Agent", "kokin/0.0.1")
            .with_header("Cookie", "session=abc");
        assert_eq!(base.fixture_key().unwrap(), with_ua.fixture_key().unwrap());
    }

    #[test]
    fn a_keyed_header_does_change_the_key() {
        let base = HttpRequest::get("https://example.com/p");
        let json = base.clone().with_header("Accept", "application/json");
        assert_ne!(base.fixture_key().unwrap(), json.fixture_key().unwrap());
    }

    #[test]
    fn a_malformed_url_fails_before_any_lookup() {
        let bad = HttpRequest::get("not a url");
        assert!(matches!(bad.fixture_key(), Err(HttpError::Url(_))));
    }

    #[test]
    fn a_corrupt_fixture_file_is_named_as_such() {
        let dir = temp_dir("corrupt");
        let replay = ReplayHttp::new(&dir);
        let request = HttpRequest::get("https://example.com/p");
        replay.store(&fixture_for(&request, "body")).unwrap();

        let key = request.fixture_key().unwrap();
        std::fs::write(dir.join(format!("{key}.json")), "{ not json").unwrap();

        match replay.send(&request) {
            Err(HttpError::FixtureUnreadable { .. }) => {}
            other => panic!("expected FixtureUnreadable, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
