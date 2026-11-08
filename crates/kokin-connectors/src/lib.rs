//! Closed, declarative public-source plans. Networking exists only in kokin-net.
use kokin_net::HttpRequest;
use serde::{Deserialize, Serialize};
pub mod directory;
pub mod enrichment;
pub mod research;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid connector manifest or input: {0}")]
    Invalid(String),
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub id: String,
    pub destinations: Vec<String>,
    pub transmitted_fields: Vec<String>,
    pub capability: String,
    pub method: String,
    pub rate_per_second: u32,
    pub concurrency: u32,
    pub pagination: String,
    pub cancellation: bool,
    pub attribution: String,
    pub fixtures_version: u32,
    pub health: String,
    pub auth: String,
    pub browser_state: bool,
    pub max_response_bytes: u64,
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.id.is_empty()
            || self.id.len() > 32
            || self.capability != "public_https_get"
            || self.method != "GET"
            || self.rate_per_second != 1
            || self.concurrency != 1
            || !self.cancellation
            || self.auth != "none"
            || self.browser_state
            || self.fixtures_version != 2
            || self.max_response_bytes > 8 * 1024 * 1024
            || self.health != "unverified"
            || self.attribution != self.id
            || !matches!(
                self.id.as_str(),
                "dns" | "rdap" | "crt_sh" | "wayback" | "wayback_capture"
            )
            || self.max_response_bytes == 0
            || self.destinations.is_empty()
            || self.transmitted_fields.is_empty()
            || self.transmitted_fields.iter().any(|v| {
                !matches!(
                    v.as_str(),
                    "domain" | "record_type" | "url" | "page" | "timestamp"
                )
            })
        {
            return Err(Error::Invalid(
                "undeclared capabilities, fields or limits".into(),
            ));
        }
        for origin in &self.destinations {
            let url = url::Url::parse(origin).map_err(|_| Error::Invalid("destination".into()))?;
            if url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port_or_known_default() != Some(443)
                || !matches!(url.host(), Some(url::Host::Domain(_)))
                || url.host_str().is_none_or(|host| domain(host).is_err())
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(Error::Invalid("destination must be public HTTPS".into()));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct Plan {
    pub manifest: Manifest,
    pub normalized_input: String,
    pub requests: Vec<HttpRequest>,
    pub limitation: String,
}
fn manifest(id: &str, destination: String, fields: &[&str], pagination: &str) -> Manifest {
    Manifest {
        version: 1,
        id: id.into(),
        destinations: vec![destination],
        transmitted_fields: fields.iter().map(|s| (*s).into()).collect(),
        capability: "public_https_get".into(),
        method: "GET".into(),
        rate_per_second: 1,
        concurrency: 1,
        pagination: pagination.into(),
        cancellation: true,
        attribution: id.into(),
        fixtures_version: 2,
        health: "unverified".into(),
        auth: "none".into(),
        browser_state: false,
        max_response_bytes: 8 * 1024 * 1024,
    }
}
fn domain(value: &str) -> Result<String> {
    if value.trim().len() > 253 || value.contains(['/', ':', '@', '?', '#', '\\']) {
        return Err(Error::Invalid(
            "enter a domain name, not a URL or address".into(),
        ));
    }
    let host = url::Host::parse(value.trim().trim_end_matches('.'))
        .map_err(|_| Error::Invalid("domain".into()))?;
    let url::Host::Domain(domain) = host else {
        return Err(Error::Invalid("IP addresses are not domain names".into()));
    };
    if !domain.contains('.')
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
        || [".local", ".localhost", ".internal", ".lan"]
            .iter()
            .any(|suffix| domain.ends_with(suffix))
    {
        return Err(Error::Invalid(
            "enter a public fully qualified domain".into(),
        ));
    }
    Ok(domain)
}
fn get(url: url::Url, accept: &str) -> HttpRequest {
    let mut request = HttpRequest::get(url.as_str());
    request.headers.insert("accept".into(), accept.into());
    request
}
pub fn dns(input: &str, kind: &str) -> Result<Plan> {
    if !["A", "AAAA", "CNAME", "MX", "NS", "TXT", "SOA"].contains(&kind) {
        return Err(Error::Invalid("unsupported DNS record type".into()));
    }
    let domain = domain(input)?;
    let mut url = url::Url::parse("https://cloudflare-dns.com/dns-query")
        .map_err(|_| Error::Invalid("fixed endpoint".into()))?;
    url.query_pairs_mut()
        .append_pair("name", &domain)
        .append_pair("type", kind);
    let manifest = manifest(
        "dns",
        "https://cloudflare-dns.com/".into(),
        &["domain", "record_type"],
        "none",
    );
    manifest.validate()?;
    Ok(Plan{manifest,normalized_input:domain,requests:vec![get(url,"application/dns-json")],limitation:"Point-in-time public resolver response; no answer does not establish that records never existed. DNSSEC flags are source metadata, not an authenticity assessment.".into()})
}
pub fn rdap(input: &str) -> Result<Plan> {
    let domain = domain(input)?;
    let tld = domain
        .rsplit('.')
        .next()
        .ok_or_else(|| Error::Invalid("TLD".into()))?;
    let bootstrap: serde_json::Value = serde_json::from_str(include_str!(
        "../../../resources/iana-rdap-dns-2026-09-30.json"
    ))
    .map_err(|_| Error::Invalid("reviewed bootstrap".into()))?;
    let services = bootstrap["services"]
        .as_array()
        .ok_or_else(|| Error::Invalid("bootstrap services".into()))?;
    let base = services
        .iter()
        .filter(|row| {
            row[0]
                .as_array()
                .is_some_and(|names| names.iter().any(|name| name.as_str() == Some(tld)))
        })
        .flat_map(|row| row[1].as_array().into_iter().flatten())
        .filter_map(|value| value.as_str())
        .find(|base| base.starts_with("https://"))
        .ok_or_else(|| {
            Error::Invalid(
                "no reviewed HTTPS RDAP service for this TLD; no HTTP or WHOIS fallback".into(),
            )
        })?;
    let origin = url::Url::parse(base).map_err(|_| Error::Invalid("RDAP service URL".into()))?;
    let request = origin
        .join(&format!("domain/{domain}"))
        .map_err(|_| Error::Invalid("RDAP domain request".into()))?;
    let manifest = manifest(
        "rdap",
        origin.origin().ascii_serialization(),
        &["domain"],
        "none",
    );
    manifest.validate()?;
    Ok(Plan{manifest,normalized_input:domain,requests:vec![get(request,"application/rdap+json")],limitation:"IANA bootstrap 2026-09-30, SHA-256 e3419eb0b28dcc3a68dd3bb0e64429cb0d376c3883ca4c42317aea65fc0ab471. Registry data may be redacted or incomplete; undeclared cross-origin redirects are refused.".into()})
}
/// Bounded exact-domain certificate discovery. Similar names are not identities.
pub fn certificates(input: &str) -> Result<Plan> {
    let name = domain(input)?;
    let mut url =
        url::Url::parse("https://crt.sh/").map_err(|_| Error::Invalid("endpoint".into()))?;
    url.query_pairs_mut()
        .append_pair("q", &name)
        .append_pair("output", "json");
    let m = manifest(
        "crt_sh",
        "https://crt.sh/".into(),
        &["domain"],
        "bounded_response_no_cursor",
    );
    m.validate()?;
    Ok(Plan {manifest:m,normalized_input:name,requests:vec![get(url,"application/json")],limitation:"Certificate Transparency discovery is response-bounded and may be incomplete. A certificate name does not prove control, identity, or current ownership.".into()})
}
fn public_url(input: &str) -> Result<url::Url> {
    let url =
        url::Url::parse(input).map_err(|_| Error::Invalid("enter a public HTTP(S) URL".into()))?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str().len() > 4096
    {
        return Err(Error::Invalid(
            "unsupported URL, credentials, or fragment".into(),
        ));
    }
    domain(
        url.host_str()
            .ok_or_else(|| Error::Invalid("URL host".into()))?,
    )?;
    if !matches!(url.port_or_known_default(), Some(80) | Some(443)) {
        return Err(Error::Invalid("nonstandard URL port".into()));
    }
    Ok(url)
}
/// Explicit CDX pages: each page is a separately disclosed request.
pub fn wayback(input: &str, page: u32) -> Result<Plan> {
    if page > 100 {
        return Err(Error::Invalid("archive page limit reached".into()));
    }
    let original = public_url(input)?;
    let mut url = url::Url::parse("https://web.archive.org/cdx/search/cdx")
        .map_err(|_| Error::Invalid("endpoint".into()))?;
    url.query_pairs_mut()
        .append_pair("url", original.as_str())
        .append_pair("output", "json")
        .append_pair("fl", "timestamp,original,mimetype,statuscode,digest")
        .append_pair("pageSize", "100")
        .append_pair("page", &page.to_string());
    let m = manifest(
        "wayback",
        "https://web.archive.org/".into(),
        &["url", "page"],
        "explicit_page_0_to_100",
    );
    m.validate()?;
    Ok(Plan{manifest:m,normalized_input:original.into(),requests:vec![get(url,"application/json")],limitation:"CDX capture discovery only. Pages contain at most 100 index blocks and responses remain bounded to 8 MiB; omitted history is not absence. Retrieving a capture is a separate collection action.".into()})
}
pub fn archived_page(input: &str, timestamp: &str) -> Result<Plan> {
    let original = public_url(input)?;
    if timestamp.len() != 14 || !timestamp.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::Invalid(
            "archive timestamp must contain 14 digits".into(),
        ));
    }
    let url = url::Url::parse(&format!(
        "https://web.archive.org/web/{timestamp}id_/{}",
        original.as_str()
    ))
    .map_err(|_| Error::Invalid("archive URL".into()))?;
    let m = manifest(
        "wayback_capture",
        "https://web.archive.org/".into(),
        &["url", "timestamp"],
        "none",
    );
    m.validate()?;
    Ok(Plan{manifest:m,normalized_input:original.into(),requests:vec![HttpRequest::get(url.as_str())],limitation:"Static archived response only. No JavaScript, browser session, or live-site fallback. Missing Memento metadata is recorded as an unverified archive response.".into()})
}
pub fn plan(
    kind: &str,
    input: &str,
    record_type: &str,
    page: u32,
    timestamp: &str,
) -> Result<Plan> {
    match kind {
        "dns" => dns(input, record_type),
        "rdap" => rdap(input),
        "crt_sh" => certificates(input),
        "wayback" => wayback(input, page),
        "wayback_capture" => archived_page(input, timestamp),
        _ => Err(Error::Invalid("unknown connector".into())),
    }
}
pub mod tabular;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn dns_types_and_disclosure_are_closed() {
        for kind in ["A", "AAAA", "CNAME", "MX", "NS", "TXT", "SOA"] {
            let plan = dns("EXAMPLE.org.", kind).unwrap();
            assert_eq!(plan.normalized_input, "example.org");
            assert_eq!(plan.requests[0].headers["accept"], "application/dns-json");
            assert_eq!(
                plan.manifest.transmitted_fields,
                vec!["domain", "record_type"]
            );
        }
        assert!(dns("example.org", "AXFR").is_err());
        for value in [
            "127.0.0.1",
            "[::1]",
            "http://example.org",
            "private.local",
            "user@example.org",
        ] {
            assert!(dns(value, "A").is_err());
        }
    }
    #[test]
    fn rdap_uses_reviewed_registry_and_has_no_insecure_fallback() {
        let plan = rdap("example.com").unwrap();
        assert!(plan.requests[0]
            .url
            .starts_with("https://rdap.verisign.com/com/v1/domain/"));
        assert!(rdap("example.kg").is_err());
    }
    #[test]
    fn certificate_and_archive_requests_are_bounded_and_separate() {
        let cert = certificates("example.org").unwrap();
        assert_eq!(cert.manifest.max_response_bytes, 8 * 1024 * 1024);
        let discovery = wayback("https://example.org/path?a=1", 2).unwrap();
        assert!(discovery.requests[0].url.contains("page=2"));
        assert!(!discovery.requests[0].url.contains("id_"));
        let capture = archived_page("https://example.org/path", "20260101000000").unwrap();
        assert!(capture.requests[0]
            .url
            .starts_with("https://web.archive.org/web/20260101000000id_/"));
        assert!(capture.requests[0].headers.is_empty());
        for url in [
            "http://127.0.0.1/",
            "https://user:secret@example.org/",
            "https://private.local/",
            "https://example.org/#secret",
        ] {
            assert!(wayback(url, 0).is_err());
        }
        assert!(wayback("https://example.org/", 101).is_err());
        assert!(archived_page("https://example.org/", "../evil").is_err());
        let mut m = cert.manifest;
        m.destinations = vec!["https://localhost/".into()];
        assert!(m.validate().is_err());
    }
    #[test]
    fn unsafe_manifests_are_refused() {
        let mut m = dns("example.org", "A").unwrap().manifest;
        m.transmitted_fields.push("case_key".into());
        assert!(m.validate().is_err());
        m.transmitted_fields.pop();
        m.destinations = vec!["http://127.0.0.1/".into()];
        assert!(m.validate().is_err());
    }
}

pub mod profiles;

pub mod wallet;

pub mod candidates;
