//! The seven confidence scales of ADR-0007, and how they get into a case.
//!
//! The scales are authored as YAML in `docs/data-model/scales/`, embedded here
//! at compile time, and **copied into every case they are used in**. That last
//! part is the design decision worth defending.
//!
//! The obvious alternative is to keep the scales in the binary only and have
//! `assessment` name a scale by id. It is smaller, and it is wrong: the moment
//! a scale is revised, every assessment already made silently acquires the new
//! meaning. An analyst who recorded "similar, unverified" under a definition
//! that required no corroboration would find their judgement restated under a
//! definition that does. Copying the definitions into the case, keyed by
//! version, makes a case self-describing — it carries what its words meant.
//!
//! There is deliberately **no composite score** here. Not a field, not a
//! helper, not a weighting table. See ADR-0007.

use std::sync::OnceLock;

use rusqlite::Connection;
use serde::Deserialize;

use crate::{GraphError, Result};

/// The authored scale files, embedded so a case never depends on `docs/` being
/// present at runtime. Adding a file here without adding it to the directory
/// (or the reverse) is caught by `every_authored_scale_is_embedded`.
const EMBEDDED: &[(&str, &str)] = &[
    (
        "extraction-certainty.yaml",
        include_str!("../../../docs/data-model/scales/extraction-certainty.yaml"),
    ),
    (
        "geospatial-precision.yaml",
        include_str!("../../../docs/data-model/scales/geospatial-precision.yaml"),
    ),
    (
        "identifier-match.yaml",
        include_str!("../../../docs/data-model/scales/identifier-match.yaml"),
    ),
    (
        "relationship-confidence.yaml",
        include_str!("../../../docs/data-model/scales/relationship-confidence.yaml"),
    ),
    (
        "review-status.yaml",
        include_str!("../../../docs/data-model/scales/review-status.yaml"),
    ),
    (
        "source-reliability.yaml",
        include_str!("../../../docs/data-model/scales/source-reliability.yaml"),
    ),
    (
        "temporal-consistency.yaml",
        include_str!("../../../docs/data-model/scales/temporal-consistency.yaml"),
    ),
];

/// The value every scale must offer, and which must never be the bottom of the
/// ordering. "We have not established this" and "we have established this is
/// weak" are different findings; conflating them is how absence of evidence
/// becomes evidence of absence.
pub const INSUFFICIENT_INFORMATION: &str = "insufficient_information";

/// One scale, as authored.
///
/// The prose fields are carried rather than dropped because they are what makes
/// a scale meaningful. A scale without calibration examples is an opinion with
/// a number attached, so an export that omits them would export the number and
/// lose the meaning.
#[derive(Debug, Clone, Deserialize)]
pub struct Scale {
    pub id: String,
    pub version: i64,
    pub title: String,
    pub adr: String,
    pub status: String,
    pub question: String,
    pub values: Vec<ScaleValue>,
    pub threshold_rationale: String,
    pub known_failure_modes: Vec<String>,

    /// The file this came from, verbatim. Stored in the case.
    #[serde(skip)]
    pub source_yaml: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScaleValue {
    pub key: String,
    pub label: String,
    pub definition: String,
    pub calibration_examples: Vec<String>,

    /// Whether an automated pass may set this value. Absent means false: the
    /// safe reading of "the author did not say" is "a human decides".
    #[serde(default)]
    pub machine_assignable: bool,
}

static PARSED: OnceLock<std::result::Result<Vec<Scale>, String>> = OnceLock::new();

/// The seven scales, parsed once.
///
/// Parsing cannot fail at runtime for any input a user controls — the bytes are
/// fixed at compile time — so a failure here is a build that shipped a broken
/// scale file, and it is surfaced as an error rather than a panic because this
/// crate forbids panicking on a case-carrying path.
pub fn scales() -> Result<&'static [Scale]> {
    let parsed = PARSED.get_or_init(|| {
        let mut out = Vec::with_capacity(EMBEDDED.len());
        for (name, text) in EMBEDDED {
            let mut scale: Scale =
                yaml_serde::from_str(text).map_err(|e| format!("{name}: {e}"))?;
            scale.source_yaml = (*text).to_string();

            if !scale
                .values
                .iter()
                .any(|v| v.key == INSUFFICIENT_INFORMATION)
            {
                return Err(format!(
                    "{name}: every scale must offer '{INSUFFICIENT_INFORMATION}' (ADR-0007)"
                ));
            }
            out.push(scale);
        }
        Ok(out)
    });

    match parsed {
        Ok(scales) => Ok(scales),
        Err(why) => Err(GraphError::MalformedScale {
            reason: why.clone(),
        }),
    }
}

/// A fingerprint of the embedded scale corpus, used to decide whether a case
/// already holds this exact set. Cheaper and more honest than a version number
/// someone has to remember to bump.
fn corpus_fingerprint() -> String {
    let mut joined = String::new();
    for (name, text) in EMBEDDED {
        joined.push_str(name);
        joined.push('\0');
        joined.push_str(text);
        joined.push('\0');
    }
    kokin_store::blake3_hex(joined.as_bytes())
}

const SEEDED_KEY: &str = "scales.corpus_fingerprint";

