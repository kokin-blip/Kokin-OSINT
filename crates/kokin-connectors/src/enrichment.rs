//! Bounded display projections. These assertions never establish an identity.
use crate::research::{CheckOutcome, Selector};
use serde_json::{json, Map, Value};

const MAX_CARDS: usize = 200;
const MAX_CARD_BYTES: usize = 48 * 1024;
const MAX_STRING_BYTES: usize = 2048;

fn typed_value(value: &Value) -> Option<Value> {
    match value.get("type")?.as_str()? {
        "bool" => value.get("value")?.as_bool().map(Value::Bool),
        "int" => value.get("value")?.as_i64().map(|v| json!(v)),
        "str" => value
            .get("value")?
            .as_str()
            .filter(|s| {
                !s.is_empty() && s.len() <= MAX_STRING_BYTES && !s.chars().any(char::is_control)
            })
            .map(|s| Value::String(s.into())),
        _ => None,
    }
}

/// Official spec-format response projection. HTTP success alone is not a hit.
/// Unknown and negative modules do not establish broad account absence.
pub fn osint_industries(value: &Value, selector: &Selector) -> (CheckOutcome, String, Value) {
    let mut cards = Vec::new();
    let mut bytes = 0;
    let mut omitted = 0;
    let mut unclassified_modules = 0;
    let mut malformed_fields = 0;
    let mut accounts = 0;
    let Some(modules) = value.as_array() else {
        return (
            CheckOutcome::Unknown,
            String::new(),
            json!({"cards":[],"coverage":{"modules":0,"unclassified_modules":0,"omitted_items":0,"malformed_fields":0,"account_assertions":0,"malformed_response":true,"limited":true,"breach_coverage":"Unavailable: OSINT Industries does not supply breach data."},"interpretation":"Expected the documented module array. Inspect captured evidence; HTTP success does not establish account presence."}),
        );
    };
    for (module_index, module) in modules.iter().enumerate() {
        let name = module["module"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control));
        let matched = module["query"]
            .as_str()
            .is_some_and(|q| q == selector.normalized || q == selector.original);
        let found = module["status"].as_str() == Some("found");
        let mut classified = false;
        let Some(profiles) = module["spec_format"].as_array() else {
            unclassified_modules += 1;
            continue;
        };
        for (profile_index, profile) in profiles.iter().enumerate() {
            let Some(profile) = profile.as_object() else {
                malformed_fields += 1;
                continue;
            };
            let mut fields = Map::new();
            let mut origins = Map::new();
            let mut normalization = Map::new();
            for key in [
                "registered",
                "name",
                "first_name",
                "last_name",
                "id",
                "username",
                "email",
                "phone",
                "age",
                "verified",
                "private",
                "premium",
                "creation_date",
                "last_seen",
                "gender",
                "location",
                "address",
                "bio",
                "description",
                "url",
                "profile_url",
                "picture",
                "picture_url",
                "banner_url",
                "avatar",
                "website",
                "language",
                "email_hint",
                "phone_hint",
                "devices",
                "followers",
                "following",
            ] {
                let Some(raw) = profile.get(key) else {
                    continue;
                };
                let expected_bool =
                    matches!(key, "registered" | "verified" | "private" | "premium");
                let expected_int = matches!(key, "followers" | "following");
                if let Some(mut v) = typed_value(raw).filter(|v| {
                    if expected_bool {
                        v.is_boolean()
                    } else if expected_int {
                        v.is_i64()
                    } else if key == "id" {
                        v.is_string() || v.is_i64()
                    } else {
                        v.is_string()
                    }
                }) {
                    if key == "age" {
                        if let Some(age) = v
                            .as_str()
                            .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                            .and_then(|s| s.parse::<u8>().ok())
                            .filter(|age| *age <= 150)
                        {
                            v = json!(age);
                            normalization.insert(key.into(), json!("Decimal age string to integer, inclusive range 0–150; no age inferred from dates."));
                        }
                    }
                    fields.insert(key.into(), v);
                    origins.insert(
                        key.into(),
                        json!(format!(
                            "/{module_index}/spec_format/{profile_index}/{key}/value"
                        )),
                    );
                } else {
                    malformed_fields += 1;
                }
            }
            if let Some(variables) = profile.get("platform_variables").and_then(Value::as_array) {
                for (index, variable) in variables.iter().take(16).enumerate() {
                    let Some(key) = variable["key"].as_str().filter(|key| {
                        !key.is_empty()
                            && key.len() <= 64
                            && key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    }) else {
                        malformed_fields += 1;
                        continue;
                    };
                    if let Some(v) = typed_value(variable) {
                        let key = format!("platform.{key}");
                        // Preserve the first declared field; duplicate keys cannot silently replace its origin.
                        if !fields.contains_key(&key) {
                            fields.insert(key.clone(), v);
                            origins.insert(key, json!(format!("/{module_index}/spec_format/{profile_index}/platform_variables/{index}/value")));
                        } else {
                            omitted += 1;
                        }
                    } else {
                        malformed_fields += 1;
                    }
                }
                omitted += variables.len().saturating_sub(16);
            }
            if fields.is_empty() {
                continue;
            }
            let account = name.is_some()
                && found
                && matched
                && fields.get("registered") == Some(&Value::Bool(true));
            classified |= account;
            let card = json!({"module":name.unwrap_or("Unclassified module"),"kind":if account {"account"} else {"unclassified"},"fields":fields,"origins":origins,"normalization":normalization,"provider_assertion":true,"query_matched":matched,"status":if found {"found"} else {"unclassified"}});
            let size = serde_json::to_vec(&card).map_or(MAX_CARD_BYTES + 1, |v| v.len());
            if cards.len() >= MAX_CARDS || bytes + size > MAX_CARD_BYTES {
                omitted += 1;
                continue;
            }
            bytes += size;
            accounts += usize::from(account);
            cards.push(card);
        }
        if !classified {
            unclassified_modules += 1;
        }
    }
    (
        if accounts > 0 {
            CheckOutcome::Found
        } else {
            CheckOutcome::Unknown
        },
        String::new(),
        json!({
            "cards":cards,
            "coverage":{"modules":modules.len(),"unclassified_modules":unclassified_modules,"omitted_items":omitted,"malformed_fields":malformed_fields,"account_assertions":accounts,"breach_coverage":"Unavailable: OSINT Industries does not supply breach data. A successful lookup or registered account is not a breach membership assertion.","limited":omitted > 0 || malformed_fields > 0 || unclassified_modules > 0},
            "interpretation":"Typed provider assertions only. Separate profiles remain separate leads; registration, verified/private flags and source dates do not establish identity, freshness or KOKIN confidence. Missing/unclassified modules do not establish account absence. Source image URLs are text only and are never fetched automatically. Inspect captured evidence for omitted or unmapped fields."
        }),
    )
}

