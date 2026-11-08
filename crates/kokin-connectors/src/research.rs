//! Deterministic research planning and classification. No sockets or executable recipes.
use crate::{Error, Result};
use kokin_net::{HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};

pub const MAX_BATCH: usize = 15;
pub const MAX_SOURCES: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorKind {
    Username,
    Email,
    Phone,
    Name,
    Domain,
    Ip,
    Wallet,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub kind: SelectorKind,
    pub original: String,
    pub normalized: String,
}
impl Selector {
    pub fn parse(kind: SelectorKind, value: &str) -> Result<Self> {
        let value = value.trim();
        if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
            return Err(Error::Invalid(
                "empty, oversized or control-bearing selector".into(),
            ));
        }
        let normalized = match kind {
            SelectorKind::Username => {
                if value.len() > 64
                    || !value.bytes().any(|c| c.is_ascii_alphanumeric())
                    || !value
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
                {
                    return Err(Error::Invalid(
                        "username must contain 1–64 letters, numbers, dots, hyphens or underscores"
                            .into(),
                    ));
                }
                value.into()
            }
            SelectorKind::Email => {
                let (local, host) = value
                    .rsplit_once('@')
                    .ok_or_else(|| Error::Invalid("email needs a local part and domain".into()))?;
                if local.is_empty()
                    || local.len() > 64
                    || local.contains('@')
                    || local.starts_with('.')
                    || local.ends_with('.')
                    || local.contains("..")
                    || !local
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&b))
                {
                    return Err(Error::Invalid("supported email local parts use ASCII dot-atom syntax; quoted/SMTPUTF8 local parts require a manual workflow".into()));
                }
                let email = format!("{local}@{}", super::domain(host)?);
                if email.len() > 254 {
                    return Err(Error::Invalid("email exceeds 254 bytes".into()));
                }
                email
            }
            SelectorKind::Domain => {
                let host = if value.contains("://") {
                    let u =
                        url::Url::parse(value).map_err(|_| Error::Invalid("domain URL".into()))?;
                    if !matches!(u.scheme(), "http" | "https")
                        || !u.username().is_empty()
                        || u.password().is_some()
                    {
                        return Err(Error::Invalid(
                            "domain URL must not contain credentials".into(),
                        ));
                    }
                    u.host_str()
                        .ok_or_else(|| Error::Invalid("domain host".into()))?
                        .to_owned()
                } else {
                    value.into()
                };
                super::domain(&host)?
            }
            SelectorKind::Phone => {
                let clean: String = value
                    .chars()
                    .filter(|c| !matches!(c, ' ' | '(' | ')' | '-' | '.'))
                    .collect();
                let digits = clean.strip_prefix('+').ok_or_else(|| {
                    Error::Invalid(
                        "use an international phone number beginning with + and country code"
                            .into(),
                    )
                })?;
                if !(7..=15).contains(&digits.len())
                    || digits.starts_with('0')
                    || !digits.bytes().all(|c| c.is_ascii_digit())
                {
                    return Err(Error::Invalid("invalid international phone format; formatting does not establish an account".into()));
                }
                clean
            }
            SelectorKind::Name => {
                let name = value.split_whitespace().collect::<Vec<_>>().join(" ");
                if name.chars().count() > 200 {
                    return Err(Error::Invalid("name is too long".into()));
                }
                name
            }
            SelectorKind::Ip => {
                if !value.contains(':') {
                    let parts = value.split('.').collect::<Vec<_>>();
                    if parts.len() != 4
                        || parts.iter().any(|p| {
                            p.is_empty()
                                || (p.len() > 1 && p.starts_with('0'))
                                || !p.bytes().all(|c| c.is_ascii_digit())
                                || p.parse::<u8>().is_err()
                        })
                    {
                        return Err(Error::Invalid("use four decimal IPv4 octets; abbreviated, hexadecimal and octal forms are not accepted".into()));
                    }
                }
                let input = if value.contains(':') && !value.starts_with('[') {
                    format!("[{value}]")
                } else {
                    value.into()
                };
                match url::Host::parse(&input)
                    .map_err(|_| Error::Invalid("invalid IP address".into()))?
                {
                    url::Host::Ipv4(ip) => ip.to_string(),
                    url::Host::Ipv6(ip) => ip.to_string(),
                    _ => return Err(Error::Invalid("enter an IPv4 or IPv6 address".into())),
                }
            }
            SelectorKind::Wallet => crate::wallet::normalize(value)?,
            SelectorKind::Image => {
                if value.len() > 128
                    || !value
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
                {
                    return Err(Error::Invalid("select an imported image artifact; arbitrary paths/URLs are not artifact IDs".into()));
                }
                value.into()
            }
        };
        Ok(Self {
            kind,
            original: value.into(),
            normalized,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Native,
    ProviderBacked,
    ManualWorkflow,
    Blocked,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorManifestV2 {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub category: String,
    pub selectors: Vec<SelectorKind>,
    pub destinations: Vec<String>,
    pub methods: Vec<String>,
    pub transmitted_fields: Vec<String>,
    pub delivery: Delivery,
    pub credential_ref: Option<String>,
    pub max_response_bytes: u64,
    pub rate_per_second: u32,
    pub concurrency: u32,
    pub max_calls: usize,
    pub health: String,
    pub limitation: String,
    pub adult: bool,
    pub cache_max_age_seconds: u32,
    pub profile_rule: Option<crate::profiles::ProfileRule>,
}
fn manifest(id: &str, title: &str, host: &str, kinds: Vec<SelectorKind>) -> ConnectorManifestV2 {
    ConnectorManifestV2 { version: 2, id: id.into(), title: title.into(), category: "Public profiles".into(),
        selectors: kinds, destinations: if host.is_empty(){vec![]}else{vec![format!("https://{host}")]}, methods: vec!["GET".into()],
        transmitted_fields: vec!["selector".into()], delivery: Delivery::Native, credential_ref: None,
        max_response_bytes: 2 * 1024 * 1024, rate_per_second: 1, concurrency: 1, max_calls: 1,
        health: "fixture-tested; live health unverified".into(), limitation: "Public profile existence is a lead, not identity proof. Rate limits and challenge pages remain unknown/blocked.".into(), adult: false, cache_max_age_seconds: 8 * 3600, profile_rule: crate::profiles::rule(id).cloned() }
}
pub fn registry() -> Vec<ConnectorManifestV2> {
    use SelectorKind::*;
    let mut rows = vec![
        manifest(
            "github",
            "GitHub public user",
            "api.github.com",
            vec![Username],
        ),
        manifest(
            "codeberg",
            "Codeberg public user",
            "codeberg.org",
            vec![Username],
        ),
        manifest(
            "hackernews",
            "Hacker News public user",
            "hacker-news.firebaseio.com",
            vec![Username],
        ),
        manifest("dns", "DNS records", "cloudflare-dns.com", vec![Domain]),
        manifest("rdap", "Domain registration · RDAP", "", vec![Domain]),
        manifest("crt_sh", "Certificate Transparency", "crt.sh", vec![Domain]),
    ];
    for row in &mut rows {
        if matches!(row.id.as_str(), "dns" | "rdap" | "crt_sh") {
            row.category = "Public records".into();
            row.max_response_bytes = 8 * 1024 * 1024;
            row.limitation=match row.id.as_str(){"dns"=>"Point-in-time DNS records; missing answers do not establish registration availability or account ownership.","rdap"=>"Reviewed registry registration response; records may be redacted or incomplete. No ownership identity or registration price is inferred.",_=>"Historical certificate observations; they do not establish current ownership, registration availability or account association."}.into();
        }
    }
    let mut discovery = manifest(
        "searxng",
        "Name/text discovery · local SearXNG",
        "",
        vec![Name],
    );
    discovery.category = "Text discovery".into();
    discovery.transmitted_fields = vec!["name/text query, optional exact phrase, page 1".into()];
    discovery.destinations = vec!["http://127.0.0.1:8088".into()];
    discovery.cache_max_age_seconds = 0;
    discovery.max_response_bytes = 4 * 1024 * 1024;
    discovery.health =
        "optional local SearXNG service required; fixture-tested, live health unverified".into();
    discovery.limitation="One disclosed request to local SearXNG; its configured upstream engines see the query and may make multiple requests. The app call ceiling does not count engine-internal fanout. Search snippets are leads; destination pages are not collected. This does not establish account association or identity.".into();
    rows.push(discovery);
    let mut archive = manifest(
        "wayback",
        "Archived page discovery · Wayback",
        "web.archive.org",
        vec![Domain],
    );
    archive.category = "Public records".into();
    archive.transmitted_fields = vec!["HTTPS domain root, CDX page 0".into()];
    archive.max_response_bytes = 8 * 1024 * 1024;
    archive.limitation="Historical CDX discovery for the HTTPS domain root only. Use Public records for an explicit full-URL archive query. No archived page is retrieved automatically. Missing captures do not establish historical absence, ownership or registration availability.".into();
    rows.push(archive);
    for (id, title, host, kinds) in [
        (
            "osint_industries",
            "OSINT Industries · official API",
            "api.osint.industries",
            vec![Email, Phone, Username, Name, Wallet],
        ),
        (
            "lenso",
            "Lenso · image categories",
            "api.lenso.ai",
            vec![Image],
        ),
        (
            "eyematch",
            "EYEMATCH · eligible face search",
            "api.eyematch.ai",
            vec![Image],
        ),
    ] {
        let mut m = manifest(id, title, host, kinds);
        m.delivery = Delivery::ProviderBacked;
        m.cache_max_age_seconds = 0;
        m.methods = vec!["POST".into()];
        m.credential_ref = Some(id.into());
        m.health = "partial contract; credential check required; live health unverified".into();
        m.max_response_bytes = 8 * 1024 * 1024;
        m.category = "Optional providers".into();
        m.limitation = "Requires your own official API entitlement and a separately authorized call budget. No undocumented endpoint fallback.".into();
        rows.push(m);
    }
    let mut credits = manifest(
        "osint_industries_credits",
        "OSINT Industries · credit balance",
        "api.osint.industries",
        vec![Username, Email, Phone, Name, Domain, Ip, Wallet, Image],
    );
    credits.delivery = Delivery::ProviderBacked;
    credits.credential_ref = Some("osint_industries".into());
    credits.transmitted_fields = vec!["native credential only; no query or image".into()];
    credits.max_response_bytes = 64 * 1024;
    credits.cache_max_age_seconds = 0;
    credits.category = "Optional providers".into();
    credits.health = "offline contract tested; live health and entitlement unverified".into();
    credits.limitation = "Explicit account usage check only, once per batch. No selector or image is transmitted or searched. The provider-reported balance is an observation, not a spending reservation or eligibility confirmation. No automatic refresh.".into();
    rows.push(credits);
    for (id, title) in [
        ("pimeyes", "PimEyes"),
        ("instantusername", "InstantUsername"),
        ("whatsmyname", "WhatsMyName"),
        (
            "osint_industries_web",
            "OSINT Industries · manual web interfaces",
        ),
    ] {
        let mut m = manifest(
            id,
            title,
            "",
            if id == "pimeyes" {
                vec![Image]
            } else if id == "osint_industries_web" {
                vec![Ip, Domain, Image]
            } else {
                vec![Username]
            },
        );
        m.delivery = Delivery::ManualWorkflow;
        m.cache_max_age_seconds = 0;
        m.methods.clear();
        m.transmitted_fields.clear();
        m.max_calls = 0;
        m.category = "Browser workflows".into();
        m.health = "manual workflow".into();
        m.limitation="Open the official website yourself and import the resulting evidence. No automatic upload or internal API access.".into();
        rows.push(m);
    }
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Found,
    NotFound,
    Taken,
    Available,
    Invalid,
    Unknown,
    Blocked,
    Error,
    Unsupported,
}
impl CheckOutcome {
    pub fn cacheable(self) -> bool {
        matches!(
            self,
            Self::Found | Self::NotFound | Self::Taken | Self::Available
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchResult {
    pub id: String,
    pub run_id: String,
    pub selector: Selector,
    pub source: String,
    pub outcome: CheckOutcome,
    pub url: String,
    pub fields: serde_json::Value,
    pub observed_utc: String,
    pub source_reported_utc: Option<String>,
    pub indexed_utc: Option<String>,
    pub artifact_id: Option<String>,
    pub from_cache: bool,
    pub limitation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchRequest {
    pub version: u32,
    pub selectors: Vec<Selector>,
    pub sources: Vec<String>,
    pub rescan: bool,
    pub safe_mode: bool,
    #[serde(default)]
    pub options: ProviderOptions,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderOptions {
    pub paid_authorized: bool,
    pub face_eligible: bool,
    pub premium: bool,
    pub exact_match: bool,
    pub image_category: String,
    pub page: u32,
    pub domain_filter: String,
    pub sort_type: String,
}
impl Default for ProviderOptions {
    fn default() -> Self {
        Self {
            paid_authorized: false,
            face_eligible: false,
            premium: false,
            exact_match: true,
            image_category: "similar".into(),
            page: 1,
            domain_filter: String::new(),
            sort_type: "QUALITY_DESCENDING".into(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDisclosure {
    pub artifact_id: String,
    pub content_hash: String,
    pub media_type: String,
    pub byte_length: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputOrigin {
    pub selector: Selector,
    pub observation_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportParent {
    pub id: String,
    pub revision: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchPlan {
    #[serde(default)]
    pub parent_import: Option<ImportParent>,
    pub version: u32,
    pub generation: u64,
    pub selectors: Vec<Selector>,
    pub sources: Vec<String>,
    pub destinations: Vec<String>,
    pub request_urls: Vec<String>,
    pub max_calls: usize,
    pub rescan: bool,
    pub safe_mode: bool,
    pub limitation: String,
    pub request_digests: Vec<String>,
    pub provider_options: ProviderOptions,
    pub image_disclosures: Vec<ImageDisclosure>,
    #[serde(default)]
    pub input_origins: Vec<InputOrigin>,
    pub provider_call_limits: std::collections::BTreeMap<String, usize>,
    pub provider_credit_upper_bounds: std::collections::BTreeMap<String, usize>,
}
#[derive(Debug, Clone)]
pub struct CheckPlan {
    pub authorized_request_digest: Option<String>,
    pub validation_error: Option<String>,
    pub source: String,
    pub selector: Selector,
    pub request: HttpRequest,
    pub manifest: ConnectorManifestV2,
}
pub fn checks(
    request: &ResearchRequest,
    generation: u64,
) -> Result<(ResearchPlan, Vec<CheckPlan>)> {
    if request.version != 1
        || request.selectors.is_empty()
        || request.selectors.len() > MAX_BATCH
        || request.sources.is_empty()
        || request.sources.len() > MAX_SOURCES
    {
        return Err(Error::Invalid(
            "choose 1–15 selectors and 1–100 sources".into(),
        ));
    }
    let registry = registry();
    let mut selectors = Vec::new();
    let mut checks = Vec::new();
    let mut urls = Vec::new();
    let mut destinations = Vec::new();
    let mut provider_call_limits = std::collections::BTreeMap::<String, usize>::new();
    let mut provider_credit_upper_bounds = std::collections::BTreeMap::<String, usize>::new();
    let opts = &request.options;
    if opts.page == 0
        || opts.page > 500
        || !["duplicates", "landmarks", "similar", "related"]
            .contains(&opts.image_category.as_str())
        || ![
            "QUALITY_DESCENDING",
            "QUALITY_ASCENDING",
            "DATE_DESCENDING",
            "DATE_ASCENDING",
        ]
        .contains(&opts.sort_type.as_str())
        || opts.domain_filter.len() > 253
    {
        return Err(Error::Invalid(
            "unsupported provider pagination, category, domain or sort option".into(),
        ));
    }
    if !opts.domain_filter.is_empty() {
        super::domain(&opts.domain_filter)?;
    }
    let mut sources = request.sources.clone();
    sources.sort();
    sources.dedup();
    for raw in &request.selectors {
        let s = Selector::parse(raw.kind, &raw.original)?;
        if s.normalized != raw.normalized {
            return Err(Error::Invalid(
                "selector normalization changed; review the query again".into(),
            ));
        }
        if selectors
            .iter()
            .any(|v: &Selector| v.kind == s.kind && v.normalized == s.normalized)
        {
            continue;
        }
        selectors.push(s.clone());
        let mut compatible = false;
        for id in &sources {
            let mut m = registry
                .iter()
                .find(|v| &v.id == id)
                .cloned()
                .ok_or_else(|| Error::Invalid("unknown source definition".into()))?;
            if !m.selectors.contains(&s.kind) || (request.safe_mode && m.adult) {
                continue;
            }
            if m.delivery == Delivery::ManualWorkflow || m.delivery == Delivery::Blocked {
                return Err(Error::Invalid(format!(
                    "{} is a browser/import workflow, not an automated connector",
                    m.title
                )));
            }
            if m.delivery == Delivery::ProviderBacked && !opts.paid_authorized {
                return Err(Error::Invalid("authorize the displayed provider call/credit budget before preparing paid lookups".into()));
            }
            if id == "eyematch" && !opts.face_eligible {
                return Err(Error::Invalid("confirm the facial provider's eligibility/consent requirements before preparing an image upload".into()));
            }
            compatible = true;
            if id == "osint_industries_credits"
                && checks.iter().any(|c: &CheckPlan| c.source == *id)
            {
                continue;
            }
            let validation_error = m
                .profile_rule
                .as_ref()
                .and_then(|r| r.invalid_username(&s.normalized));
            let request = match id.as_str() {
                "github" | "codeberg" | "hackernews" => crate::profiles::rule(id)
                    .ok_or_else(|| Error::Invalid("missing reviewed recipe".into()))?
                    .request(&s),
                "searxng" => {
                    let query = if opts.exact_match {
                        format!(
                            "\"{}\"",
                            s.normalized.replace('\\', "\\\\").replace('"', "\\\"")
                        )
                    } else {
                        s.normalized.clone()
                    };
                    HttpRequest::get(
                        kokin_net::discovery::query_url(&query, 1)
                            .map_err(|e| Error::Invalid(e.to_string()))?,
                    )
                    .with_header("accept", "application/json")
                }
                "wayback" => super::wayback(&format!("https://{}/", s.normalized), 0)?
                    .requests
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::Invalid("archive request missing".into()))?,
                "dns" | "rdap" | "crt_sh" => {
                    let p = super::plan(id, &s.normalized, "A", 0, "")?;
                    m.destinations = p.manifest.destinations;
                    m.max_response_bytes = p.manifest.max_response_bytes;
                    p.requests
                        .into_iter()
                        .next()
                        .ok_or_else(|| Error::Invalid("source has no request".into()))?
                }
                "osint_industries_credits" => {
                    provider_call_limits.insert(id.clone(), 1);
                    provider_credit_upper_bounds.insert(id.clone(), 0);
                    HttpRequest::get("https://api.osint.industries/misc/credits")
                }
                "osint_industries" => {
                    *provider_call_limits.entry(id.clone()).or_default() += 1;
                    *provider_credit_upper_bounds.entry(id.clone()).or_default() +=
                        if opts.premium { 4 } else { 1 };
                    let mut r = HttpRequest::get("https://api.osint.industries/v2/request");
                    r.method = "POST".into();
                    r.body=Some(serde_json::json!({"type":s.kind,"query":s.normalized,"timeout":60,"exact_match":opts.exact_match,"premium":opts.premium}).to_string());
                    r
                }
                "lenso" | "eyematch" => {
                    *provider_call_limits.entry(id.clone()).or_default() += 1;
                    let mut r = HttpRequest::get(if id == "lenso" {
                        "https://api.lenso.ai/search"
                    } else {
                        "https://api.eyematch.ai/search"
                    });
                    r.method = "POST".into();
                    // Replaced by image bytes only inside native execution; never sent as an artifact ID.
                    let mut body = serde_json::json!({"image_artifact_id":s.normalized,"sortType":opts.sort_type,"domain":opts.domain_filter,"page":opts.page});
                    if id == "lenso" {
                        body["category"] = opts.image_category.clone().into();
                    }
                    r.body = Some(body.to_string());
                    r
                }
                _ => return Err(Error::Invalid("source is not executable".into())),
            };
            if validation_error.is_none() {
                destinations.extend(m.destinations.clone());
                urls.push(request.url.clone());
            }
            checks.push(CheckPlan {
                authorized_request_digest: None,
                validation_error,
                source: id.clone(),
                selector: s.clone(),
                request,
                manifest: m,
            });
        }
        if !compatible {
            return Err(Error::Invalid(format!("No selected automated source supports {:?} query {:?}. Choose a compatible source or remove this row and use its browser/import workflow. Nothing was sent.",s.kind,s.original)));
        }
    }
    if checks.is_empty() {
        return Err(Error::Invalid("no selected native source supports these selector types; choose a provider or browser workflow".into()));
    }
    destinations.sort();
    destinations.dedup();
    let request_digests = checks
        .iter()
        .map(|c| {
            c.request
                .fixture_key()
                .map_err(|_| Error::Invalid("request digest unavailable".into()))
        })
        .collect::<Result<Vec<_>>>()?;
    let limitation = if sources.iter().any(|s| s == "osint_industries_credits") {
        "Only the reviewed lookups will run. Credit balance is an explicit account-usage request only: no selector/image is sent to that endpoint and no selected query is searched by that source. Its result records provider-reported balance at observation time, not a spending reservation. Other selected providers see their disclosed queries/images and your network address. No automatic retries or refresh. Results are leads, not verified identities."
    } else {
        "Only the reviewed lookups will run. Providers see the selected query/image and your network address. API requests may consume credits even if cancelled or timed out; there is no automatic retry. Results are leads, not verified identities."
    };
    Ok((
        ResearchPlan {
            parent_import: None,
            request_digests,
            provider_options: request.options.clone(),
            image_disclosures: vec![],
            input_origins: vec![],
            version: 1,
            generation,
            selectors,
            sources,
            destinations,
            request_urls: urls,
            max_calls: checks
                .iter()
                .filter(|c| c.validation_error.is_none())
                .count(),
            rescan: request.rescan,
            safe_mode: request.safe_mode,
            provider_call_limits,
            provider_credit_upper_bounds,
            limitation: limitation.into(),
        },
        checks,
    ))
}

pub fn classify(
    source: &str,
    response: &HttpResponse,
    selector: &Selector,
) -> (CheckOutcome, String, serde_json::Value) {
    use CheckOutcome::*;
    if let Some(rule) = crate::profiles::rule(source) {
        return rule.classify(response, selector);
    }
    let bytes = response
        .body_bytes
        .as_deref()
        .unwrap_or(response.body.as_bytes());
    if matches!(response.status, 401 | 402 | 403 | 429) {
        return (
            Blocked,
            String::new(),
            serde_json::json!({"status":response.status}),
        );
    }
    if response.status >= 500 {
        return (
            Error,
            String::new(),
            serde_json::json!({"status":response.status}),
        );
    }
    let value: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => {
            return (
                Unknown,
                String::new(),
                serde_json::json!({"reason":"response is not the expected JSON; a challenge/error page is not an availability result"}),
            )
        }
    };
    if response.status != 200 {
        return (
            Unknown,
            String::new(),
            serde_json::json!({"status":response.status}),
        );
    }
    match source {
        "osint_industries_credits" => {
            let usage = if let Some(credits) = value["credits"].as_u64() {
                serde_json::json!({"state":if credits == 0 {"exhausted"} else {"credits_available"},"credits":credits,"eligibility":"unverified","coverage":"Provider-reported balance at observation time; not a spending reservation. No query or image was transmitted or searched."})
            } else {
                serde_json::json!({"state":"unknown","eligibility":"unverified","coverage":"Credit response did not contain a nonnegative integer balance; no zero balance or account/search result is inferred."})
            };
            (Unknown, String::new(), serde_json::json!({"usage":usage}))
        }
        "osint_industries" => crate::enrichment::osint_industries(&value, selector),
        "lenso" | "eyematch" if value["results"].is_array() => {
            let empty = value["results"].as_array().is_some_and(|r| r.is_empty());
            (
                if empty { NotFound } else { Found },
                String::new(),
                serde_json::json!({"results":value["results"],"available_pages":value["availablePages"],"score_notice":"Provider similarity scores are not KOKIN confidence or identity probability.","date_notice":"Provider dates describe indexing, not original publication."}),
            )
        }
        "searxng" if value["results"].is_array() => {
            let rows = value["results"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let failed = value["unresponsive_engines"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty());
            let valid = rows
                .iter()
                .filter(|row| {
                    row["url"]
                        .as_str()
                        .is_some_and(|url| kokin_net::browser_handoff(url).is_ok())
                })
                .collect::<Vec<_>>();
            let malformed = valid.len() != rows.len()
                || value.get("error").is_some()
                || (rows.is_empty() && !value["unresponsive_engines"].is_array());
            (
                if failed || malformed {
                    Unknown
                } else if rows.is_empty() {
                    NotFound
                } else {
                    Found
                },
                String::new(),
                serde_json::json!({"discovery_leads":valid.into_iter().take(100).collect::<Vec<_>>(),"unresponsive_engines":value["unresponsive_engines"],"coverage":"Only search response snippets are captured. No match does not establish absence of accounts or identity; destination pages need separate capture."}),
            )
        }
        "wayback" if value.is_array() => {
            let rows = value.as_array().map(Vec::as_slice).unwrap_or_default();
            let header = rows.first().and_then(|v| v.as_array());
            let valid_header = header.is_some_and(|h| {
                h.first().is_some_and(|v| v == "timestamp")
                    && h.get(1).is_some_and(|v| v == "original")
            });
            let valid_rows = rows.iter().skip(1).all(|r| {
                r.as_array().is_some_and(|cells| {
                    cells.len() == 5
                        && cells[0]
                            .as_str()
                            .is_some_and(|t| t.len() == 14 && t.bytes().all(|c| c.is_ascii_digit()))
                        && cells[1].as_str().is_some_and(|u| {
                            super::public_url(u).is_ok_and(|url| {
                                url.host_str().is_some_and(|host| {
                                    host.eq_ignore_ascii_case(&selector.normalized)
                                })
                            })
                        })
                })
            });
            (
                if rows.is_empty() || valid_header && rows.len() == 1 {
                    NotFound
                } else if valid_header && valid_rows {
                    Found
                } else {
                    Unknown
                },
                String::new(),
                serde_json::json!({"capture_index":value,"coverage":"Historical capture discovery only. The source pages and attachments have not been retrieved."}),
            )
        }
        "dns" if value["Status"].is_number() => (
            if value["Status"] == 0 && value["Answer"].as_array().is_some_and(|a| !a.is_empty()) {
                Found
            } else {
                Unknown
            },
            String::new(),
            serde_json::json!({"response":value,"limitation":"DNS responses describe records, not registration availability or account ownership. Missing answers, NXDOMAIN and resolver errors cannot establish a claimable domain."}),
        ),
        "rdap"
            if value["objectClassName"] == "domain"
                && value["ldhName"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&selector.normalized)) =>
        {
            (Found, String::new(), value)
        }
        "crt_sh" if value.is_array() => {
            let empty = value.as_array().is_some_and(|v| v.is_empty());
            let suffix = format!(".{}", selector.normalized.to_ascii_lowercase());
            let matches = value.as_array().is_some_and(|rows| {
                rows.iter().any(|row| {
                    row["name_value"].as_str().is_some_and(|names| {
                        names.lines().any(|name| {
                            name.eq_ignore_ascii_case(&selector.normalized)
                                || name.to_ascii_lowercase().ends_with(&suffix)
                        })
                    })
                })
            });
            (
                if empty {
                    NotFound
                } else if matches {
                    Found
                } else {
                    Unknown
                },
                String::new(),
                serde_json::json!({"certificates":value,"limitation":"Certificate observations are historical and do not establish current registration, ownership or availability."}),
            )
        }
        _ => (
            Unknown,
            String::new(),
            serde_json::json!({"reason":"response did not meet the source's positive/negative contract"}),
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkRow {
    pub value: String,
    pub kind: SelectorKind,
    pub selector: Option<Selector>,
    pub error: Option<String>,
    pub repeats: usize,
    #[serde(default)]
    pub origin_ids: Vec<String>,
}
pub fn parse_bulk(text: &str, kind: SelectorKind) -> Result<Vec<BulkRow>> {
    if text.len() > 2_000_000 {
        return Err(Error::Invalid(
            "text import exceeds 2 million characters".into(),
        ));
    }
    let mut rows: Vec<BulkRow> = Vec::new();
    let mut positions = std::collections::HashMap::<String, usize>::new();
    for value in text
        .split(['\n', '\r', '\t', ',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if rows.len() >= 20_000 {
            return Err(Error::Invalid("import exceeds 20,000 rows".into()));
        }
        let parsed = Selector::parse(kind, value);
        let identity = parsed.as_ref().map_or(value, |s| s.normalized.as_str());
        if let Some(index) = positions.get(identity) {
            rows[*index].repeats += 1;
            continue;
        }
        positions.insert(identity.to_owned(), rows.len());
        let (selector, error) = match parsed {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        rows.push(BulkRow {
            value: value.into(),
            kind,
            selector,
            error,
            repeats: 1,
            origin_ids: vec![],
        });
    }
    Ok(rows)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            body: body.into(),
            headers: Default::default(),
            body_bytes: None,
            redirect_chain: vec![],
            from_fixture: true,
        }
    }
    #[test]
    fn credit_balance_is_one_explicit_usage_call_without_any_query_or_spending() {
        let kinds = [
            SelectorKind::Username,
            SelectorKind::Email,
            SelectorKind::Phone,
            SelectorKind::Name,
            SelectorKind::Domain,
            SelectorKind::Ip,
            SelectorKind::Wallet,
            SelectorKind::Image,
        ];
        let values = [
            "example",
            "Case@example.org",
            "+12025550123",
            "Example Person",
            "example.org",
            "192.0.2.1",
            "0x0000000000000000000000000000000000000000",
            "image-example",
        ];
        let mut request = ResearchRequest {
            version: 1,
            selectors: kinds
                .into_iter()
                .zip(values)
                .map(|(kind, value)| Selector::parse(kind, value).unwrap())
                .collect(),
            sources: vec!["osint_industries_credits".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        assert!(checks(&request, 1).is_err());
        request.options.paid_authorized = true;
        let (plan, checks) = checks(&request, 1).unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(plan.selectors.len(), 8);
        assert_eq!(plan.max_calls, 1);
        assert_eq!(plan.provider_call_limits["osint_industries_credits"], 1);
        assert_eq!(
            plan.provider_credit_upper_bounds["osint_industries_credits"],
            0
        );
        assert_eq!(
            checks[0].request.url,
            "https://api.osint.industries/misc/credits"
        );
        assert_eq!(checks[0].request.method, "GET");
        assert!(checks[0].request.body.is_none());
        assert!(checks[0].request.headers.is_empty());
        assert_eq!(
            checks[0].manifest.credential_ref.as_deref(),
            Some("osint_industries")
        );
        assert_eq!(checks[0].manifest.max_response_bytes, 64 * 1024);
        assert_eq!(checks[0].manifest.cache_max_age_seconds, 0);
        assert!(plan.limitation.contains("no selected query is searched"));
        for (body, state, credits) in [
            ("{\"credits\":5}", "credits_available", Some(5)),
            ("{\"credits\":0}", "exhausted", Some(0)),
            ("{\"credits\":-1}", "unknown", None),
            ("{\"credits\":1.5}", "unknown", None),
            ("{\"credits\":\"5\"}", "unknown", None),
            ("{}", "unknown", None),
        ] {
            let (outcome, url, fields) = classify(
                "osint_industries_credits",
                &response(200, body),
                &request.selectors[0],
            );
            assert_eq!(outcome, CheckOutcome::Unknown);
            assert!(url.is_empty());
            assert_eq!(fields["usage"]["state"], state);
            assert_eq!(fields["usage"]["credits"].as_u64(), credits);
            assert_eq!(fields["usage"]["eligibility"], "unverified");
        }
        assert_eq!(
            classify(
                "osint_industries_credits",
                &response(401, "{}"),
                &request.selectors[0]
            )
            .0,
            CheckOutcome::Blocked
        );
    }
    #[test]
    fn provider_plans_bind_calls_pages_eligibility_and_premium() {
        let selector = Selector::parse(SelectorKind::Email, "Case@EXAMPLE.org").unwrap();
        let mut request = ResearchRequest {
            version: 1,
            selectors: vec![selector],
            sources: vec!["osint_industries".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        assert!(checks(&request, 1).is_err());
        request.options.paid_authorized = true;
        request.options.premium = true;
        let (plan, c) = checks(&request, 1).unwrap();
        assert_eq!(plan.provider_credit_upper_bounds["osint_industries"], 4);
        request.options.exact_match = false;
        let changed = checks(&request, 1).unwrap().0;
        assert_ne!(plan.request_digests, changed.request_digests);
        assert_ne!(
            serde_json::to_vec(&plan).unwrap(),
            serde_json::to_vec(&changed).unwrap()
        );

        assert_eq!(c[0].request.method, "POST");
        assert!(c[0].request.headers.is_empty());
        let body: serde_json::Value =
            serde_json::from_str(c[0].request.body.as_deref().unwrap()).unwrap();
        assert_eq!(body["query"], "Case@example.org");
        assert_eq!(body["timeout"], 60);
        request.selectors = vec![Selector::parse(SelectorKind::Image, "artifact-image").unwrap()];
        request.sources = vec!["eyematch".into()];
        assert!(checks(&request, 1).is_err());
        request.options.face_eligible = true;
        request.options.page = 3;
        let (plan, c) = checks(&request, 1).unwrap();
        assert_eq!(plan.max_calls, 1);
        assert_eq!(plan.provider_call_limits["eyematch"], 1);
        assert!(plan.provider_credit_upper_bounds.is_empty());
        assert!(c[0]
            .request
            .body
            .as_ref()
            .unwrap()
            .contains("image_artifact_id"));
        assert_eq!(
            classify(
                "osint_industries",
                &response(200, "{}"),
                &request.selectors[0]
            )
            .0,
            CheckOutcome::Unknown
        );
    }
    #[test]
    fn unsupported_rows_cannot_silently_disappear_from_a_mixed_plan() {
        let request = ResearchRequest {
            version: 1,
            selectors: vec![
                Selector::parse(SelectorKind::Domain, "example.org").unwrap(),
                Selector::parse(SelectorKind::Email, "Case@example.org").unwrap(),
            ],
            sources: vec!["dns".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        let error = checks(&request, 1).unwrap_err().to_string();
        assert!(error.contains("No selected automated source supports Email"));
        assert!(error.contains("Nothing was sent"));
    }
    #[test]
    fn name_discovery_and_archives_are_scoped_leads_not_account_claims() {
        let request = ResearchRequest {
            version: 1,
            selectors: vec![Selector::parse(SelectorKind::Name, "Jane Doe").unwrap()],
            sources: vec!["searxng".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        let (plan, checks) = checks(&request, 1).unwrap();
        assert_eq!(plan.max_calls, 1);
        assert_eq!(checks[0].manifest.cache_max_age_seconds, 0);
        assert!(plan.request_urls[0].contains("q=%22Jane+Doe%22"));
        let selector = &request.selectors[0];
        assert_eq!(
            classify(
                "searxng",
                &response(200, r#"{"results":[],"unresponsive_engines":[]}"#),
                selector
            )
            .0,
            CheckOutcome::NotFound
        );
        assert_eq!(
            classify(
                "searxng",
                &response(
                    200,
                    r#"{"results":[],"unresponsive_engines":[["engine","timeout"]]}"#
                ),
                selector
            )
            .0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify(
                "searxng",
                &response(200, r#"{"results":[{}],"unresponsive_engines":[]}"#),
                selector
            )
            .0,
            CheckOutcome::Unknown
        );
        assert_eq!(classify("searxng",&response(200,r#"{"results":[{"url":"https://example.org/page","title":"Synthetic lead"}],"unresponsive_engines":[]}"#),selector).0,CheckOutcome::Found);
        let domain_request = ResearchRequest {
            version: 1,
            selectors: vec![Selector::parse(
                SelectorKind::Domain,
                "https://example.org/private?token=do-not-send#fragment",
            )
            .unwrap()],
            sources: vec!["wayback".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        let domain_plan = super::checks(&domain_request, 1).unwrap().0;
        assert!(!domain_plan.request_urls[0].contains("do-not-send"));
        assert!(!domain_plan.request_urls[0].contains("private"));
        let domain = Selector::parse(SelectorKind::Domain, "example.org").unwrap();
        assert_eq!(classify("wayback",&response(200,r#"[["timestamp","original","mimetype","statuscode","digest"],["20200101000000","https://example.org/","text/html","200","hash"]]"#),&domain).0,CheckOutcome::Found);
        assert_eq!(classify("wayback",&response(200,r#"[["timestamp","original","mimetype","statuscode","digest"],["20200101000000","https://unrelated.org/","text/html","200","hash"]]"#),&domain).0,CheckOutcome::Unknown);
        assert_eq!(
            classify("wayback", &response(200, "[]"), &domain).0,
            CheckOutcome::NotFound
        );
    }
    #[test]
    fn every_native_profile_rule_has_positive_negative_and_challenge_contracts() {
        let s = Selector::parse(SelectorKind::Username, "example").unwrap();
        for source in ["github", "codeberg"] {
            assert_eq!(
                classify(source, &response(200, r#"{"login":"example","id":1}"#), &s).0,
                CheckOutcome::Found
            );
            assert_eq!(
                classify(
                    source,
                    &response(
                        404,
                        if source == "github" {
                            r#"{"message":"Not Found"}"#
                        } else {
                            r#"{"message":"The user does not exist [uid: 0]"}"#
                        }
                    ),
                    &s
                )
                .0,
                CheckOutcome::NotFound
            );
            assert_eq!(
                classify(source, &response(403, "<html>challenge</html>"), &s).0,
                CheckOutcome::Blocked
            );
            assert_eq!(
                classify(source, &response(200, "{}"), &s).0,
                CheckOutcome::Unknown
            );
        }
        assert_eq!(
            classify(
                "hackernews",
                &response(200, r#"{"id":"example","karma":1}"#),
                &s
            )
            .0,
            CheckOutcome::Found
        );
        assert_eq!(
            classify("hackernews", &response(200, "null"), &s).0,
            CheckOutcome::NotFound
        );
        assert_eq!(
            classify("hackernews", &response(429, "rate limited"), &s).0,
            CheckOutcome::Blocked
        );
        assert_eq!(
            classify("hackernews", &response(200, "{}"), &s).0,
            CheckOutcome::Unknown
        );
    }
    #[test]
    fn empty_dns_and_unrelated_domain_records_are_not_positive_findings() {
        let s = Selector::parse(SelectorKind::Domain, "example.org").unwrap();
        assert_eq!(
            classify("dns", &response(200, r#"{"Status":0,"Answer":[]}"#), &s).0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify(
                "dns",
                &response(200, r#"{"Status":0,"Answer":[{"data":"192.0.2.1"}]}"#),
                &s
            )
            .0,
            CheckOutcome::Found
        );
        assert_eq!(
            classify(
                "rdap",
                &response(200, r#"{"objectClassName":"domain","ldhName":"other.org"}"#),
                &s
            )
            .0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify(
                "rdap",
                &response(
                    200,
                    r#"{"objectClassName":"domain","ldhName":"EXAMPLE.ORG"}"#
                ),
                &s
            )
            .0,
            CheckOutcome::Found
        );
        assert_eq!(
            classify(
                "crt_sh",
                &response(200, r#"[{"name_value":"badexample.org"}]"#),
                &s
            )
            .0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify(
                "crt_sh",
                &response(200, r#"[{"name_value":"*.example.org"}]"#),
                &s
            )
            .0,
            CheckOutcome::Found
        );
    }
    #[test]
    fn malformed_email_and_ambiguous_ipv4_cannot_enter_a_plan() {
        for email in [
            "a..b@example.org",
            ".a@example.org",
            "a.@example.org",
            "a(b)@example.org",
            "a b@example.org",
        ] {
            assert!(Selector::parse(SelectorKind::Email, email).is_err());
        }
        assert_eq!(
            Selector::parse(SelectorKind::Email, "Case+Tag@EXAMPLE.org")
                .unwrap()
                .normalized,
            "Case+Tag@example.org"
        );
        for ip in [
            "127.1",
            "2130706433",
            "0x7f000001",
            "010.0.0.1",
            "192.0.2.256",
        ] {
            assert!(Selector::parse(SelectorKind::Ip, ip).is_err());
        }
        assert_eq!(
            Selector::parse(SelectorKind::Ip, "2001:0db8::1")
                .unwrap()
                .normalized,
            "2001:db8::1"
        );
    }
    #[test]
    fn normalization_preserves_identity_and_requires_country() {
        assert_eq!(
            Selector::parse(SelectorKind::Email, "Some.Name@EXAMPLE.org")
                .unwrap()
                .normalized,
            "Some.Name@example.org"
        );
        assert!(Selector::parse(SelectorKind::Phone, "555-1234").is_err());
        assert_eq!(
            Selector::parse(SelectorKind::Phone, "+1 (602) 555-0123")
                .unwrap()
                .normalized,
            "+16025550123"
        );
        assert!(Selector::parse(SelectorKind::Domain, "http://user:secret@example.org").is_err());
        assert!(Selector::parse(SelectorKind::Ip, "example.org").is_err());
        assert_eq!(
            Selector::parse(SelectorKind::Ip, "2001:db8::1")
                .unwrap()
                .normalized,
            "2001:db8::1"
        );
    }
    #[test]
    fn challenges_soft_errors_and_mismatched_names_are_never_available() {
        let s = Selector::parse(SelectorKind::Username, "example").unwrap();
        assert_eq!(
            classify("github", &response(200, "<html>challenge</html>"), &s).0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify("github", &response(200, r#"{"login":"other","id":1}"#), &s).0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify(
                "github",
                &response(429, r#"{"message":"rate limited"}"#),
                &s
            )
            .0,
            CheckOutcome::Blocked
        );
        assert_eq!(
            classify("github", &response(404, "{}"), &s).0,
            CheckOutcome::Unknown
        );
        assert_eq!(
            classify(
                "github",
                &response(
                    200,
                    r#"{"login":"example","id":1,"html_url":"https://github.com/example"}"#
                ),
                &s
            )
            .0,
            CheckOutcome::Found
        );
    }
    #[test]
    fn plans_are_closed_and_bulk_never_sends_automatically() {
        let selector = Selector::parse(SelectorKind::Username, "example").unwrap();
        let mut r = ResearchRequest {
            version: 1,
            selectors: vec![selector],
            sources: vec!["github".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        assert_eq!(checks(&r, 42).unwrap().0.max_calls, 1);
        r.selectors[0].normalized = "injected".into();
        assert!(checks(&r, 42).is_err());
        assert_eq!(
            parse_bulk("one\none\ntwo", SelectorKind::Username).unwrap()[0].repeats,
            2
        );
        assert!(!CheckOutcome::Unknown.cacheable());
        assert!(!CheckOutcome::Blocked.cacheable());
    }
}
