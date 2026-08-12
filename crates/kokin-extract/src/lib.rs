//! Extraction: turning a stored artifact into observations with locators.
//!
//! This is reference-workflow step 4, and the first crate that reads evidence
//! back out rather than writing it in. It composes and holds no policy of its
//! own: decryption and verification belong to `kokin-blob`, the append-only
//! rules to `kokin-store`'s triggers, and the parsing bounds to
//! [`html::Limits`].
//!
//! # The order matters
//!
//! ```text
//! transform_run (running)
//!   -> read artifact -> blob, streamed and verified
//!   -> parse                 bounded memory, hostile input assumed
//!   -> observation           one row per extracted value, each with a locator
//!   -> derivation            artifact -> observation, via this run
//!   -> observation_supersession   for anything an earlier run of this
//!                                 extractor said about the same artifact
//! transform_run (succeeded | failed)
//! ```
//!
//! # Why re-extraction supersedes rather than replaces
//!
//! An artifact is immutable, so running a newer extractor over it is the
//! normal way this crate is used: the bytes did not change, our reading of them
//! did. ADR-0006 requires the old reading to survive that, because a conclusion
//! an analyst already accepted rests on it. Old observations are therefore
//! never edited or deleted — they gain a supersession row pointing at whatever
//! replaced them, or at nothing if the newer extractor withdrew the claim.

use kokin_blob::BlobStore;
use kokin_keys::CaseMasterKey;
use rusqlite::Connection;
use std::collections::HashMap;

pub mod html;

pub use html::{Extraction, HtmlExtractor, Limits, Locator, RawObservation};

/// Identifies the code that produced a row, so a rerun after an upgrade is
/// distinguishable from the original (ADR-0006).
pub const TRANSFORM_HTML: &str = "extract.html";
pub const TRANSFORM_VERSION: &str = "1";

