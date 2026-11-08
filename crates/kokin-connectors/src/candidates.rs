//! Local candidate extraction. Syntax matches require explicit analyst selection.
use crate::research::{BulkRow, Selector, SelectorKind};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Debug, Serialize, Deserialize)]
pub struct Segment {
    pub origin_id: String,
    pub text: String,
}
#[derive(Debug, Serialize)]
pub struct CandidatePreview {
    pub rows: Vec<BulkRow>,
    pub warnings: Vec<String>,
    pub segment_count: usize,
}
fn kind(token: &str) -> Option<SelectorKind> {
    use SelectorKind::*;
    if token.contains('@') {
        Some(Email)
    } else if token.starts_with("0x")
        || token.to_ascii_lowercase().starts_with("bc1")
        || ((26..=35).contains(&token.len())
            && matches!(token.as_bytes().first(), Some(b'1' | b'3' | b'T')))
    {
        Some(Wallet)
    } else if token.starts_with('+') {
        Some(Phone)
    } else if (token.contains(':') && !token.contains("://"))
        || (token.split('.').count() == 4 && token.bytes().all(|c| c.is_ascii_digit() || c == b'.'))
    {
        Some(Ip)
    } else if token.contains('.') && token.bytes().any(|c| c.is_ascii_alphabetic()) {
        Some(Domain)
    } else {
        None
    }
}
pub fn extract(segments: &[Segment]) -> Result<CandidatePreview> {
    if segments.len() > 20_000
        || segments.iter().map(|s| s.text.len()).sum::<usize>() > 2_000_000
        || segments.iter().any(|s| s.origin_id.len() > 128)
    {
        return Err(Error::Invalid(
            "candidate preview exceeds 2 MB or 20,000 text segments".into(),
        ));
    }
    let mut rows: Vec<BulkRow> = vec![];
    let mut positions = HashMap::<String, usize>::new();
    let mut truncated = false;
    let mut origins_truncated = false;
    let mut add = |raw: &str, kind: SelectorKind, origin: &str| {
        if raw.len() > 2048 || raw.is_empty() {
            return;
        }
        let parsed = Selector::parse(kind, raw);
        let key = format!(
            "{kind:?}:{}",
            parsed.as_ref().map_or(raw, |s| s.normalized.as_str())
        );
        if let Some(&i) = positions.get(&key) {
            rows[i].repeats += 1;
            if !rows[i].origin_ids.iter().any(|s| s == origin) {
                if rows[i].origin_ids.len() < 100 {
                    rows[i].origin_ids.push(origin.into());
                } else {
                    origins_truncated = true;
                }
            }
            return;
        }
        if rows.len() >= 20_000 {
            truncated = true;
            return;
        }
        positions.insert(key, rows.len());
        let (selector, error) = match parsed {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e.to_string())),
        };
        rows.push(BulkRow {
            value: raw.into(),
            kind,
            selector,
            error,
            repeats: 1,
            origin_ids: vec![origin.into()],
        });
    };
    for segment in segments {
        for raw in segment
            .text
            .split(|c: char| c.is_whitespace() || ",;<>\"(){}".contains(c))
        {
            let raw = raw.trim_matches(|c: char| ".,!?".contains(c));
            let token = raw
                .split_once(':')
                .filter(|(prefix, _)| {
                    [
                        "email", "e-mail", "phone", "tel", "ip", "domain", "website", "wallet",
                    ]
                    .contains(&prefix.to_ascii_lowercase().as_str())
                })
                .map_or(raw, |(_, value)| value);
            if let Some(k) = kind(token) {
                if k != SelectorKind::Phone {
                    add(token, k, &segment.origin_id);
                }
            }
        }
        // International phone formatting may span OCR words. Never guess a country.
        for (start, _) in segment.text.match_indices('+') {
            let phone = segment.text[start..]
                .chars()
                .take_while(|c| c.is_ascii_digit() || "+ ().-".contains(*c))
                .take(40)
                .collect::<String>();
            if phone.chars().filter(char::is_ascii_digit).count() >= 7 {
                add(phone.trim(), SelectorKind::Phone, &segment.origin_id);
            }
        }
    }
    let mut warnings=vec!["Syntax matches are candidates, not discovered accounts. No rows are selected or transmitted automatically. Names and bare usernames need explicit column mapping or manual entry. Inspect OCR and ambiguous domains against their source.".into()];
    if origins_truncated {
        warnings.push("At most 100 origin references per candidate are retained in this preview. Repeat counts include additional occurrences; inspect the source for full provenance.".into());
    }
    if truncated {
        warnings.push(
            "20,000-candidate bound reached; remaining candidates are omitted. Narrow the input."
                .into(),
        );
    }
    Ok(CandidatePreview {
        rows,
        warnings,
        segment_count: segments.len(),
    })
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn repeated_candidates_disclose_the_origin_reference_bound() {
        let segments = (0..101)
            .map(|i| Segment {
                origin_id: format!("observation-{i}"),
                text: "Case@example.org".into(),
            })
            .collect::<Vec<_>>();
        let preview = extract(&segments).unwrap();
        assert_eq!(preview.rows[0].repeats, 101);
        assert_eq!(preview.rows[0].origin_ids.len(), 100);
        assert!(preview
            .warnings
            .iter()
            .any(|s| s.contains("100 origin references")));
    }
    #[test]
    fn mixed_text_candidates_preserve_origins_duplicates_and_errors_without_guessing_names() {
        let p=extract(&[Segment{origin_id:"observation-1".into(),text:"Jane Doe <Case@EXAMPLE.org> ip:192.0.2.1 phone: +1 (602) 555-0123 wallet:0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAEd".into()},Segment{origin_id:"observation-2".into(),text:"Case@example.org https://example.org/profile 0xnotvalid".into()}]).unwrap();
        let email = p
            .rows
            .iter()
            .find(|r| r.kind == SelectorKind::Email)
            .unwrap();
        assert_eq!(email.repeats, 2);
        assert_eq!(email.origin_ids.len(), 2);
        assert_eq!(
            email.selector.as_ref().unwrap().normalized,
            "Case@example.org"
        );
        assert!(p.rows.iter().any(|r| r.kind == SelectorKind::Phone
            && r.selector
                .as_ref()
                .is_some_and(|s| s.normalized == "+16025550123")));
        assert!(p
            .rows
            .iter()
            .any(|r| r.kind == SelectorKind::Wallet && r.error.is_some()));
        assert!(!p
            .rows
            .iter()
            .any(|r| matches!(r.kind, SelectorKind::Name | SelectorKind::Username)));
        assert!(p
            .rows
            .iter()
            .any(|r| r.kind == SelectorKind::Ip && r.error.is_none()));
    }
}
