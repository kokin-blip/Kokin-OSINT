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
    /// The same bytes as `read_artifact`, collected from a second location. The
    /// case deduplicates the blob and keeps both arrivals, which is what makes
    /// "this document reached us twice" answerable.
    duplicate_artifact: String,
    /// One observation the extractor recorded from `read_artifact`.
    observation: String,
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
    // Byte-for-byte identical to read.html, at a different location, so the
    // case holds one blob and two collections of it.
    let duplicate_file = dir.join("mirror").join("read.html");
    std::fs::write(&read_file, READ_PAGE).unwrap();
    std::fs::write(&unread_file, UNREAD_PAGE).unwrap();
    std::fs::create_dir_all(duplicate_file.parent().unwrap()).unwrap();
    std::fs::write(&duplicate_file, READ_PAGE).unwrap();

    let (read_artifact, unread_artifact, duplicate_artifact, observation, canonical, absorbed) =
        session
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
                let duplicate_artifact = ingest(&duplicate_file, "job-mirror");

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
                kokin_graph::add_identifier(
                    &mut open.conn,
                    &second,
                    "username",
                    "amercer",
                    &evidence,
                )
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
                Ok((
                    read_artifact,
                    unread_artifact,
                    duplicate_artifact,
                    observation,
                    canonical,
                    absorbed,
                ))
            })
            .unwrap();

    Fixture {
        dir,
        session,
        read_artifact,
        unread_artifact,
        duplicate_artifact,
        observation,
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
    // Three artifacts, two of them unread — and one of those two holds the same
    // bytes as the read one. Deduplication shares a blob and does not share a
    // reading: the mirrored copy has never been through an extractor, and a
    // coverage figure that quietly counted it as read because its hash was
    // familiar would be the exact false reassurance this caveat exists to stop.
    assert_eq!(view.coverage.artifacts, 3);
    assert_eq!(view.coverage.complete, 1);
    assert_eq!(view.coverage.never_attempted, 2);
    assert_eq!(view.coverage.incomplete, 2);
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
// Evidence and lineage
// ---------------------------------------------------------------------------

fn evidence(f: &Fixture, artifact_id: &str) -> kokin_osint_lib::EvidenceListView {
    f.session
        .with_case(|open| kokin_osint_lib::read::artifact_observations(open, artifact_id, false))
        .unwrap()
}

fn observation(f: &Fixture, observation_id: &str) -> kokin_osint_lib::ObservationView {
    f.session
        .with_case(|open| kokin_osint_lib::read::observation(open, observation_id))
        .unwrap()
}

/// Write an observation whose locator does not support its value.
///
/// It has to be an insert. `observation` is append-only — the trigger says
/// *rerun the extractor, do not edit what it saw* — so a test cannot corrupt an
/// existing citation, and that is a control working rather than an obstacle.
///
/// It also could not come from the extractor: that writes the value and the
/// locator in the same breath from the same match, so a citation it produces
/// holds by construction. The check exists for rows something *else* wrote — an
/// imported package, a later extractor, a bug — and the only honest way to
/// exercise it is to be that something else.
fn forge_observation(f: &Fixture, value: &str, locator_json: &str) -> String {
    let id = format!("obs-forged-{}", value.len());
    f.session
        .with_case(|open| {
            let run_id: String = open
                .conn
                .query_row(
                    "SELECT run_id FROM observation WHERE artifact_id = ?1 LIMIT 1",
                    [&f.read_artifact],
                    |r| r.get(0),
                )
                .unwrap();
            open.conn
                .execute(
                    "INSERT INTO observation
                        (id, artifact_id, run_id, kind, value, locator_json, observed_utc)
                     VALUES (?1, ?2, ?3, 'forged', ?4, ?5, '2026-08-14T00:00:00Z')",
                    rusqlite::params![id, f.read_artifact, run_id, value, locator_json],
                )
                .unwrap();
            Ok(())
        })
        .unwrap();
    id
}

/// Every citation this case holds points at bytes that support it.
///
/// Asserted over all of them rather than a chosen one, because a spot check
/// would pass on an extractor that got the common case right and the attribute
/// case wrong — which is the shape this bug actually takes.
#[test]
fn every_observation_quotes_the_bytes_it_says_it_came_from() {
    let f = fixture("quote");
    let table = evidence(&f, &f.read_artifact);
    assert!(
        table.observations.len() >= 2,
        "the fixture page yielded only {} observations",
        table.observations.len()
    );

    for row in &table.observations {
        let view = observation(&f, &row.observation_id);
        match view.quote {
            kokin_osint_lib::QuoteView::Exact { excerpt } => {
                assert_eq!(excerpt.quoted, row.value);
            }
            kokin_osint_lib::QuoteView::Contains { excerpt } => {
                assert!(
                    excerpt.quoted.contains(&row.value),
                    "{} claims {:?} and quotes {:?}",
                    row.kind,
                    row.value,
                    excerpt.quoted
                );
            }
            other => panic!("{} cites {:?} and got {other:?}", row.kind, row.value),
        }
    }
}

/// The quote is bytes from the document, not the observation read back.
///
/// The failure this rules out is the one that looks best: a panel that renders
/// `value` twice — once as the claim and once as the "quote" — and therefore
/// agrees with itself no matter what the document says.
#[test]
fn the_quote_carries_the_document_around_it_and_not_just_the_value() {
    let f = fixture("context");
    let table = evidence(&f, &f.read_artifact);
    let page = String::from_utf8(READ_PAGE.to_vec()).unwrap();

    let mut with_context = 0;
    for row in &table.observations {
        let view = observation(&f, &row.observation_id);
        if let kokin_osint_lib::QuoteView::Exact { excerpt }
        | kokin_osint_lib::QuoteView::Contains { excerpt } = view.quote
        {
            let rebuilt = format!("{}{}{}", excerpt.before, excerpt.quoted, excerpt.after);
            assert!(
                page.contains(&rebuilt),
                "{:?} is not a substring of the document",
                rebuilt
            );
            if !excerpt.before.is_empty() || !excerpt.after.is_empty() {
                with_context += 1;
            }
        }
    }
    assert!(
        with_context > 0,
        "no observation came back with any surrounding document"
    );
}

/// A-037. A locator that points somewhere else is a disagreement, not a quote.
#[test]
fn a_citation_that_does_not_support_its_claim_says_so() {
    let f = fixture("differs");

    // Byte range 0..6 of the fixture page is `<html>`. The claim is a real
    // string from that same page, so the row is wrong only in the one way that
    // matters: the bytes it points at do not say it.
    let forged = forge_observation(
        &f,
        "press@acme.example",
        r#"{"kind":"html_byte_range","selector":"forged","start":0,"end":6}"#,
    );

    match observation(&f, &forged).quote {
        kokin_osint_lib::QuoteView::Differs { excerpt } => {
            assert_eq!(excerpt.quoted, "<html>");
        }
        other => panic!("a forged locator was accepted as {other:?}"),
    }
}

/// A locator naming bytes the document does not have.
#[test]
fn a_locator_past_the_end_of_the_document_is_out_of_range() {
    let f = fixture("range");
    let forged = forge_observation(
        &f,
        "anything at all",
        r#"{"kind":"html_byte_range","selector":"forged","start":0,"end":999999}"#,
    );

    match observation(&f, &forged).quote {
        kokin_osint_lib::QuoteView::OutOfRange { byte_length, .. } => {
            assert_eq!(byte_length, READ_PAGE.len() as i64);
        }
        other => panic!("a range past the end was accepted as {other:?}"),
    }
}

/// A scheme this build has never seen keeps its raw form and is not guessed at.
#[test]
fn an_unknown_locator_scheme_is_shown_rather_than_interpreted() {
    let f = fixture("scheme");
    let raw = r#"{"kind":"pdf_page_box","page":3,"start":0,"end":6}"#;
    let forged = forge_observation(&f, "page three", raw);

    let view = observation(&f, &forged);
    // `start` and `end` are present and this build must not use them: reading a
    // page-relative offset as a document-relative one would quote confidently
    // from the wrong place.
    match view.quote {
        kokin_osint_lib::QuoteView::UnknownScheme { scheme } => {
            assert_eq!(scheme, "pdf_page_box");
        }
        other => panic!("an unknown scheme was resolved as {other:?}"),
    }
    assert_eq!(view.locator.raw_json, raw);
    assert_eq!(view.locator.scheme, "pdf_page_box");
}

/// A quote is never shown from bytes that are not there to be checked.
#[test]
fn a_quote_from_a_shredded_document_says_which_kind_of_missing() {
    let f = fixture("quote-shredded");
    let target = evidence(&f, &f.read_artifact).observations[0]
        .observation_id
        .clone();

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

    match observation(&f, &target).quote {
        kokin_osint_lib::QuoteView::Unavailable { document_state } => {
            assert_eq!(document_state, "shredded");
        }
        other => panic!("a shredded document quoted as {other:?}"),
    }
}

/// The bytes behind a quote go through the AEAD, like every other read.
///
/// An implementation that seeked to the locator instead of streaming would
/// produce a quote from a document whose remainder had been modified, and it
/// would look exactly as convincing as a real one.
#[test]
fn a_quote_is_not_shown_from_a_document_that_failed_authentication() {
    let f = fixture("quote-damaged");
    let target = evidence(&f, &f.read_artifact).observations[0]
        .observation_id
        .clone();

    let blob_path = blob_file(&f, &f.read_artifact);
    let mut bytes = std::fs::read(&blob_path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&blob_path, &bytes).unwrap();

    match observation(&f, &target).quote {
        kokin_osint_lib::QuoteView::Unavailable { document_state } => {
            assert_eq!(document_state, "damaged");
        }
        other => panic!("a modified document quoted as {other:?}"),
    }
}

/// Lineage reaches all the way back to where the bytes came from.
#[test]
fn lineage_names_the_run_the_capture_and_the_source() {
    let f = fixture("lineage");
    let view = observation(&f, &f.observation);

    assert_eq!(view.lineage.extraction.transform_name, "extract.html");
    assert_eq!(view.lineage.extraction.status, "succeeded");
    assert_eq!(view.lineage.artifact.artifact_id, f.read_artifact);

    let collection = view
        .lineage
        .collection
        .expect("an artifact with no recorded collection");
    assert_eq!(collection.capture.connector, "file_import");
    assert_eq!(collection.capture.job_id, "job-read");
    assert!(collection.source.raw_locator.ends_with("read.html"));
    // The run that collected it, not the run that read it.
    let ingest = collection.ingest.expect("a capture with no ingest run");
    assert_ne!(ingest.run_id, view.lineage.extraction.run_id);
}

/// The same document from two places is two collections of one blob.
///
/// A panel naming only the artifact in front of the analyst would report a
/// single origin for evidence the case knows arrived twice.
#[test]
fn a_document_that_arrived_twice_names_both_arrivals() {
    let f = fixture("duplicate");
    let view = observation(&f, &f.observation);

    assert_eq!(view.lineage.also_collected.len(), 1);
    let other = &view.lineage.also_collected[0];
    assert_eq!(other.artifact_id, f.duplicate_artifact);
    assert_eq!(other.capture.job_id, "job-mirror");
    assert_ne!(
        other.source.canonical_locator,
        view.lineage
            .collection
            .as_ref()
            .unwrap()
            .source
            .canonical_locator
    );
}

/// An artifact row says what has read it, in the same words coverage uses.
#[test]
fn an_artifact_row_reports_whether_anything_has_read_it() {
    let f = fixture("artifacts");
    let rows = f
        .session
        .with_case(|open| kokin_osint_lib::read::artifacts(open, None))
        .unwrap();
    assert_eq!(rows.len(), 3);

    let by_id = |id: &str| {
        rows.iter()
            .find(|r| r.artifact_id == id)
            .unwrap_or_else(|| panic!("{id} is missing from the artifact list"))
            .clone()
    };

    let read = by_id(&f.read_artifact);
    assert_eq!(read.extraction, "complete");
    assert!(read.observations > 0);
    assert!(read.canonical_locator.is_some());

    let unread = by_id(&f.unread_artifact);
    assert_eq!(unread.extraction, "never_attempted");
    assert_eq!(unread.observations, 0);

    // Deduplicated, so the bytes are shared and the row still stands on its own.
    let duplicate = by_id(&f.duplicate_artifact);
    assert_eq!(duplicate.content_hash, read.content_hash);
    assert_eq!(duplicate.extraction, "never_attempted");
}

/// A table that hides withdrawn rows has to say how many it hid.
#[test]
fn the_evidence_table_counts_what_it_is_not_showing() {
    let f = fixture("superseded");
    let before = evidence(&f, &f.read_artifact);
    assert_eq!(before.superseded, 0);

    // Re-reading the same document withdraws the first run's observations and
    // records replacements (ADR-0006).
    f.session
        .with_case(|open| {
            let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
            kokin_extract::extract_artifact(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &f.read_artifact,
                kokin_extract::Limits::default(),
            )
            .unwrap();
            Ok(())
        })
        .unwrap();

    let after = evidence(&f, &f.read_artifact);
    assert!(
        after.superseded > 0,
        "a rerun withdrew nothing, so this asserts nothing"
    );
    assert!(
        after.observations.iter().all(|o| !o.superseded),
        "a withdrawn row was returned in a list that excludes them"
    );
    assert_eq!(after.observations.len(), before.observations.len());

    let with_withdrawn = f
        .session
        .with_case(|open| {
            kokin_osint_lib::read::artifact_observations(open, &f.read_artifact, true)
        })
        .unwrap();
    assert!(with_withdrawn.observations.iter().any(|o| o.superseded));
}

/// A withdrawal that replaced nothing is a stronger claim than a correction.
#[test]
fn an_observation_withdrawn_by_a_rerun_names_what_replaced_it() {
    let f = fixture("withdrawn");
    let original = evidence(&f, &f.read_artifact).observations[0]
        .observation_id
        .clone();

    f.session
        .with_case(|open| {
            let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
            kokin_extract::extract_artifact(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &f.read_artifact,
                kokin_extract::Limits::default(),
            )
            .unwrap();
            Ok(())
        })
        .unwrap();

    let view = observation(&f, &original);
    let superseded = view.superseded.expect("the rerun withdrew nothing");
    assert_ne!(superseded.superseded_by.as_deref(), Some(original.as_str()));
    assert!(!superseded.run_id.is_empty());
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