/// Media types this extractor will read.
///
/// Checked rather than assumed: running an HTML parser over a PDF produces
/// confident nonsense, and an artifact's media type is already recorded.
const HTML_MEDIA_TYPES: &[&str] = &["text/html", "application/xhtml+xml"];

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("store error: {0}")]
    Store(#[from] kokin_store::StoreError),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("blob error: {0}")]
    Blob(#[from] kokin_blob::BlobError),

    #[error("keys error: {0}")]
    Keys(#[from] kokin_keys::KeyError),

    #[error("random number generation failed: {0}")]
    Random(String),

    #[error("artifact {id} is not in this case")]
    ArtifactMissing { id: String },

    #[error("artifact {id} has media type {media_type}, which this extractor does not read")]
    UnsupportedMediaType { id: String, media_type: String },

    #[error("blob {content_hash} has no row in this case")]
    BlobMissing { content_hash: String },

    #[error("blob {content_hash} has been shredded and cannot be read")]
    BlobShredded { content_hash: String },

    #[error("the document is malformed beyond parsing: {reason}")]
    Malformed { reason: String },

    #[error("the document would produce more than {limit} observations")]
    TooManyObservations { limit: usize },

    #[error("parsing needed more than {limit} bytes of buffer")]
    ParserMemory { limit: usize },

    #[error("the parser panicked: {reason}")]
    ParserPanicked { reason: String },
}

pub type Result<T> = std::result::Result<T, ExtractError>;

/// What one extraction produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub run_id: String,
    pub artifact_id: String,
    pub observation_ids: Vec<String>,
    /// Values too long to record. See [`Extraction::skipped_oversize`].
    pub skipped_oversize: usize,
    /// Observations from an earlier run of this extractor that this run
    /// superseded.
    pub superseded: usize,
}

/// Read an artifact and record what it says.
///
/// The artifact must already be in the case: this crate never fetches, and
/// takes no capability that would let it.
pub fn extract_artifact(
    conn: &mut Connection,
    blobs: &BlobStore,
    cmk: &CaseMasterKey,
    artifact_id: &str,
    limits: Limits,
) -> Result<Extracted> {
    let params = params_hash(&limits);

    with_run(conn, TRANSFORM_HTML, &params, |conn, run_id| {
        let blob = load_readable_blob(conn, artifact_id)?;
        let extraction = parse_blob(blobs, cmk, &blob, limits)?;
        record(conn, artifact_id, run_id, &extraction)
    })
}

/// The artifact's bytes, in a form the blob store will hand back.
struct ReadableBlob {
    reference: kokin_blob::BlobRef,
}

/// A `blob` row's size and key material. Both key columns are NULL once the
/// blob has been crypto-shredded, which is the case this tuple exists to make
/// visible rather than to paper over.
type BlobKeyRow = (i64, Option<Vec<u8>>, Option<Vec<u8>>);

fn load_readable_blob(conn: &Connection, artifact_id: &str) -> Result<ReadableBlob> {
    let (content_hash, media_type): (String, String) = conn
        .query_row(
            "SELECT content_hash, media_type FROM artifact WHERE id = ?1",
            [artifact_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ExtractError::ArtifactMissing {
                id: artifact_id.to_string(),
            },
            other => ExtractError::Sqlite(other),
        })?;

    // Parameters were already stripped when the media type was recorded, so an
    // equality check is right here and a prefix match would be looser than
    // intended.
    if !HTML_MEDIA_TYPES.contains(&media_type.as_str()) {
        return Err(ExtractError::UnsupportedMediaType {
            id: artifact_id.to_string(),
            media_type,
        });
    }

    // `.ok()` here would fold a genuine database failure into "no such blob",
    // which would report a broken case as a missing one and send whoever reads
    // the error code looking in the wrong place.
    let row: BlobKeyRow = match conn.query_row(
        "SELECT size_bytes, wrapped_key, wrapped_nonce FROM blob WHERE content_hash = ?1",
        [&content_hash],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(ExtractError::BlobMissing { content_hash })
        }
        Err(other) => return Err(ExtractError::Sqlite(other)),
    };

    let (size, wrapped_key, wrapped_nonce) = row;

    // A shredded blob is a row whose key is gone. That is a different fact from
    // the blob never having existed, and an analyst reading the error needs to
    // be able to tell them apart.
    let (Some(ciphertext), Some(nonce)) = (wrapped_key, wrapped_nonce) else {
        return Err(ExtractError::BlobShredded { content_hash });
    };

    let nonce: [u8; kokin_keys::NONCE_LEN] =
        nonce
            .as_slice()
            .try_into()
            .map_err(|_| ExtractError::BlobShredded {
                content_hash: content_hash.clone(),
            })?;

    Ok(ReadableBlob {
        reference: kokin_blob::BlobRef {
            content_hash,
            size: size as u64,
            wrapped_key: kokin_keys::WrappedKey { nonce, ciphertext },
        },
    })
}

/// Stream the blob through the parser.
///
/// `read_to` decrypts and verifies chunk by chunk and pushes into the sink, so
/// the document is never held in memory in full — which is the whole reason the
/// extractor is push-based.
fn parse_blob(
    blobs: &BlobStore,
    cmk: &CaseMasterKey,
    blob: &ReadableBlob,
    limits: Limits,
) -> Result<Extraction> {
    catching_panics(|| {
        let mut sink = ExtractSink {
            extractor: HtmlExtractor::new(limits),
            failure: None,
        };

        match blobs.read_to(&blob.reference, cmk, &mut sink) {
            Ok(()) => {}
            Err(e) => {
                // A parse failure surfaces as an I/O error from the sink, so
                // the real cause has to be recovered before it is reported as
                // the blob store having failed.
                return Err(sink.failure.unwrap_or(ExtractError::Blob(e)));
            }
        }

        sink.extractor.finish()
    })
}

