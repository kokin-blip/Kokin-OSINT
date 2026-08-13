//! What the interface can read, and what it is not allowed to read without.
//!
//! These run against a real case on disk — created, ingested, extracted, merged
//! and assessed — because every claim here is about the shape of an answer
//! assembled from four crates, and a mocked one would only assert that this file
//! agrees with itself.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_graph::{Grounding, NewEntity, Subject, SubjectKind};
use kokin_osint_lib::{DocumentContent, ErrorCode, SearchRequest, Session};
use serde_json::Value;

static COUNTER: AtomicU32 = AtomicU32::new(0);

const PASSPHRASE: &str = "correct horse battery staple";
const READ_PAGE: &[u8] =
    b"<html><title>Acme Holdings</title><body><p>Contact press@acme.example about QX-7741.</p></body></html>";
const UNREAD_PAGE: &[u8] = b"<html><title>Never opened</title><body>tungsten</body></html>";

fn case_dir(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("kokin-read-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// What a fixture case contains, by id.
struct Fixture {
    dir: PathBuf,
    session: Session,
    /// The artifact that was extracted, so search can find it.
    read_artifact: String,
    /// An artifact nobody ever read. Its whole job is to make coverage
    /// incomplete, which is the state every result list has to admit to.
    unread_artifact: String,
    /// The entity that survived the merge.
    canonical: String,
    /// The entity that was absorbed into it.
    absorbed: String,
}

/// Build a case with two documents, two people who turn out to be one, and a
/// disagreement about how reliable the source was.
fn fixture(name: &str) -> Fixture {
    let dir = case_dir(name);
    let session = Session::default();
    session
        .create(&dir.join("case.kokincase"), "case-read", PASSPHRASE)
        .unwrap();

    let read_file = dir.join("read.html");
    let unread_file = dir.join("unread.html");
    std::fs::write(&read_file, READ_PAGE).unwrap();
    std::fs::write(&unread_file, UNREAD_PAGE).unwrap();

    let (read_artifact, unread_artifact, canonical, absorbed) = session
        .with_case(|open| {
            let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
            let mut ingest = |path: &Path, job: &str| {
                kokin_ingest::ingest_file(
                    &mut open.conn,
                    &blobs,
                    &open.cmk,
                    path,
                    &kokin_ingest::IngestContext {
                        connector: "file_import",
                        job_id: job,
                    },
                )
                .unwrap()
                .artifact_id
            };
            let read_artifact = ingest(&read_file, "job-read");
            let unread_artifact = ingest(&unread_file, "job-unread");

            // Only one of them is extracted, so the case is honestly incomplete.
            let extracted = kokin_extract::extract_artifact(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &read_artifact,
                kokin_extract::Limits::default(),
            )
            .unwrap();
            let observation = extracted.observation_ids[0].clone();

            let evidence = [Grounding::supporting_observation(&observation)];
            let first = kokin_graph::create_entity(
                &mut open.conn,
                NewEntity {
                    type_key: "person",
                    display_name: "A. Mercer",
                    notes: "named on the contact page",
                },
                &evidence,
            )
            .unwrap();
            let second = kokin_graph::create_entity(
                &mut open.conn,
                NewEntity {
                    type_key: "person",
                    display_name: "Mercer, Alex",
                    notes: "from the filings",
                },
                &evidence,
            )
            .unwrap();

            kokin_graph::add_identifier(
                &mut open.conn,
                &first,
                "email_address",
                "press@acme.example",
                &evidence,
            )
            .unwrap();
            kokin_graph::add_identifier(&mut open.conn, &second, "username", "amercer", &evidence)
                .unwrap();

            // The two rows were judged differently before anyone decided they
            // were one person. Merging does not settle the argument.
            for (entity, value) in [(&first, "usually_reliable"), (&second, "mixed")] {
                kokin_graph::assess(
                    &mut open.conn,
                    Subject {
                        kind: SubjectKind::Entity,
                        id: entity,
                    },
                    "source_reliability",
                    value,
                    "[]",
                    "user:local",
                )
                .unwrap();
            }
            // And one dimension both agree on, so "assessed" is exercised too.
            for entity in [&first, &second] {
                kokin_graph::assess(
                    &mut open.conn,
                    Subject {
                        kind: SubjectKind::Entity,
                        id: entity,
                    },
                    "review_status",
                    "needs_review",
                    "[]",
                    "user:local",
                )
                .unwrap();
            }

            kokin_graph::resolution::merge(
                &mut open.conn,
                &first,
                &second,
                "user:local",
                "same email in the filings",
                None,
            )
            .unwrap();

            let canonical = kokin_graph::resolution::canonical_id(&open.conn, &first).unwrap();
            let absorbed = if canonical == first { second } else { first };
            Ok((read_artifact, unread_artifact, canonical, absorbed))
        })
        .unwrap();

    Fixture {
        dir,
        session,
        read_artifact,
        unread_artifact,
        canonical,
        absorbed,
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.session.close();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The ciphertext file behind an artifact, wherever the store filed it.
///
/// Found by name rather than by rebuilding the fan-out, so a change to the
/// store's directory layout does not turn these tests into a puzzle about
/// pathnames. The *name* is the coupling that matters and it is public:
/// `storage_name` is keyed with the case master key, which is why a case that
/// loses its key cannot even locate its evidence.
fn blob_file(f: &Fixture, artifact_id: &str) -> PathBuf {
    let (root, name) = f
        .session
        .with_case(|open| {
            let hash: String = open
                .conn
                .query_row(
                    "SELECT content_hash FROM artifact WHERE id = ?1",
                    [artifact_id],
                    |r| r.get(0),
                )
                .unwrap();
            Ok((
                open.paths.blobs(),
                kokin_blob::storage_name(&hash, &open.cmk),
            ))
        })
        .unwrap();

    fn find(dir: &Path, needle: &str) -> Option<PathBuf> {
        for entry in std::fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                if let Some(found) = find(&path, needle) {
                    return Some(found);
                }
            } else if path.file_stem().and_then(|s| s.to_str()) == Some(needle) {
                return Some(path);
            }
        }
        None
    }

    find(&root, &name).expect("the case has no ciphertext for that artifact")
}

fn search(f: &Fixture, query: &str) -> kokin_osint_lib::SearchView {
    f.session
        .with_case(|open| {
            kokin_osint_lib::read::search(
                open,
                &SearchRequest {
                    query: query.to_string(),
                    ..Default::default()
                },
            )
        })
        .unwrap()
}

fn document(f: &Fixture, artifact_id: &str) -> kokin_osint_lib::DocumentView {
    f.session
        .with_case(|open| kokin_osint_lib::read::artifact_document(open, artifact_id))
        .unwrap()
}

fn entity(f: &Fixture, entity_id: &str) -> kokin_osint_lib::EntityView {
    f.session
        .with_case(|open| kokin_osint_lib::read::entity(open, entity_id))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Search and its caveat
// ---------------------------------------------------------------------------

/// The one that matters: hits and coverage arrive together.
///
/// The type makes it unavoidable — `SearchView::coverage` is not an `Option` and
/// there is no command returning hits without it — so this asserts the figures
/// are real rather than that the field exists. A case with an unread document
/// must not report itself complete while handing back a confident page of
/// results.
#[test]
fn a_result_list_arrives_with_the_coverage_it_needs() {
    let f = fixture("coverage");
    let view = search(&f, "acme");

    assert!(!view.hits.is_empty(), "the extracted page was not indexed");
    assert_eq!(view.coverage.artifacts, 2);
    assert_eq!(view.coverage.complete, 1);
    assert_eq!(view.coverage.never_attempted, 1);
    assert_eq!(view.coverage.incomplete, 1);
    assert!(
        !view.coverage.is_complete,
        "a case holding a document nobody read reported itself fully searchable"
    );

    // Unread is not unavailable, and the two must not be confused. The document
    // opens perfectly; what it says is simply not in the index, which is exactly
    // the gap the caveat above exists to declare.
    assert!(matches!(
        document(&f, &f.unread_artifact).content,
        DocumentContent::Readable { .. }
    ));
}

/// A hit carries what opens it and what to show. Not a score.
///
/// Written as an exact field set: `relevance` is a BM25 figure where lower is
/// better and the scale depends on the corpus, so the only honest thing it can
/// do is order the list — which it already did, server-side, before this value
/// existed. A number in this payload is a number somebody renders as a
/// percentage.
#[test]
fn a_hit_carries_no_relevance_score() {
    let f = fixture("hitshape");
    let view = search(&f, "acme");

    let hit = serde_json::to_value(&view.hits[0]).unwrap();
    let mut fields: Vec<&str> = hit
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "canonical_id",
            "facet",
            "snippet",
            "subject_id",
            "subject_kind",
            "superseded",
            "title"
        ]
    );
}

/// A clamped page is visibly a page.
///
/// `resolve_merged` is off here so both entity rows match, which is what makes a
/// second page exist at all — and it is the specialised request precisely
/// because the merged reading is the one an analyst asked for.
#[test]
fn a_narrow_page_still_reports_the_whole_total() {
    let f = fixture("paging");
    let view = f
        .session
        .with_case(|open| {
            kokin_osint_lib::read::search(
                open,
                &SearchRequest {
                    query: "mercer".to_string(),
                    limit: Some(1),
                    resolve_merged: Some(false),
                    ..Default::default()
                },
            )
        })
        .unwrap();

    assert_eq!(view.hits.len(), 1);
    assert!(
        view.total > 1,
        "a one-result page reported a total of {}, so the interface cannot tell it is a page",
        view.total
    );
}

// ---------------------------------------------------------------------------
// Documents: five answers
// ---------------------------------------------------------------------------

#[test]
fn a_document_the_case_still_holds_reads_back_byte_for_byte() {
    let f = fixture("readable");
    let view = document(&f, &f.read_artifact);

    assert_eq!(view.media_type, "text/html");
    assert_eq!(view.byte_length, READ_PAGE.len() as i64);
    match view.content {
        DocumentContent::Readable {
            text,
            truncated_bytes,
            lossy,
        } => {
            assert_eq!(text.as_bytes(), READ_PAGE);
            assert_eq!(truncated_bytes, 0);
            assert!(!lossy);
        }
        other => panic!("a document the case holds reported as {other:?}"),
    }
}

/// Destroyed on purpose is not damage, and the provenance outlives the bytes.
#[test]
fn a_shredded_document_says_so_and_still_reports_what_it_was() {
    let f = fixture("shredded");
    f.session
        .with_case(|open| {
            open.conn
                .execute(
                    "UPDATE blob SET wrapped_key = NULL, wrapped_nonce = NULL,
                            shredded_utc = '2026-08-13T00:00:00Z'
                      WHERE content_hash = (SELECT content_hash FROM artifact WHERE id = ?1)",
                    [&f.read_artifact],
                )
                .unwrap();
            Ok(())
        })
        .unwrap();

    let view = document(&f, &f.read_artifact);
    // The record that this evidence was collected survives its destruction.
    // That is the entire point of crypto-shredding rather than deletion.
    assert_eq!(view.byte_length, READ_PAGE.len() as i64);
    assert!(!view.content_hash.is_empty());
    assert_eq!(
        view.content,
        DocumentContent::Shredded {
            shredded_utc: Some("2026-08-13T00:00:00Z".to_string())
        }
    );
}

/// The fourth state, which `blob_access` cannot see and this layer can.
///
/// The key is in the database and the ciphertext is not. Nobody decided this.
/// Reporting it as `Shredded` would file real data loss as retention policy, at
/// the exact moment somebody is looking at the document it happened to.
#[test]
fn a_document_whose_bytes_have_vanished_is_lost_not_shredded() {
    let f = fixture("lost");
    std::fs::remove_file(blob_file(&f, &f.read_artifact)).unwrap();
    assert_eq!(
        document(&f, &f.read_artifact).content,
        DocumentContent::Lost
    );
}

/// The fifth state, and the only one that is an accusation.
#[test]
fn a_modified_document_is_reported_as_damage() {
    let f = fixture("damaged");
    let blob_path = blob_file(&f, &f.read_artifact);

    let mut bytes = std::fs::read(&blob_path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&blob_path, &bytes).unwrap();

    match document(&f, &f.read_artifact).content {
        DocumentContent::Damaged { detail } => assert!(!detail.is_empty()),
        other => panic!("a modified case directory reported as {other:?}"),
    }
}

/// An excerpt describes a document known to be intact.
///
/// The tail past the excerpt limit still goes through the AEAD and the content
/// hash on its way to being discarded, so damage beyond the cut is caught. An
/// implementation that stopped reading at the limit would pass every other test
/// in this file and fail this one, which is why it is here.
#[test]
fn an_excerpt_is_verified_against_the_whole_document() {
    let f = fixture("excerpt");
    let big = f.dir.join("big.html");
    let mut html = b"<html><title>Large</title><body>".to_vec();
    html.resize(3 * 1024 * 1024, b'x');
    html.extend_from_slice(b"</body></html>");
    std::fs::write(&big, &html).unwrap();

    let artifact = f
        .session
        .with_case(|open| {
            let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
            Ok(kokin_ingest::ingest_file(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &big,
                &kokin_ingest::IngestContext {
                    connector: "file_import",
                    job_id: "job-big",
                },
            )
            .unwrap()
            .artifact_id)
        })
        .unwrap();

    match document(&f, &artifact).content {
        DocumentContent::Readable {
            text,
            truncated_bytes,
            ..
        } => {
            assert_eq!(text.len(), 2 * 1024 * 1024);
            assert_eq!(truncated_bytes as usize, html.len() - text.len());
        }
        other => panic!("a large document reported as {other:?}"),
    }

    // Now damage a byte the excerpt never shows.
    let blob_path = blob_file(&f, &artifact);
    let mut bytes = std::fs::read(&blob_path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&blob_path, &bytes).unwrap();

    assert!(
        matches!(
            document(&f, &artifact).content,
            DocumentContent::Damaged { .. }
        ),
        "damage past the excerpt limit was not detected, so the excerpt is unverified"
    );
}

// ---------------------------------------------------------------------------
// Entities, through the merge map
// ---------------------------------------------------------------------------

/// Following a merge is fine. Doing it silently is not.
#[test]
fn asking_for_a_merged_away_entity_redirects_and_says_it_did() {
    let f = fixture("redirect");
    let view = entity(&f, &f.absorbed);

    assert_eq!(view.requested_id, f.absorbed);
    assert_eq!(view.entity_id, f.canonical);
    assert!(view.redirected);
    assert!(
        view.merged_from.iter().any(|m| m.entity_id == f.absorbed),
        "the absorbed row is not named, so the analyst cannot tell where the name came from"
    );

    // And the canonical entity itself is not a redirect.
    let direct = entity(&f, &f.canonical);
    assert!(!direct.redirected);
    assert_eq!(direct.entity_id, f.canonical);
}

/// A merge does not move rows, so a read that only looked at the survivor would
/// lose the absorbed record's material — at the moment it matters most.
#[test]
fn a_merged_entity_shows_both_records_identifiers_and_evidence() {
    let f = fixture("cluster");
    let view = entity(&f, &f.canonical);

    let namespaces: Vec<&str> = view
        .identifiers
        .iter()
        .map(|i| i.namespace.as_str())
        .collect();
    assert!(
        namespaces.contains(&"email_address") && namespaces.contains(&"username"),
        "identifiers from only one side of the merge: {namespaces:?}"
    );
    assert!(
        view.identifiers.iter().any(|i| i.entity_id == f.absorbed),
        "an identifier lost the row it actually hangs on"
    );
    assert!(
        view.evidence.iter().any(|e| e.entity_id == f.absorbed),
        "the absorbed record's evidence is missing"
    );
    assert!(
        view.history.iter().any(|d| d.action == "merge"),
        "the merge is not in the entity's history"
    );
}

/// Seven dimensions, always, assessed or not.
///
/// A sparse list renders as absence and absence reads as "no concern", which is
/// the opposite of what an unassessed dimension means. ADR-0007 makes
/// `insufficient_information` first-class so "we do not know" is a statement.
#[test]
fn every_dimension_is_present_whether_or_not_anyone_assessed_it() {
    let f = fixture("dimensions");
    let view = entity(&f, &f.canonical);

    let mut names: Vec<&str> = view
        .confidence
        .iter()
        .map(|d| d.dimension.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "extraction_certainty",
            "geospatial_precision",
            "identifier_match",
            "relationship_confidence",
            "review_status",
            "source_reliability",
            "temporal_consistency"
        ]
    );

    let untouched = view
        .confidence
        .iter()
        .find(|d| d.dimension == "geospatial_precision")
        .unwrap();
    assert_eq!(untouched.agreement, "unassessed");
    assert!(untouched.values.is_empty());
    assert!(
        !untouched.question.is_empty(),
        "a dimension with no question cannot be rendered honestly"
    );
}