/// Copy any scale version this case does not already hold.
///
/// Idempotent, and additive only: an existing `(id, version)` is left exactly
/// as it was found, because a case's record of what its assessments *meant* is
/// not ours to rewrite. Publishing a revised scale means bumping its `version`,
/// which lands here as a new row alongside the old one. The schema enforces
/// this too — `scale_is_immutable` refuses the update outright.
pub fn seed_scales(conn: &Connection) -> Result<usize> {
    let fingerprint = corpus_fingerprint();

    let already: Option<String> = conn
        .query_row(
            "SELECT value FROM case_meta WHERE key = ?1",
            [SEEDED_KEY],
            |r| r.get(0),
        )
        .ok();
    if already.as_deref() == Some(fingerprint.as_str()) {
        return Ok(0);
    }

    let now = kokin_store::now_utc_rfc3339();
    let mut inserted = 0usize;

    for scale in scales()? {
        // INSERT OR IGNORE, not INSERT OR REPLACE: replace would rewrite a
        // definition an existing assessment was made under.
        let changed = conn.execute(
            "INSERT OR IGNORE INTO scale (id, version, title, status, question, source_yaml, loaded_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                scale.id,
                scale.version,
                scale.title,
                scale.status,
                scale.question,
                scale.source_yaml,
                now
            ],
        )?;
        inserted += changed;

        for (ordinal, value) in scale.values.iter().enumerate() {
            conn.execute(
                "INSERT OR IGNORE INTO scale_value
                    (scale_id, scale_version, value_key, label, definition, ordinal, machine_assignable)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    scale.id,
                    scale.version,
                    value.key,
                    value.label,
                    value.definition,
                    ordinal as i64,
                    value.machine_assignable as i64
                ],
            )?;
        }
    }

    conn.execute(
        "INSERT INTO case_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![SEEDED_KEY, fingerprint],
    )?;

    Ok(inserted)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A scale file that exists but is not embedded would be documentation the
    /// product does not implement — the exact gap between a stated control and
    /// a real one that this project keeps finding in its own work.
    #[test]
    fn every_authored_scale_is_embedded() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/data-model/scales");

        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".yaml"))
            .collect();
        on_disk.sort();

        let mut embedded: Vec<String> = EMBEDDED.iter().map(|(n, _)| (*n).to_string()).collect();
        embedded.sort();

        assert_eq!(
            on_disk, embedded,
            "docs/data-model/scales and the embedded list disagree"
        );
    }

    #[test]
    fn all_seven_scales_parse() {
        let scales = scales().unwrap();
        assert_eq!(scales.len(), 7, "ADR-0007 names seven dimensions");

        for scale in scales {
            assert_eq!(scale.adr, "ADR-0007");
            assert!(!scale.values.is_empty());
            assert!(
                !scale.threshold_rationale.trim().is_empty(),
                "{}: a scale without threshold rationale is an opinion",
                scale.id
            );
            assert!(!scale.known_failure_modes.is_empty(), "{}", scale.id);
        }
    }

    /// ADR-0007 makes this value first-class in *every* scale. Tested here
    /// rather than trusted to review, because it is a one-word omission.
    #[test]
    fn insufficient_information_is_offered_everywhere_and_is_never_the_worst_value() {
        for scale in scales().unwrap() {
            let position = scale
                .values
                .iter()
                .position(|v| v.key == INSUFFICIENT_INFORMATION)
                .unwrap_or_else(|| panic!("{} does not offer it at all", scale.id));

            assert_eq!(
                position, 0,
                "{}: insufficient_information must sit outside the ordering, not at the bottom of it",
                scale.id
            );
        }
    }

    /// The scales whose thresholds have not been measured must say so, rather
    /// than quietly shipping an invented number.
    #[test]
    fn a_scale_with_unmeasured_thresholds_is_marked_partial() {
        let by_id = |id: &str| {
            scales()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| panic!("missing scale {id}"))
                .clone()
        };

        // Its similarity tiers wait on PoC P8's measured false-positive rate.
        assert_eq!(by_id("identifier_match").status, "partial");

        // And while it waits, the machine may assign only the two ends.
        let assignable: Vec<_> = by_id("identifier_match")
            .values
            .iter()
            .filter(|v| v.machine_assignable)
            .map(|v| v.key.clone())
            .collect();
        assert_eq!(
            assignable,
            vec!["same_identifier", "similar_unverified"],
            "an unmeasured tier became machine-assignable"
        );
    }

    /// Each scale states its machine ceiling twice: once in prose that a human
    /// reads, and once in a field the database enforces. They can disagree.
    ///
    /// They did. `relationship-confidence.yaml` said in prose that nothing above
    /// `co_occurrence` is machine-assignable, and marked *nothing* assignable at
    /// all — so the extractor could not record even the one thing it is entitled
    /// to observe. Nobody reading the file noticed; the first automated write
    /// did. This test is the cheap version of that discovery.
    #[test]
    fn the_machine_ceiling_matches_what_each_scale_says_in_prose() {
        let assignable = |id: &str| -> Vec<String> {
            scales()
                .unwrap()
                .iter()
                .find(|s| s.id == id)
                .unwrap_or_else(|| panic!("missing scale {id}"))
                .values
                .iter()
                .filter(|v| v.machine_assignable)
                .map(|v| v.key.clone())
                .collect()
        };

        // "Nothing above co_occurrence is machine-assignable in Phase 1. An
        // automated pass can observe that two entities appear together; it
        // cannot establish that the co-appearance means anything."
        assert_eq!(assignable("relationship_confidence"), vec!["co_occurrence"]);

        // "Until those measurements exist, the machine may assign only the two
        // ends." (identifier_match, pending PoC P8)
        assert_eq!(
            assignable("identifier_match"),
            vec!["same_identifier", "similar_unverified"]
        );

        // Geospatial precision is rated from the provenance of a coordinate,
        // which a machine can read; every tier is therefore assignable except
        // `contested`, which is a judgement that sources disagree.
        assert!(
            !assignable("geospatial_precision").contains(&"contested".to_string()),
            "a machine cannot decide that sources are in conflict"
        );
    }
}
