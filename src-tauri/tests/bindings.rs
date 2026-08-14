//! The TypeScript side of the boundary has to describe the Rust side.
//!
//! `views.rs` is one file so a person can read the whole boundary in one
//! sitting, and `ui/src/lib/bindings.ts` is its mirror. Nothing makes them agree
//! — Tauri passes JSON and neither compiler sees the other — so a field added to
//! a Rust view and forgotten in TypeScript produces a value the interface simply
//! never reads, silently and for as long as nobody notices.
//!
//! That failure has a specific shape worth naming: it is not a crash. It is a
//! panel that renders correctly and omits something. `has_recovery_key` is the
//! example that matters — if it stopped being mirrored, the case screen would go
//! on looking right while no longer telling anyone that a forgotten passphrase
//! is permanent.
//!
//! # What this checks, and what it cannot
//!
//! It parses field names out of the `pub struct` blocks in `views.rs` and
//! requires each to appear in the TypeScript file. That catches the common drift
//! in the direction it actually happens.
//!
//! It does **not** check types, optionality, or that the TypeScript interface
//! carrying a name is the one that should. A string mirrored as a number passes
//! this and fails at runtime. Generating the bindings from the Rust types would
//! catch all of it, and is the right answer if this ever grows past one file —
//! recorded as a known limitation rather than implied to be handled.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// Every `pub struct` in a Rust source file, with its field names.
///
/// Deliberately crude: this is a lint over text, not a parser, and the moment it
/// needs to be a parser it should be a code generator instead.
fn rust_view_structs(source: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut structs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in source.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("pub struct ") {
            // Unit and tuple structs carry no named fields to mirror.
            current = rest.strip_suffix('{').map(|name| name.trim().to_string());
            if let Some(name) = &current {
                structs.entry(name.clone()).or_default();
            }
            continue;
        }
        if trimmed == "}" {
            current = None;
            continue;
        }
        let Some(name) = &current else { continue };

        if let Some(rest) = trimmed.strip_prefix("pub ") {
            if let Some((field, _)) = rest.split_once(':') {
                let field = field.trim();
                if !field.is_empty() && field.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    structs
                        .entry(name.clone())
                        .or_default()
                        .insert(field.to_string());
                }
            }
        }
    }

    structs
}

/// View types the interface reads today.
const BOUND: &[&str] = &["CaseView", "NewCaseView", "SessionView"];

/// View types with no TypeScript yet, and the increment that adds each.
///
/// An explicit list rather than a filter, so that **a new view type is a
/// failure until somebody classifies it**. The alternative — checking only what
/// happens to be bound — silently accepts every type nobody got round to, which
/// is the state this list exists to make visible. Each later increment deletes
/// lines from it.
const NOT_YET_BOUND: &[(&str, &str)] = &[
    ("SearchRequest", "increment 24: search"),
    ("HitView", "increment 24: search"),
    ("CoverageView", "increment 24: search"),
    ("SearchView", "increment 24: search"),
    ("DocumentView", "increment 23: evidence and lineage"),
    ("EntityView", "increment 23: entities"),
    ("EntityRefView", "increment 23: entities"),
    ("IdentifierView", "increment 23: entities"),
    ("EvidenceView", "increment 23: evidence and lineage"),
    ("DimensionView", "increment 23: confidence"),
    ("DimensionValueView", "increment 23: confidence"),
    ("DecisionView", "increment 23: resolution"),
    ("IngestFileRequest", "increment 25: write flows"),
    ("IngestView", "increment 25: write flows"),
    ("ExtractRequest", "increment 25: write flows"),
    ("ExtractView", "increment 25: write flows"),
    ("GroundingInput", "increment 25: write flows"),
    ("NewEntityRequest", "increment 25: write flows"),
    ("NewIdentifierRequest", "increment 25: write flows"),
    ("NewRelationshipRequest", "increment 25: write flows"),
    ("AssessRequest", "increment 25: write flows"),
    ("MergeRequest", "increment 25: write flows"),
    ("RejectRequest", "increment 25: write flows"),
    ("SplitRequest", "increment 25: write flows"),
    ("WriteView", "increment 25: write flows"),
    ("ResolutionView", "increment 25: write flows"),
];

