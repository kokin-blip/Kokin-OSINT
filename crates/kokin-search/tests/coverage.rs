//! Coverage against the real pipeline.
//!
//! Every state asserted here is produced by really ingesting a file and really
//! running the extractor over it — including the states that come from the
//! extractor failing. A hand-written `run_subject` row would prove only that
//! this file and `coverage.rs` agree with each other, and the claim under test
//! is that a case which cannot answer a question knows that about itself.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_blob::BlobStore;
use kokin_extract::{extract_artifact, Limits, GAP_TEXT_TRUNCATED};
use kokin_ingest::{ingest_file, IngestContext};
use kokin_keys::CaseMasterKey;
use kokin_search::coverage::{coverage, gaps_for_artifact, incomplete_artifacts, CoverageState};
use kokin_search::{search, Query, SearchOptions};
use rusqlite::Connection;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// The sentence that goes missing when the text budget bites. Deliberately
/// made of words that appear nowhere else, so a search for it can only be
/// answered by prose that was actually indexed.
const BURIED: &str = "Registered in Vaduz under reference ZK-9930.";

struct Case {
    conn: Connection,
    dir: PathBuf,
    blobs: BlobStore,
    cmk: CaseMasterKey,
}

impl Case {
    fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("kokin-coverage-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let case_dir = dir.join("case.kokincase");
        let (conn, paths, _recovery) =
            kokin_store::create_case(&case_dir, "case-coverage", "passphrase").unwrap();
        let blobs = BlobStore::new(paths.blobs());
        let cmk = CaseMasterKey::generate().unwrap();

        Case {
            conn,
            dir,
            blobs,
            cmk,
        }
    }

    /// Put a file in the case without reading it.
    fn ingest(&mut self, file_name: &str, body: &[u8]) -> String {
        let path = self.dir.join(file_name);
        std::fs::write(&path, body).unwrap();
        self.ingest_path(&path)
    }

    fn ingest_path(&mut self, path: &Path) -> String {
        ingest_file(
            &mut self.conn,
            &self.blobs,
            &self.cmk,
            path,
            &IngestContext {
                connector: "file_import",
                job_id: "job-coverage",
            },
        )
        .unwrap()
        .artifact_id
    }

    fn extract(&mut self, artifact: &str, limits: Limits) -> kokin_extract::Result<()> {
        extract_artifact(&mut self.conn, &self.blobs, &self.cmk, artifact, limits).map(|_| ())
    }

    fn finds(&self, raw: &str) -> usize {
        search(&self.conn, &Query::parse(raw), &SearchOptions::default())
            .unwrap()
            .len()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A page long enough that a small text budget cuts it, with the distinctive
/// sentence at the end so truncation is what hides it.
fn long_page() -> String {
    let filler = "<p>Ordinary paragraph text that carries no distinctive terms.</p>\n".repeat(200);
    format!(
        "<html><head><title>Filings</title></head><body>\n{filler}<p>{BURIED}</p>\n</body></html>"
    )
}

#[test]
fn a_case_that_has_read_everything_says_so() {
    let mut case = Case::new("complete");
    let artifact = case.ingest(
        "page.html",
        b"<html><title>Acme</title><body><p>Hi</p></body></html>",
    );
    case.extract(&artifact, Limits::default()).unwrap();

    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.artifacts, 1);
    assert_eq!(c.complete, 1);
    assert!(c.is_complete(), "{c:?}");
    assert!(incomplete_artifacts(&case.conn, 10).unwrap().is_empty());
}

/// An empty case hides nothing, so it has nothing to caveat. The thing that
/// makes an empty case dangerous is a conclusion drawn from it, which is a
/// different control at a different point in the workflow.
#[test]
fn an_empty_case_is_complete_because_it_holds_nothing() {
    let case = Case::new("empty");
    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.artifacts, 0);
    assert!(c.is_complete());
}

#[test]
fn a_document_nobody_has_read_is_reported_as_unread() {
    let mut case = Case::new("unread");
    let read = case.ingest("read.html", b"<html><title>Acme</title></html>");
    case.extract(&read, Limits::default()).unwrap();
    let unread = case.ingest("unread.html", b"<html><title>Zeta</title></html>");

    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.artifacts, 2);
    assert_eq!(c.never_attempted, 1);
    assert_eq!(c.complete, 1);
    assert!(!c.is_complete());

    let worst = incomplete_artifacts(&case.conn, 10).unwrap();
    assert_eq!(worst.len(), 1);
    assert_eq!(worst[0].artifact_id, unread);
    assert_eq!(worst[0].state, CoverageState::NeverAttempted);
}