/// Reviewed native-source projections. Call only after a positive classifier result.
/// Origins address the original response JSON, not a synthesized profile document.
pub fn native_cards(source: &str, value: &Value) -> Vec<Value> {
    let allowed: &[&str] = match source {
        "github" => &[
            "login",
            "name",
            "id",
            "bio",
            "followers",
            "created_at",
            "updated_at",
            "location",
            "company",
            "blog",
            "type",
        ],
        "codeberg" => &[
            "login",
            "full_name",
            "id",
            "description",
            "followers_count",
            "created",
            "location",
            "website",
        ],
        "hackernews" => &["id", "created", "karma", "about"],
        "rdap" => &[
            "ldhName",
            "unicodeName",
            "objectClassName",
            "handle",
            "status",
        ],
        _ => return vec![],
    };
    let mut fields = Map::new();
    let mut origins = Map::new();
    let mut normalization = Map::new();
    for key in allowed {
        let Some(v) = value.get(key) else { continue };
        let bounded = v
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= MAX_STRING_BYTES)
            || v.is_number()
            || v.is_boolean()
            || (*key == "status"
                && v.as_array().is_some_and(|a| {
                    a.len() <= 16 && a.iter().all(|v| v.as_str().is_some_and(|s| s.len() <= 128))
                }));
        if bounded {
            fields.insert((*key).into(), v.clone());
            origins.insert((*key).into(), json!(format!("/{key}")));
        }
    }
    if source == "rdap" {
        if let Some(events) = value["events"].as_array() {
            for (index, event) in events.iter().enumerate().take(100) {
                let key = match event["eventAction"].as_str() {
                    Some("registration") => "creation_date",
                    Some("last changed") => "updated_at",
                    Some("expiration") => "expiration_date",
                    _ => continue,
                };
                if let Some(date) = event["eventDate"]
                    .as_str()
                    .filter(|s| s.len() <= 128 && !s.is_empty())
                {
                    if !fields.contains_key(key) {
                        fields.insert(key.into(), json!(date));
                        origins.insert(key.into(), json!(format!("/events/{index}/eventDate")));
                    }
                }
            }
        }
    }
    let created_key = match source {
        "github" => Some("created_at"),
        "codeberg" | "hackernews" => Some("created"),
        _ => None,
    };
    if let Some(key) = created_key {
        let created = if source == "hackernews" {
            value[key].as_u64().and_then(unix_utc).map(Value::String)
        } else {
            fields.get(key).filter(|v| v.is_string()).cloned()
        };
        if let Some(created) = created {
            fields.insert("creation_date".into(), created);
            origins.insert("creation_date".into(), json!(format!("/{key}")));
            normalization.insert("creation_date".into(), json!(if source == "hackernews" {
                "Unix seconds to UTC ISO8601, supported range 1970–9999; original created number is preserved."
            } else {
                "Source creation timestamp aliased to creation_date; original field is preserved."
            }));
        }
    }
    if fields.is_empty() {
        return vec![];
    }
    vec![
        json!({"module":source,"kind":if source == "rdap" {"public_record"} else {"account"},"fields":fields,"origins":origins,"normalization":normalization,"provider_assertion":false}),
    ]
}