/// Two rows judged differently stay two judgements.
///
/// Reconciling them here would be a composite score with a smaller denominator:
/// the disagreement is the finding, and it is attributable to a row rather than
/// merely visible.
#[test]
fn rows_assessed_differently_are_reported_as_a_disagreement() {
    let f = fixture("disagreement");
    let view = entity(&f, &f.canonical);

    let reliability = view
        .confidence
        .iter()
        .find(|d| d.dimension == "source_reliability")
        .unwrap();
    assert_eq!(reliability.agreement, "disagreed");
    assert_eq!(reliability.values.len(), 2);
    let mut keys: Vec<&str> = reliability
        .values
        .iter()
        .map(|v| v.value_key.as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["mixed", "usually_reliable"]);
    assert!(
        reliability.values.iter().any(|v| v.entity_id == f.absorbed),
        "a disagreement that cannot be attributed to a row is not actionable"
    );
    assert!(reliability.values.iter().all(|v| !v.label.is_empty()));

    // Agreement is not the same as a single row having been assessed.
    let review = view
        .confidence
        .iter()
        .find(|d| d.dimension == "review_status")
        .unwrap();
    assert_eq!(review.agreement, "assessed");
    assert_eq!(review.values.len(), 2);
}

/// The composite score has one place it could be invented, and this is it.
///
/// Walks the whole serialised confidence block and refuses any number other than
/// `scale_version`. Values are keys on named ordinal scales, so there is nothing
/// to average — and this test is what keeps it that way when somebody needs to
/// sort by confidence.
#[test]
fn the_confidence_block_carries_nothing_that_could_be_added_up() {
    let f = fixture("nocomposite");
    let view = entity(&f, &f.canonical);
    let json = serde_json::to_value(&view.confidence).unwrap();

    let mut offenders = Vec::new();
    walk_numbers(&json, "confidence", &mut offenders);
    assert_eq!(
        offenders,
        vec!["confidence[].values[].scale_version"],
        "a number appeared in the confidence block that is not a scale version"
    );
}