/// The distinction `run_subject` exists for.
///
/// A run that read a document and found nothing in it leaves no observation and
/// no derivation edge, so before this increment it was indistinguishable from a
/// document nobody had opened. Those are opposite facts: one is finished work,
/// the other is a job still to do, and a case that confuses them either invents
/// work or hides it.
#[test]
fn a_document_read_and_found_empty_is_not_a_document_nobody_read() {
    let mut case = Case::new("empty-doc");
    let read = case.ingest("bare.html", b"<html></html>");
    case.extract(&read, Limits::default()).unwrap();
    let never = case.ingest("never.html", b"<html></html>");

    // The premise: reading it really did produce nothing to hang a report on.
    let observations: i64 = case
        .conn
        .query_row(
            "SELECT COUNT(*) FROM observation WHERE artifact_id = ?1",
            [&read],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(observations, 0, "the premise of this test no longer holds");

    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.complete, 1, "a document read to the end is read: {c:?}");
    assert_eq!(c.never_attempted, 1, "{c:?}");

    let worst = incomplete_artifacts(&case.conn, 10).unwrap();
    assert_eq!(worst.len(), 1);
    assert_eq!(worst[0].artifact_id, never);
}

/// A format this build cannot read is the clearest thing a case can be asked
/// about, and until now it was recorded only as a failed run nothing could tie
/// back to the document.
#[test]
fn a_document_the_extractor_refused_is_reported_not_forgotten() {
    let mut case = Case::new("refused");
    let pdf = case.ingest("filing.pdf", b"%PDF-1.7\n%bytes\n");
    let err = case.extract(&pdf, Limits::default()).unwrap_err();
    assert!(
        matches!(
            err,
            kokin_extract::ExtractError::UnsupportedMediaType { .. }
        ),
        "{err:?}"
    );

    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.failed, 1, "{c:?}");
    assert!(!c.is_complete());

    let worst = incomplete_artifacts(&case.conn, 10).unwrap();
    assert_eq!(worst[0].artifact_id, pdf);
    assert_eq!(worst[0].media_type, "application/pdf");
    assert_eq!(
        worst[0].state,
        CoverageState::Failed {
            error_code: Some("extract.unsupported_media_type".to_string())
        },
        "the reason must survive to the report, or the analyst cannot tell \
         'we cannot read this format' from 'this file is broken'"
    );
}

/// The whole increment, in one test: the case answers a search with silence,
/// and separately admits that its silence is about itself.
#[test]
fn prose_past_the_budget_is_visible_at_the_point_of_search() {
    let mut case = Case::new("truncated");
    let artifact = case.ingest("long.html", long_page().as_bytes());
    case.extract(
        &artifact,
        Limits {
            max_text_bytes: 2048,
            ..Limits::default()
        },
    )
    .unwrap();

    // The search that quietly fails.
    assert_eq!(
        case.finds("ZK-9930"),
        0,
        "the fixture no longer buries its distinctive term past the budget"
    );

    // The part that stops it being quiet.
    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.partial, 1, "{c:?}");
    assert_eq!(c.complete, 0, "{c:?}");
    assert!(!c.is_complete());

    let gaps = gaps_for_artifact(&case.conn, &artifact).unwrap();
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(gaps[0].gap_kind, GAP_TEXT_TRUNCATED);
    assert_eq!(gaps[0].unit.as_deref(), Some("bytes"));
    assert!(
        gaps[0].magnitude.unwrap_or(0) > 0,
        "a gap with no size reads as a rounding error"
    );
    assert!(
        gaps[0].detail.contains("search"),
        "the detail must say what the gap costs, not only that one exists: {}",
        gaps[0].detail
    );
}