#[test]
fn every_view_type_is_either_bound_or_knowingly_not() {
    let views = std::fs::read_to_string(repo_file("src/views.rs")).unwrap();
    let structs = rust_view_structs(&views);

    assert!(
        structs.len() > 20,
        "the scraper found only {} structs; it no longer matches views.rs",
        structs.len()
    );

    let deferred: BTreeSet<&str> = NOT_YET_BOUND.iter().map(|(name, _)| *name).collect();
    let bound: BTreeSet<&str> = BOUND.iter().copied().collect();

    let unclassified: Vec<&String> = structs
        .keys()
        .filter(|name| !bound.contains(name.as_str()) && !deferred.contains(name.as_str()))
        .collect();
    assert!(
        unclassified.is_empty(),
        "neither bound nor listed as deferred; classify them: {unclassified:?}"
    );

    let stale: Vec<&str> = bound
        .iter()
        .chain(deferred.iter())
        .filter(|name| !structs.contains_key(**name))
        .copied()
        .collect();
    assert!(
        stale.is_empty(),
        "these names are listed here and no longer exist in views.rs: {stale:?}"
    );
}

#[test]
fn the_typescript_bindings_cover_every_bound_view_field() {
    let views = std::fs::read_to_string(repo_file("src/views.rs")).unwrap();
    let bindings = std::fs::read_to_string(repo_file("../ui/src/lib/bindings.ts")).unwrap();
    let structs = rust_view_structs(&views);

    let mut missing = Vec::new();
    for name in BOUND {
        let fields = structs
            .get(*name)
            .unwrap_or_else(|| panic!("{name} is listed as bound and is not in views.rs"));
        for field in fields {
            if !bindings.contains(field) {
                missing.push(format!("{name}.{field}"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "bound view fields missing from ui/src/lib/bindings.ts: {missing:?}"
    );
}

/// Every `ErrorCode` variant must have a presentation, or a failure renders as
/// `undefined`.
///
/// `ErrorNotice.svelte` holds a `Record<ErrorCode, Presentation>`, so TypeScript
/// already refuses a *missing* key — but only against the union in
/// `bindings.ts`, and that union is hand-written. This checks the union itself
/// against the Rust enum, which is the link no compiler covers.
#[test]
fn every_error_code_reaches_the_interface() {
    let error_rs = std::fs::read_to_string(repo_file("src/error.rs")).unwrap();
    let bindings = std::fs::read_to_string(repo_file("../ui/src/lib/bindings.ts")).unwrap();

    // Variants sit between `pub enum ErrorCode {` and its closing brace, one per
    // line, bare and capitalised.
    let body = error_rs
        .split_once("pub enum ErrorCode {")
        .expect("ErrorCode enum not found")
        .1;
    let body = body.split_once("\n}").expect("unterminated enum").0;

    let mut variants = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("#[")
            || !trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
            || !trimmed.chars().all(|c| c.is_ascii_alphanumeric())
        {
            continue;
        }
        variants.push(trimmed.to_string());
    }

    assert!(
        variants.len() >= 15,
        "only {} ErrorCode variants were parsed; the scraper has drifted",
        variants.len()
    );

    // `#[serde(rename_all = "snake_case")]` is what the wire actually carries.
    let missing: Vec<String> = variants
        .iter()
        .map(|v| snake_case(v))
        .filter(|snake| !bindings.contains(&format!("\"{snake}\"")))
        .collect();

    assert!(
        missing.is_empty(),
        "these error codes are not in the TypeScript union, so a failure carrying \
         one has no presentation and renders as undefined: {missing:?}"
    );
}

fn snake_case(variant: &str) -> String {
    let mut out = String::new();
    for (i, c) in variant.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `invoke` is called in exactly one file.
///
/// The same rule as `serialize_is_derived_only_where_the_boundary_is_declared`
/// on the Rust side, pointed the other way. A component calling `invoke`
/// directly would bypass the typed wrappers and, more importantly, would put a
/// second place to look when asking what this application can ask of its
/// backend.
#[test]
fn only_the_bindings_module_calls_invoke() {
    let ui = repo_file("../ui/src");
    let mut offenders = Vec::new();
    let mut stack = vec![ui];

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name == "bindings.ts" {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext, "ts" | "svelte") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            if text.contains("@tauri-apps/api/core") || text.contains("invoke(") {
                offenders.push(name);
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "every call into Rust goes through ui/src/lib/bindings.ts: {offenders:?}"
    );
}