fn walk_numbers(value: &Value, path: &str, out: &mut Vec<String>) {
    match value {
        Value::Number(_) => out.push(path.to_string()),
        Value::Array(items) => {
            for item in items {
                walk_numbers(item, &format!("{path}[]"), out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                walk_numbers(item, &format!("{path}.{key}"), out);
            }
        }
        _ => {}
    }
    out.dedup();
}

// ---------------------------------------------------------------------------
// The boundary itself
// ---------------------------------------------------------------------------

/// One file lists everything that crosses the IPC boundary.
///
/// The guarantee against leaking the case master key is that the domain crates
/// take no `serde` dependency — but that only stays *legible* if the types which
/// do serialise are somewhere a person can read in one sitting. This fails if a
/// `derive` mentioning Serialize or Deserialize appears anywhere in this crate
/// outside `views.rs` and `error.rs`, including in a module added later by
/// someone who has not read the note at the top of `views.rs`.
#[test]
fn serialize_is_derived_only_where_the_boundary_is_declared() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = ["views.rs", "error.rs"];
    let mut offenders = Vec::new();

    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if allowed.contains(&name.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("#[derive")
                && (line.contains("Serialize") || line.contains("Deserialize"))
            {
                offenders.push(format!("{name}: {line}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "types crossing the boundary must be declared in views.rs: {offenders:?}"
    );
}

/// A stale id is something the interface can act on, not a broken case.
#[test]
fn a_missing_artifact_and_a_missing_entity_are_both_not_found() {
    let f = fixture("notfound");

    let err = f
        .session
        .with_case(|open| kokin_osint_lib::read::artifact_document(open, "no-such-artifact"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    let err = f
        .session
        .with_case(|open| kokin_osint_lib::read::entity(open, "no-such-entity"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}