/// Turn a panic in the parser into an error code instead of a lost case.
///
/// Required by the plan: a malformed document must not be able to take the
/// application down with it. The parser is the one place in this crate reached
/// directly by hostile bytes, and it is third-party code, so "it does not
/// panic" is an assumption rather than something this crate can guarantee.
///
/// A panic gets its own code rather than being folded into
/// [`ExtractError::Malformed`], because the two mean different things: a
/// malformed document is a fact about the evidence, and a panic is a bug in
/// us. Filing the second as the first would quietly convert our defects into
/// observations about someone's website.
///
/// `AssertUnwindSafe` is sound here because everything the closure touches is
/// created inside it and dropped on the way out: no caller-visible state can be
/// observed in a torn condition, and the run is already being marked failed.
fn catching_panics<F>(work: F) -> Result<Extraction>
where
    F: FnOnce() -> Result<Extraction>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(result) => result,
        Err(payload) => Err(ExtractError::ParserPanicked {
            reason: describe_panic(payload.as_ref()),
        }),
    }
}

fn describe_panic(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panicked with a non-string payload".to_string()
    }
}

struct ExtractSink {
    extractor: HtmlExtractor,
    failure: Option<ExtractError>,
}

impl std::io::Write for ExtractSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Err(e) = self.extractor.write(buf) {
            self.failure = Some(e);
            return Err(std::io::Error::other("extraction stopped"));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Write the observations, their lineage, and any supersessions.
///
/// One transaction: a partial extraction is worse than none, because it looks
/// like a complete reading of the page.
fn record(
    conn: &mut Connection,
    artifact_id: &str,
    run_id: &str,
    extraction: &Extraction,
) -> Result<Extracted> {
    let now = now_utc();
    let tx = conn.transaction()?;

    let mut observation_ids = Vec::with_capacity(extraction.observations.len());
    // Keyed by what identifies "the same reading of the same place": the kind
    // of claim and where in the artifact it was made.
    let mut by_position: HashMap<(String, String), String> = HashMap::new();

    for observation in &extraction.observations {
        let id = new_id()?;
        let locator_json = observation.locator.to_json();

        tx.execute(
            "INSERT INTO observation
                (id, artifact_id, run_id, kind, value, locator_json, observed_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                artifact_id,
                run_id,
                observation.kind,
                observation.value,
                locator_json,
                now,
            ],
        )?;

        tx.execute(
            "INSERT INTO derivation (id, run_id, input_kind, input_id, output_kind, output_id)
             VALUES (?1, ?2, 'artifact', ?3, 'observation', ?4)",
            rusqlite::params![new_id()?, run_id, artifact_id, id],
        )?;

        by_position.insert((observation.kind.clone(), locator_json), id.clone());
        observation_ids.push(id);
    }

    let superseded = supersede_earlier_runs(&tx, artifact_id, run_id, &by_position, &now)?;

    kokin_store::audit::append(
        &tx,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action: "evidence.extracted",
            subject_kind: Some("artifact"),
            subject_id: Some(artifact_id),
            payload_json: &format!(
                r#"{{"observations":{},"skipped_oversize":{},"superseded":{}}}"#,
                observation_ids.len(),
                extraction.skipped_oversize,
                superseded
            ),
        },
    )?;

    tx.commit()?;

    Ok(Extracted {
        run_id: run_id.to_string(),
        artifact_id: artifact_id.to_string(),
        observation_ids,
        skipped_oversize: extraction.skipped_oversize,
        superseded,
    })
}

