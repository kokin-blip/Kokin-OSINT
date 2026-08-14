//! What the package says about itself.
//!
//! Serialised as RFC 8785 JCS: object keys in code-point order, no insignificant
//! whitespace, integers without exponents. That matters because the manifest is
//! meant to be re-serialisable byte-for-byte by an independent implementation —
//! someone checking a package a year from now should not have to reproduce serde's
//! field order to reproduce the file.
//!
//! # The manifest is not a signature
//!
//! It carries hashes, and a hash proves only that the bytes match what *this
//! file* says they should be. Anyone who can modify a blob can recompute its
//! entry and rewrite the manifest, and nothing in the package will disagree.
//!
//! This is the same standing as the audit chain (ADR-0014): **tamper-evident
//! against accident and corruption, not tamper-proof against an author.** It
//! detects a truncated download, a flipped bit, a partial copy, and a file
//! swapped by someone who did not think to update the manifest. It does not
//! detect a deliberate, competent edit, and there is no signing key in this
//! phase to make it. Reports and UI must say so in those words.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ExportError, Result};

/// Bumped only for a change an older build cannot read.
pub const MANIFEST_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format_version: u32,
    pub case_id: String,
    pub exported_utc: String,
    /// The audit chain head at the moment of export, hex. `None` for a case with
    /// no audit events, which is distinct from a chain whose head is zero.
    pub audit_head: Option<String>,
    /// Every entry in the package, in the order written.
    pub entries: Vec<Entry>,
    /// Blobs the case holds and this package does not carry, each with the
    /// reason. Never empty without meaning: an entry here is the difference
    /// between "this package omits evidence and says so" and a case whose
    /// lineage points at bytes nobody can find.
    pub omissions: Vec<Omission>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Omission {
    /// The content hash, which survives in the database whatever happened to the
    /// bytes — so the omission can be matched to the rows that reference it.
    pub content_hash: String,
    pub reason: OmissionReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OmissionReason {
    /// The per-blob key was destroyed in the source case. The bytes were already
    /// unreadable before the export; nothing was lost in the packaging.
    Shredded,
    /// The analyst chose to leave this document out of this package. The case
    /// still holds it.
    Excluded,
    /// The case's database references it and the blob store does not have it.
    /// Not a decision anybody made — see A-032. Packaged as an omission so the
    /// recipient learns of it, rather than as an error that stops the export.
    Missing,
}

impl Manifest {
    /// Serialise as RFC 8785 JCS.
    pub fn to_jcs(&self) -> Result<String> {
        let value = serde_json::to_value(self).map_err(ExportError::Encode)?;
        let mut out = String::new();
        write_jcs(&value, &mut out)?;
        Ok(out)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|e| ExportError::Malformed(e.to_string()))?;
        if manifest.format_version != MANIFEST_FORMAT_VERSION {
            return Err(ExportError::UnsupportedPackageVersion {
                found: manifest.format_version,
                supported: MANIFEST_FORMAT_VERSION,
            });
        }
        Ok(manifest)
    }
}

/// Write `value` in RFC 8785 canonical form.
///
/// serde_json's own output is close but not equal: it preserves struct field
/// order rather than sorting, which is the one difference that matters for a
/// format whose whole purpose is that two implementations agree byte-for-byte.
fn write_jcs(value: &serde_json::Value, out: &mut String) -> Result<()> {
    use serde_json::Value;
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            // JCS defers to ECMAScript number formatting. Every number this
            // manifest holds is a u64 byte count or a version, so the integer
            // case is the only one that can arise - and a float appearing here
            // later should be a loud failure rather than a silently
            // non-canonical serialisation.
            let Some(i) = n.as_u64() else {
                return Err(ExportError::Encode(serde::ser::Error::custom(format!(
                    "{n} is not a non-negative integer; JCS number formatting \
                     for this value is not implemented"
                ))));
            };
            out.push_str(&i.to_string());
        }
        Value::String(s) => write_jcs_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_jcs(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            // Sorted by UTF-16 code unit, per RFC 8785. For the ASCII keys this
            // manifest uses that is identical to byte order; BTreeMap is used
            // rather than sort_by so the ordering is a property of the structure
            // and not of a call somebody can forget.
            let sorted: BTreeMap<_, _> = map.iter().collect();
            out.push('{');
            for (i, (key, val)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_jcs_string(key, out);
                out.push(':');
                write_jcs(val, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// RFC 8785 string escaping: the two-character forms where they exist, `\u00xx`
/// for the remaining control characters, and everything else literal.
fn write_jcs_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            format_version: MANIFEST_FORMAT_VERSION,
            case_id: "case-1".into(),
            exported_utc: "2026-08-14T00:00:00Z".into(),
            audit_head: Some("ab".repeat(32)),
            entries: vec![Entry {
                name: "case.db".into(),
                size: 4096,
                blake3: "cd".repeat(32),
            }],
            omissions: vec![],
        }
    }

    #[test]
    fn keys_are_sorted_and_nothing_is_padded() {
        let jcs = sample().to_jcs().unwrap();
        assert!(jcs.starts_with(r#"{"audit_head":"#), "{jcs}");
        assert!(!jcs.contains(' '), "insignificant whitespace: {jcs}");
        assert!(!jcs.contains('\n'));

        // Sorted, not declaration order: case_id is declared before exported_utc
        // and audit_head after both.
        let order: Vec<&str> = ["audit_head", "case_id", "entries", "exported_utc"]
            .into_iter()
            .filter(|k| jcs.contains(&format!("\"{k}\":")))
            .collect();
        let positions: Vec<usize> = order
            .iter()
            .map(|k| jcs.find(&format!("\"{k}\":")).unwrap())
            .collect();
        let mut sorted = positions.clone();
        sorted.sort_unstable();
        assert_eq!(positions, sorted, "keys are not in code-point order: {jcs}");
    }

    #[test]
    fn the_canonical_form_round_trips() {
        let manifest = sample();
        let jcs = manifest.to_jcs().unwrap();
        let back = Manifest::from_slice(jcs.as_bytes()).unwrap();
        assert_eq!(back, manifest);
        assert_eq!(back.to_jcs().unwrap(), jcs, "re-serialising changed bytes");
    }

    #[test]
    fn control_characters_are_escaped_the_way_the_spec_says() {
        let mut m = sample();
        m.case_id = "a\"b\\c\nd\u{1}e".into();
        let jcs = m.to_jcs().unwrap();
        assert!(jcs.contains(r#""case_id":"a\"b\\c\nd\u0001e""#), "{jcs}");
    }

    #[test]
    fn a_package_from_a_later_build_is_refused_by_name() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["format_version"] = serde_json::json!(2);
        let err = Manifest::from_slice(value.to_string().as_bytes()).unwrap_err();
        assert!(
            matches!(err, ExportError::UnsupportedPackageVersion { found: 2, .. }),
            "{err}"
        );
    }
}