// Proleptic Gregorian civil date from Unix days, with bounded nonnegative input.
// The era decomposition uses 400-year Gregorian cycles (146097 days).
fn unix_utc(seconds: u64) -> Option<String> {
    if seconds > 253_402_300_799 {
        return None;
    }
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds % 86_400 / 3600,
        seconds % 3600 / 60,
        seconds % 60
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::research::SelectorKind;
    fn selector() -> Selector {
        Selector::parse(SelectorKind::Email, "Case@example.org").unwrap()
    }
    fn profile(registered: Value) -> Value {
        json!({"registered":{"type":"bool","value":registered},"name":{"type":"str","value":"Example"},"verified":{"type":"bool","value":false},"private":{"type":"bool","value":true}})
    }
    fn response(profiles: Vec<Value>) -> Value {
        json!([{"module":"example","status":"found","query":"Case@example.org","spec_format":profiles}])
    }
    #[test]
    fn typed_profiles_preserve_false_and_distinct_raw_evidence_origins() {
        let raw = response(vec![profile(json!(true)), profile(json!(false))]);
        let (outcome, _, fields) = osint_industries(&raw, &selector());
        assert_eq!(outcome, CheckOutcome::Found);
        assert_eq!(fields["cards"].as_array().unwrap().len(), 2);
        assert_eq!(fields["cards"][0]["kind"], "account");
        assert_eq!(fields["cards"][1]["kind"], "unclassified");
        assert_eq!(fields["cards"][0]["fields"]["verified"], false);
        assert_eq!(
            fields["cards"][0]["origins"]["verified"],
            "/0/spec_format/0/verified/value"
        );
        assert_eq!(
            fields["cards"][1]["origins"]["name"],
            "/0/spec_format/1/name/value"
        );
        for card in fields["cards"].as_array().unwrap() {
            for (key, pointer) in card["origins"].as_object().unwrap() {
                assert_eq!(
                    raw.pointer(pointer.as_str().unwrap()).unwrap(),
                    &card["fields"][key]
                );
            }
        }
    }
    #[test]
    fn malformed_status_only_negative_and_mismatched_queries_do_not_establish_accounts() {
        for raw in [
            json!({"success":true}),
            json!([]),
            json!([{"status":"found"}]),
            response(vec![profile(json!("true"))]),
            response(vec![profile(json!(false))]),
        ] {
            assert_eq!(osint_industries(&raw, &selector()).0, CheckOutcome::Unknown);
        }
        let mut raw = response(vec![profile(json!(true))]);
        raw[0]["query"] = json!("someoneelse@example.org");
        assert_eq!(osint_industries(&raw, &selector()).0, CheckOutcome::Unknown);
        raw[0]["query"] = json!("case@example.org");
        assert_eq!(osint_industries(&raw, &selector()).0, CheckOutcome::Unknown);
        raw[0]["query"] = json!("Case@example.org");
        raw[0]["status"] = json!("error");
        assert_eq!(osint_industries(&raw, &selector()).0, CheckOutcome::Unknown);
    }
    #[test]
    fn breach_and_score_flags_never_become_breach_membership_or_confidence() {
        let mut p = profile(json!(true));
        p["breach"] = json!({"type":"bool","value":true});
        p["score"] = json!({"type":"str","value":"100"});
        let (_, _, fields) = osint_industries(&response(vec![p]), &selector());
        assert_eq!(fields["cards"][0]["kind"], "account");
        assert!(fields["cards"][0]["fields"].get("breach").is_none());
        assert!(fields["cards"][0]["fields"].get("score").is_none());
        assert!(fields["coverage"]["breach_coverage"]
            .as_str()
            .unwrap()
            .contains("does not supply breach data"));
    }
    #[test]
    fn projections_are_bounded_and_disclose_omissions() {
        let (_, _, fields) =
            osint_industries(&response(vec![profile(json!(true)); 1000]), &selector());
        assert!(fields["cards"].as_array().unwrap().len() <= MAX_CARDS);
        assert!(serde_json::to_vec(&fields).unwrap().len() < 64 * 1024);
        assert!(fields["coverage"]["omitted_items"].as_u64().unwrap() > 0);
        let mut p = profile(json!(true));
        p["name"]["value"] = json!("x".repeat(MAX_STRING_BYTES + 1));
        let (_, _, fields) = osint_industries(&response(vec![p]), &selector());
        assert!(fields["cards"][0]["fields"].get("name").is_none());
        assert_eq!(fields["coverage"]["malformed_fields"], 1);
    }
    #[test]
    fn official_integer_fields_and_numeric_age_project_without_losing_raw_origins() {
        let mut p = profile(json!(true));
        for (key, value) in [("followers", 123), ("following", 12), ("id", 456)] {
            p[key] = json!({"type":"int","value":value});
        }
        for (key, value) in [
            ("age", "31"),
            ("devices", "iPhone"),
            ("picture_url", "https://example.org/image.jpg"),
            ("banner_url", "https://example.org/banner.jpg"),
            ("first_name", "Example"),
            ("last_name", "Person"),
            ("language", "en"),
            ("email_hint", "e***@example.org"),
            ("phone_hint", "+1***123"),
            ("website", "https://example.org"),
        ] {
            p[key] = json!({"type":"str","value":value});
        }
        p["premium"] = json!({"type":"bool","value":false});
        let raw = response(vec![p]);
        let (_, _, output) = osint_industries(&raw, &selector());
        let card = &output["cards"][0];
        assert_eq!(card["fields"]["age"], 31);
        assert!(card["fields"]["age"].is_u64());
        assert!(card["normalization"]["age"]
            .as_str()
            .unwrap()
            .contains("0–150"));
        assert_eq!(
            raw.pointer(card["origins"]["age"].as_str().unwrap()),
            Some(&json!("31"))
        );
        assert_eq!(card["fields"]["followers"], 123);
        assert_eq!(card["fields"]["id"], 456);
        assert_eq!(card["fields"]["devices"], "iPhone");
        assert_eq!(card["fields"]["premium"], false);
        assert_eq!(output["coverage"]["malformed_fields"], 0);
        for age in ["unknown", "151", "-1", "31 years", "3.5"] {
            let mut p = profile(json!(true));
            p["age"] = json!({"type":"str","value":age});
            p["followers"] = json!({"type":"int","value":"123"});
            let (_, _, output) = osint_industries(&response(vec![p]), &selector());
            assert_eq!(output["cards"][0]["fields"]["age"], age);
            assert!(output["cards"][0]["fields"].get("followers").is_none());
            assert_eq!(output["coverage"]["malformed_fields"], 1);
        }
    }
    #[test]
    fn module_coverage_counts_unclassified_contracts_and_platform_omissions() {
        let mut p = profile(json!(true));
        p["platform_variables"] = json!([
            {"key":"number","type":"int","value":5},
            {"key":"number","type":"int","value":9},
            {"key":"invalid/key","type":"str","value":"bad"}
        ]);
        let mut raw = response(vec![p]);
        raw.as_array_mut().unwrap().extend([
            json!({"module":"empty","status":"found","query":"Case@example.org","spec_format":[]}),
            json!({"status":"error"}),
        ]);
        let (_, _, output) = osint_industries(&raw, &selector());
        assert_eq!(output["coverage"]["modules"], 3);
        assert_eq!(output["coverage"]["unclassified_modules"], 2);
        assert_eq!(output["coverage"]["omitted_items"], 1);
        assert_eq!(output["coverage"]["malformed_fields"], 1);
        assert_eq!(output["coverage"]["account_assertions"], 1);
        assert_eq!(output["cards"][0]["fields"]["platform.number"], 5);
        assert_eq!(
            raw.pointer(
                output["cards"][0]["origins"]["platform.number"]
                    .as_str()
                    .unwrap()
            ),
            Some(&json!(5))
        );
    }
    #[test]
    fn native_creation_dates_are_filterable_and_unix_conversion_is_bounded() {
        for (seconds, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (86_401, "1970-01-02T00:00:01Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (253_402_300_799, "9999-12-31T23:59:59Z"),
        ] {
            assert_eq!(unix_utc(seconds).as_deref(), Some(expected));
            let cards = native_cards("hackernews", &json!({"id":"example","created":seconds}));
            assert_eq!(cards[0]["fields"]["creation_date"], expected);
            assert_eq!(cards[0]["fields"]["created"], seconds);
            assert_eq!(cards[0]["origins"]["creation_date"], "/created");
        }
        assert_eq!(unix_utc(253_402_300_800), None);
        assert!(
            native_cards("hackernews", &json!({"id":"example","created":-1}))[0]["fields"]
                .get("creation_date")
                .is_none()
        );
        for (source, key) in [("github", "created_at"), ("codeberg", "created")] {
            let cards = native_cards(source, &json!({key:"2020-01-01T00:00:00Z"}));
            assert_eq!(cards[0]["fields"]["creation_date"], "2020-01-01T00:00:00Z");
            assert_eq!(cards[0]["origins"]["creation_date"], format!("/{key}"));
        }
    }
    #[test]
    fn native_record_cards_link_source_fields_and_registration_dates() {
        let raw = json!({"objectClassName":"domain","ldhName":"example.org","events":[{"eventAction":"registration","eventDate":"2000-01-01T00:00:00Z"}]});
        let cards = native_cards("rdap", &raw);
        assert_eq!(cards[0]["kind"], "public_record");
        assert_eq!(cards[0]["origins"]["creation_date"], "/events/0/eventDate");
        assert_eq!(
            cards[0]["fields"]["creation_date"],
            raw["events"][0]["eventDate"]
        );
        assert_eq!(cards[0]["provider_assertion"], false);
        assert!(native_cards("unsupported", &raw).is_empty());
    }
}