/// Mark what an earlier run of this extractor said about this artifact.
///
/// Matching is by (kind, locator): the artifact is immutable, so the same kind
/// of claim about the same bytes is the same reading, and a changed value there
/// is a genuine revision rather than a new finding. An old observation with no
/// counterpart is superseded by nothing, which records that the newer extractor
/// no longer makes that claim.
fn supersede_earlier_runs(
    tx: &rusqlite::Transaction<'_>,
    artifact_id: &str,
    run_id: &str,
    by_position: &HashMap<(String, String), String>,
    now: &str,
) -> Result<usize> {
    let mut stmt = tx.prepare(
        "SELECT o.id, o.kind, o.locator_json
           FROM observation o
           JOIN transform_run r ON r.id = o.run_id
          WHERE o.artifact_id = ?1
            AND r.transform_name = ?2
            AND o.run_id <> ?3
            AND o.id NOT IN (SELECT superseded_id FROM observation_supersession)",
    )?;

    let earlier: Vec<(String, String, String)> = stmt
        .query_map(
            rusqlite::params![artifact_id, TRANSFORM_HTML, run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);

    for (old_id, kind, locator_json) in &earlier {
        let replacement = by_position.get(&(kind.clone(), locator_json.clone()));
        tx.execute(
            "INSERT INTO observation_supersession
                (superseded_id, superseded_by, run_id, recorded_utc)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![old_id, replacement, run_id, now],
        )?;
    }

    Ok(earlier.len())
}

/// Open a run, do the work, and close the run either way.
///
/// Identical in shape to `kokin-ingest`'s, and for the same reason: a pipeline
/// that records only its successes cannot be audited.
fn with_run<F>(
    conn: &mut Connection,
    transform_name: &str,
    params_hash: &str,
    work: F,
) -> Result<Extracted>
where
    F: FnOnce(&mut Connection, &str) -> Result<Extracted>,
{
    let run_id = new_id()?;

    conn.execute(
        "INSERT INTO transform_run
            (id, transform_name, transform_version, code_version, params_hash,
             started_utc, finished_utc, status, error_code, error_detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, 'running', NULL, NULL)",
        rusqlite::params![
            run_id,
            transform_name,
            TRANSFORM_VERSION,
            code_version(),
            params_hash,
            now_utc(),
        ],
    )?;

    match work(conn, &run_id) {
        Ok(extracted) => {
            complete_run(conn, &run_id, "succeeded", None, None)?;
            Ok(extracted)
        }
        Err(e) => {
            let (code, detail) = classify(&e);
            let _ = complete_run(conn, &run_id, "failed", Some(code), Some(&detail));
            Err(e)
        }
    }
}

fn complete_run(
    conn: &Connection,
    run_id: &str,
    status: &str,
    error_code: Option<&str>,
    error_detail: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE transform_run
            SET status = ?2, finished_utc = ?3, error_code = ?4, error_detail = ?5
          WHERE id = ?1",
        rusqlite::params![run_id, status, now_utc(), error_code, error_detail],
    )?;
    Ok(())
}

/// Map an error to a stable code, because these are queried and counted.
fn classify(e: &ExtractError) -> (&'static str, String) {
    let code = match e {
        ExtractError::ArtifactMissing { .. } => "artifact.missing",
        ExtractError::UnsupportedMediaType { .. } => "extract.unsupported_media_type",
        ExtractError::BlobMissing { .. } => "blob.missing",
        ExtractError::BlobShredded { .. } => "blob.shredded",
        ExtractError::Malformed { .. } => "extract.malformed",
        ExtractError::TooManyObservations { .. } => "extract.too_many_observations",
        ExtractError::ParserMemory { .. } => "extract.parser_memory",
        ExtractError::ParserPanicked { .. } => "extract.panicked",
        ExtractError::Blob(_) => "blob.failed",
        ExtractError::Keys(_) => "keys.failed",
        ExtractError::Store(_) | ExtractError::Sqlite(_) => "store.failed",
        ExtractError::Random(_) => "random.failed",
    };
    (code, e.to_string())
}

fn code_version() -> String {
    format!("kokin-extract {}", env!("CARGO_PKG_VERSION"))
}

/// Hashes the limits, not the input.
///
/// `params_hash` answers "was this run configured the same way?", which is what
/// makes two runs over the same artifact comparable. The input is already
/// identified by the `derivation` edge.
fn params_hash(limits: &Limits) -> String {
    kokin_store::blake3_hex(
        format!(
            "max_observations={};max_value_bytes={};max_parser_memory_bytes={}",
            limits.max_observations, limits.max_value_bytes, limits.max_parser_memory_bytes
        )
        .as_bytes(),
    )
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| ExtractError::Random(e.to_string()))?;
    Ok(kokin_store::blake3_hex(&bytes)[..32].to_string())
}

fn now_utc() -> String {
    kokin_store::now_utc_rfc3339()
}

