//! Independently authored, bounded public-profile recipes. No third-party dataset.
use crate::research::{CheckOutcome, Selector};
use kokin_net::{HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRule {
    pub source: String,
    pub revision: u32,
    pub request_template: String,
    pub public_template: String,
    pub identity_field: String,
    pub case_sensitive: bool,
    pub require_numeric_id: bool,
    pub missing_status: u16,
    pub missing_message: Option<String>,
    pub missing_null: bool,
    pub fields: Vec<String>,
    pub max_response_bytes: usize,
    pub cache_max_age_seconds: u32,
    pub syntax: String,
    pub methodology: String,
}
pub fn rules() -> &'static [ProfileRule] {
    static RULES: OnceLock<Vec<ProfileRule>> = OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_str(include_str!("profile-rules.json")).unwrap_or_default()
    })
}
pub fn rule(source: &str) -> Option<&'static ProfileRule> {
    rules().iter().find(|r| r.source == source)
}
impl ProfileRule {
    pub fn request(&self, selector: &Selector) -> HttpRequest {
        HttpRequest::get(
            self.request_template
                .replace("{username}", &selector.normalized),
        )
        .with_header("accept", "application/json")
    }
    pub fn invalid_username(&self, username: &str) -> Option<String> {
        if self.source == "github"
            && (username.len() > 39
                || username.starts_with('-')
                || username.ends_with('-')
                || username.contains("--")
                || !username
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'))
        {
            Some("GitHub handles use at most 39 letters, numbers and single internal hyphens. This source was not contacted.".into())
        } else {
            None
        }
    }
    pub fn classify(
        &self,
        response: &HttpResponse,
        selector: &Selector,
    ) -> (CheckOutcome, String, Value) {
        use CheckOutcome::*;
        let unknown = |reason: &str| {
            (
                Unknown,
                String::new(),
                json!({"reason":reason,"recipe_revision":self.revision}),
            )
        };
        let bytes = response
            .body_bytes
            .as_deref()
            .unwrap_or(response.body.as_bytes());
        if bytes.len() > self.max_response_bytes {
            return unknown("response exceeds the reviewed recipe size limit");
        }
        // Research forbids redirects. A replay/import with a changed final URL cannot
        // satisfy a definitive rule even when its status/body looks plausible.
        if !response.redirect_chain.is_empty() {
            return unknown("redirected response does not match the authorized profile endpoint");
        }
        if matches!(response.status, 401 | 402 | 403 | 429) {
            return (Blocked, String::new(), json!({"status":response.status}));
        }
        if response.status >= 500 {
            return (Error, String::new(), json!({"status":response.status}));
        }
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return unknown(
                "expected JSON was not returned; challenge pages are not missing profiles",
            );
        };
        let missing = if self.missing_null {
            value.is_null()
        } else {
            value["message"].as_str().is_some_and(|m| {
                self.missing_message.as_deref().is_some_and(|expected| {
                    if self.source == "codeberg" {
                        m == expected || m.starts_with(&format!("{expected} ["))
                    } else {
                        m == expected
                    }
                })
            })
        };
        if response.status == self.missing_status && missing {
            return (
                NotFound,
                String::new(),
                json!({"note":"The reviewed public endpoint did not find this profile. This does not establish that the handle is available to register.","recipe_revision":self.revision}),
            );
        }
        if response.status != 200 {
            return unknown("status and missing-profile text did not meet the reviewed contract");
        }
        let Some(identity) = value[&self.identity_field].as_str() else {
            return unknown("response omitted the requested profile identity");
        };
        let matches = if self.case_sensitive {
            identity == selector.normalized
        } else {
            identity.eq_ignore_ascii_case(&selector.normalized)
        };
        if !matches || (self.require_numeric_id && !value["id"].as_u64().is_some_and(|id| id > 0)) {
            return unknown("response did not identify the requested profile");
        }
        let fields = self
            .fields
            .iter()
            .filter_map(|k| value.get(k).map(|v| (k.clone(), v.clone())))
            .collect::<serde_json::Map<_, _>>();
        (
            Found,
            self.public_template
                .replace("{username}", &selector.normalized),
            fields.into(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::research::SelectorKind;
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
    fn reviewed_rules_reject_soft_404_challenges_redirects_and_mismatched_identities() {
        let selector = Selector::parse(SelectorKind::Username, "example").unwrap();
        assert_eq!(rules().len(), 3);
        for rule in rules() {
            assert!(rule.cache_max_age_seconds <= 8 * 3600);
            assert_eq!(rule.revision, 1);
            assert_eq!(
                rule.classify(&response(200, "<html>not found</html>"), &selector)
                    .0,
                CheckOutcome::Unknown
            );
            assert_eq!(
                rule.classify(&response(404, "{}"), &selector).0,
                CheckOutcome::Unknown
            );
            assert_eq!(
                rule.classify(
                    &response(200, r#"{"login":"other","id":"other"}"#),
                    &selector
                )
                .0,
                CheckOutcome::Unknown
            );
            assert_eq!(
                rule.classify(&response(429, "rate limited"), &selector).0,
                CheckOutcome::Blocked
            );
            let body = if rule.source == "hackernews" {
                r#"{"id":"example"}"#
            } else {
                r#"{"login":"example","id":123}"#
            };
            let mut redirect = response(200, body);
            redirect
                .redirect_chain
                .push("https://other.example/login".into());
            assert_eq!(rule.classify(&redirect, &selector).0, CheckOutcome::Unknown);
            assert_eq!(
                rule.classify(&response(200, body), &selector).0,
                CheckOutcome::Found
            );
            assert_eq!(
                rule.classify(
                    &response(200, &"x".repeat(rule.max_response_bytes + 1)),
                    &selector
                )
                .0,
                CheckOutcome::Unknown
            );
        }
        let github = rule("github").unwrap();
        assert!(github.invalid_username("reserved_name").is_some());
        assert_eq!(
            github
                .classify(&response(404, r#"{"message":"Not Found"}"#), &selector)
                .0,
            CheckOutcome::NotFound
        );
        // An error page containing the same words at HTTP 200 is still inconclusive.
        assert_eq!(
            github
                .classify(&response(200, r#"{"message":"Not Found"}"#), &selector)
                .0,
            CheckOutcome::Unknown
        );
    }
}