/// Gaps belong to a reading, not to a document. A rerun that reads the whole
/// thing has to clear them, or the case keeps apologising for a shortfall it
/// has already fixed and the report becomes noise an analyst learns to skip.
#[test]
fn a_rerun_that_reads_the_whole_document_clears_the_gap() {
    let mut case = Case::new("rerun-clears");
    let artifact = case.ingest("long.html", long_page().as_bytes());
    case.extract(
        &artifact,
        Limits {
            max_text_bytes: 2048,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(coverage(&case.conn).unwrap().partial, 1);

    case.extract(&artifact, Limits::default()).unwrap();

    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.partial, 0, "a fixed gap is still being reported: {c:?}");
    assert_eq!(c.complete, 1, "{c:?}");
    assert!(c.is_complete());
    assert!(gaps_for_artifact(&case.conn, &artifact).unwrap().is_empty());

    // And the document really is searchable now, which is the only reason to
    // believe the cleared gap rather than the report.
    assert_eq!(case.finds("ZK-9930"), 1);

    // The superseded run's gap is still on record. Append-only means the case
    // can still be asked what it used to be unable to answer.
    let historic: i64 = case
        .conn
        .query_row("SELECT COUNT(*) FROM coverage_gap", [], |r| r.get(0))
        .unwrap();
    assert_eq!(historic, 1, "the earlier reading's gap was destroyed");
}

/// Two readings that both fell short must count as one incomplete document.
/// Counting gaps instead of documents would make a case look worse every time
/// it was re-examined, which trains people to stop re-examining.
#[test]
fn a_rerun_that_still_falls_short_does_not_report_the_gap_twice() {
    let mut case = Case::new("rerun-partial");
    let artifact = case.ingest("long.html", long_page().as_bytes());
    let tight = Limits {
        max_text_bytes: 2048,
        ..Limits::default()
    };
    case.extract(&artifact, tight).unwrap();
    case.extract(
        &artifact,
        Limits {
            max_text_bytes: 3072,
            ..Limits::default()
        },
    )
    .unwrap();

    let c = coverage(&case.conn).unwrap();
    assert_eq!(c.artifacts, 1);
    assert_eq!(c.partial, 1, "{c:?}");
    assert_eq!(
        gaps_for_artifact(&case.conn, &artifact).unwrap().len(),
        1,
        "both readings' gaps are being shown as if they were separate shortfalls"
    );
    assert_eq!(incomplete_artifacts(&case.conn, 10).unwrap().len(), 1);
}

/// Two runs in the same second must still have an order.
///
/// The only test in this file with hand-written rows, because the thing under
/// test is what happens when run ids sort the *opposite* way to insertion
/// order, and real ids are random — the pipeline cannot be asked to produce
/// that case on demand. It produces it roughly half the time, which is how this
/// was found: `a_rerun_that_reads_the_whole_document_clears_the_gap` passed on
/// a coin flip.
///
/// `started_utc` is second-resolution and re-extraction runs many transforms
/// inside one second, so this is the ordinary case, not a corner. Getting it
/// wrong reports a superseded reading's verdict — a document that has been
/// re-read in full still shown as truncated, or a truncated re-read shown as
/// complete.
#[test]
fn a_rerun_in_the_same_second_still_supersedes_the_reading_before_it() {
    let mut case = Case::new("same-second");
    let artifact = case.ingest("page.html", b"<html><title>Acme</title></html>");

    // Same timestamp; the second run inserted second but sorting *first* by id.
    let same_second = "2026-08-12T00:00:00Z";
    case.conn
        .execute_batch(&format!(
            "INSERT INTO transform_run VALUES
                ('zzzz-earlier','extract.html','1','abc','p1','{same_second}','{same_second}','succeeded',NULL,NULL);
             INSERT INTO run_subject VALUES ('zzzz-earlier','artifact','{artifact}');
             INSERT INTO coverage_gap VALUES
                ('g1','zzzz-earlier','artifact','{artifact}','text_truncated','cut short',99,'bytes','{same_second}');
             INSERT INTO transform_run VALUES
                ('aaaa-later','extract.html','1','abc','p1','{same_second}','{same_second}','succeeded',NULL,NULL);
             INSERT INTO run_subject VALUES ('aaaa-later','artifact','{artifact}');"
        ))
        .unwrap();

    let c = coverage(&case.conn).unwrap();
    assert_eq!(
        c.complete, 1,
        "the later run read the whole document and reported no gap: {c:?}"
    );
    assert_eq!(
        c.partial, 0,
        "a superseded run's gap is being reported as current: {c:?}"
    );
    assert!(gaps_for_artifact(&case.conn, &artifact).unwrap().is_empty());
}

/// Unread documents come before imperfectly read ones, because that is the
/// order in which doing something about them pays off.
#[test]
fn the_worst_gaps_are_reported_first() {
    let mut case = Case::new("ordering");
    let partial = case.ingest("long.html", long_page().as_bytes());
    case.extract(
        &partial,
        Limits {
            max_text_bytes: 2048,
            ..Limits::default()
        },
    )
    .unwrap();
    let pdf = case.ingest("filing.pdf", b"%PDF-1.7\n");
    let _ = case.extract(&pdf, Limits::default());
    let never = case.ingest("never.html", b"<html><title>Zeta</title></html>");

    let worst: Vec<String> = incomplete_artifacts(&case.conn, 10)
        .unwrap()
        .into_iter()
        .map(|a| a.artifact_id)
        .collect();
    assert_eq!(worst, vec![never, pdf, partial]);
}