#[cfg(test)]
// Tests are the one place a panic is the correct response to an unexpected
// value: it is how the failure gets reported.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use kokin_ingest::{ingest_file, IngestContext};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    const PAGE: &str = concat!(
        "<html><head><title>Acme Holdings</title>\n",
        "<meta property=\"og:title\" content=\"Acme Holdings Ltd\"></head>\n",
        "<body><a href=\"https://acme.example/about\">About</a>\n",
        "<a href=\"mailto:press@acme.example\">Press</a></body></html>"
    );

    fn temp_dir(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("kokin-extract-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    /// A case with a real artifact in it.
    ///
    /// Built by running the actual ingest pipeline rather than by inserting
    /// rows by hand: the claim being tested is that increments 6 and 7 compose,
    /// and hand-written rows would only prove that extraction works against
    /// whatever shape this test imagined.
    struct Harness {
        conn: Connection,
        blobs: BlobStore,
        cmk: CaseMasterKey,
        dir: PathBuf,
    }

    impl Harness {
        fn new(name: &str) -> Self {
            let dir = temp_dir(name);
            let case_dir = dir.join("case.kokincase");
            let (conn, paths, _recovery) =
                kokin_store::create_case(&case_dir, "case-extract", "passphrase").unwrap();

            Self {
                conn,
                blobs: BlobStore::new(paths.blobs()),
                cmk: CaseMasterKey::generate().unwrap(),
                dir,
            }
        }

        /// Put a document in the case and return its artifact id.
        fn ingest(&mut self, name: &str, contents: &str) -> String {
            let path = self.dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();

            let ctx = IngestContext {
                connector: "file_import",
                job_id: "job-extract",
            };
            ingest_file(&mut self.conn, &self.blobs, &self.cmk, &path, &ctx)
                .unwrap()
                .artifact_id
        }

        fn extract(&mut self, artifact_id: &str) -> Result<Extracted> {
            self.extract_with(artifact_id, Limits::default())
        }

        fn extract_with(&mut self, artifact_id: &str, limits: Limits) -> Result<Extracted> {
            extract_artifact(&mut self.conn, &self.blobs, &self.cmk, artifact_id, limits)
        }

        fn run_status(&self, run_id: &str) -> (String, Option<String>) {
            self.conn
                .query_row(
                    "SELECT status, error_code FROM transform_run WHERE id = ?1",
                    [run_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }

        fn last_run(&self) -> String {
            self.conn
                .query_row(
                    "SELECT id FROM transform_run WHERE transform_name = ?1
                      ORDER BY started_utc DESC, rowid DESC LIMIT 1",
                    [TRANSFORM_HTML],
                    |r| r.get(0),
                )
                .unwrap()
        }

        fn count(&self, sql: &str) -> i64 {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }

        fn cleanup(self) {
            drop(self.conn);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// The whole trail in one assertion set: observations exist, each one is
    /// reachable from the artifact through a derivation edge, and the run says
    /// it succeeded.
    #[test]
    fn an_ingested_page_becomes_observations_with_lineage() {
        let mut h = Harness::new("lineage");
        let artifact_id = h.ingest("page.html", PAGE);

        let out = h.extract(&artifact_id).unwrap();

        assert_eq!(
            out.observation_ids.len(),
            6,
            "title, meta, 2 links, 1 domain, 1 email"
        );
        assert_eq!(h.run_status(&out.run_id), ("succeeded".into(), None));
        assert_eq!(out.skipped_oversize, 0);
        assert_eq!(out.superseded, 0);

        let title: String = h
            .conn
            .query_row(
                "SELECT value FROM observation WHERE kind = 'page.title'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "Acme Holdings");

        // Every observation is walkable back to the artifact via this run.
        let edges = h.count(&format!(
            "SELECT count(*) FROM derivation
              WHERE input_kind = 'artifact' AND output_kind = 'observation'
                AND run_id = '{}'",
            out.run_id
        ));
        assert_eq!(edges, 6);

        // And the locator is a real range in a real artifact.
        let locator: String = h
            .conn
            .query_row(
                "SELECT locator_json FROM observation WHERE kind = 'page.title'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            locator.contains("\"kind\":\"html_byte_range\""),
            "{locator}"
        );

        h.cleanup();
    }

    /// ADR-0006: an upgrade must never silently rewrite the basis of a
    /// conclusion an analyst already accepted.
    #[test]
    fn re_extracting_supersedes_the_earlier_reading() {
        let mut h = Harness::new("supersede");
        let artifact_id = h.ingest("page.html", PAGE);

        let first = h.extract(&artifact_id).unwrap();
        let second = h.extract(&artifact_id).unwrap();

        assert_eq!(second.superseded, 6);
        assert_eq!(h.count("SELECT count(*) FROM observation"), 12);

        // The old rows are still there, untouched, and each points at the new
        // row that replaced it.
        for old_id in &first.observation_ids {
            let replacement: Option<String> = h
                .conn
                .query_row(
                    "SELECT superseded_by FROM observation_supersession WHERE superseded_id = ?1",
                    [old_id],
                    |r| r.get(0),
                )
                .unwrap();
            let replacement = replacement.expect("the same reading should have a replacement");
            assert!(second.observation_ids.contains(&replacement));
        }

        // A third run supersedes the second, not the first again.
        let third = h.extract(&artifact_id).unwrap();
        assert_eq!(third.superseded, 6);

        h.cleanup();
    }

    /// "This is no longer observed" is a different fact from "this was replaced
    /// by that", and the schema has to be able to say it.
    #[test]
    fn a_withdrawn_observation_is_superseded_by_nothing() {
        let mut h = Harness::new("withdrawn");
        let artifact_id = h.ingest("page.html", PAGE);

        h.extract(&artifact_id).unwrap();

        // A stricter reading that can no longer record the long meta value.
        let second = h
            .extract_with(
                &artifact_id,
                Limits {
                    max_value_bytes: 14,
                    ..Limits::default()
                },
            )
            .unwrap();

        assert_eq!(second.superseded, 6);
        assert!(second.skipped_oversize > 0);

        let withdrawn =
            h.count("SELECT count(*) FROM observation_supersession WHERE superseded_by IS NULL");
        assert!(
            withdrawn > 0,
            "a claim the newer reading no longer makes must be recorded as withdrawn"
        );

        h.cleanup();
    }

    /// Running an HTML parser over something that is not HTML produces
    /// confident nonsense, and the media type is already recorded.
    #[test]
    fn a_non_html_artifact_is_refused() {
        let mut h = Harness::new("media-type");
        let artifact_id = h.ingest("notes.txt", "just some text");

        let err = h.extract(&artifact_id).unwrap_err();

        assert!(
            matches!(err, ExtractError::UnsupportedMediaType { .. }),
            "got {err:?}"
        );
        assert_eq!(
            h.run_status(&h.last_run()),
            (
                "failed".into(),
                Some("extract.unsupported_media_type".into())
            )
        );
        assert_eq!(h.count("SELECT count(*) FROM observation"), 0);

        h.cleanup();
    }

    #[test]
    fn a_missing_artifact_leaves_a_failed_run_and_no_observations() {
        let mut h = Harness::new("missing");

        let err = h.extract("no-such-artifact").unwrap_err();

        assert!(
            matches!(err, ExtractError::ArtifactMissing { .. }),
            "got {err:?}"
        );
        assert_eq!(
            h.run_status(&h.last_run()),
            ("failed".into(), Some("artifact.missing".into()))
        );
        assert_eq!(h.count("SELECT count(*) FROM observation"), 0);

        h.cleanup();
    }

    /// Crypto-shredding destroys the key, not the row. Extraction has to say
    /// which of the two happened, because "we never had it" and "we destroyed
    /// it deliberately" are different answers to an analyst's question.
    #[test]
    fn a_shredded_blob_is_reported_as_shredded() {
        let mut h = Harness::new("shredded");
        let artifact_id = h.ingest("page.html", PAGE);

        h.conn
            .execute(
                "UPDATE blob SET wrapped_key = NULL, wrapped_nonce = NULL",
                [],
            )
            .unwrap();

        let err = h.extract(&artifact_id).unwrap_err();

        assert!(
            matches!(err, ExtractError::BlobShredded { .. }),
            "got {err:?}"
        );
        assert_eq!(
            h.run_status(&h.last_run()),
            ("failed".into(), Some("blob.shredded".into()))
        );

        h.cleanup();
    }

    /// Enforced by a trigger rather than by this crate remembering not to.
    #[test]
    fn an_observation_cannot_be_edited_or_deleted() {
        let mut h = Harness::new("append-only");
        let artifact_id = h.ingest("page.html", PAGE);
        let out = h.extract(&artifact_id).unwrap();
        let id = &out.observation_ids[0];

        let edited = h.conn.execute(
            "UPDATE observation SET value = 'rewritten' WHERE id = ?1",
            [id],
        );
        assert!(edited.is_err(), "an observation must not be editable");

        let deleted = h
            .conn
            .execute("DELETE FROM observation WHERE id = ?1", [id]);
        assert!(deleted.is_err(), "an observation must not be deletable");

        h.cleanup();
    }

    #[test]
    fn an_extraction_is_recorded_in_the_audit_chain() {
        let mut h = Harness::new("audit");
        let artifact_id = h.ingest("page.html", PAGE);

        let out = h.extract(&artifact_id).unwrap();

        let (action, subject, payload): (String, String, String) = h
            .conn
            .query_row(
                "SELECT action, subject_id, payload_json FROM audit_event
                  ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();

        assert_eq!(action, "evidence.extracted");
        assert_eq!(subject, out.artifact_id);
        assert!(payload.contains("\"observations\":6"), "{payload}");

        assert!(matches!(
            kokin_store::verify_chain(&h.conn).unwrap(),
            kokin_store::ChainStatus::Intact { .. }
        ));

        h.cleanup();
    }

    /// The plan's requirement is that a parser crash yields an error code, not
    /// a lost case. Tested by making the parse panic on purpose, because a test
    /// that only ran documents which happen not to panic would prove nothing
    /// about what happens when one does.
    #[test]
    fn a_panic_in_the_parser_becomes_an_error_and_not_a_crash() {
        let previous = std::panic::take_hook();
        // The default hook would print a backtrace and make a passing test look
        // like a failing one.
        std::panic::set_hook(Box::new(|_| {}));

        let result = catching_panics(|| panic!("a parser bug reached the top"));

        std::panic::set_hook(previous);

        match result {
            Err(ExtractError::ParserPanicked { reason }) => {
                assert!(reason.contains("a parser bug reached the top"), "{reason}");
                assert_eq!(
                    classify(&ExtractError::ParserPanicked { reason }).0,
                    "extract.panicked"
                );
            }
            other => panic!("expected a caught panic, got {other:?}"),
        }
    }

    /// A failed extraction must leave the case exactly as it was, rather than
    /// a partial reading that looks like a complete one.
    #[test]
    fn a_refused_document_writes_no_observations() {
        let mut h = Harness::new("rollback");
        let mut page = String::from("<html><body>");
        for i in 0..40 {
            page.push_str(&format!("<a href=\"https://n{i}.example/\">n</a>"));
        }
        page.push_str("</body></html>");
        let artifact_id = h.ingest("many.html", &page);

        let err = h
            .extract_with(
                &artifact_id,
                Limits {
                    max_observations: 5,
                    ..Limits::default()
                },
            )
            .unwrap_err();

        assert!(
            matches!(err, ExtractError::TooManyObservations { .. }),
            "got {err:?}"
        );
        assert_eq!(h.count("SELECT count(*) FROM observation"), 0);
        assert_eq!(
            h.count("SELECT count(*) FROM derivation WHERE output_kind = 'observation'"),
            0
        );
        assert_eq!(
            h.run_status(&h.last_run()),
            (
                "failed".into(),
                Some("extract.too_many_observations".into())
            )
        );

        h.cleanup();
    }
}
