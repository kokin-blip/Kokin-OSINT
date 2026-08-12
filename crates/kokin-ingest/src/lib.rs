//! Ingest: turning a URL or a file into provenance and lineage.
//!
//! This crate composes and contains no policy of its own. The SSRF guard lives
//! in `kokin-net`, encryption in `kokin-blob` and `kokin-keys`, and the
//! append-only rules in `kokin-store`'s triggers. Ingest's only job is to write
//! the rows in the right order and record honestly what it did.
//!
//! # The order matters
//!
//! ```text
//! transform_run (running)
//!   -> source          what was asked for, raw and canonical
//!   -> blob            the bytes, content-addressed and encrypted
//!   -> capture         the retrieval, with completeness and fixture status
//!   -> artifact        the typed thing
//!   -> derivation      capture -> artifact, via this run
//! transform_run (succeeded | failed)
//! ```
//!
//! The run row is opened **before** any work and closed after, so a crash
//! leaves a `running` row rather than no evidence that anything was attempted.
//! A pipeline that records only its successes cannot be audited.

use kokin_blob::{BlobStore, PutOutcome};
use kokin_keys::CaseMasterKey;
use kokin_net::http::{HttpCapability, HttpRequest};
use kokin_net::url_norm;
use rusqlite::Connection;
use std::path::Path;

/// Identifies the code that produced a row, so a rerun after an upgrade is
/// distinguishable from the original (ADR-0006).
pub const TRANSFORM_HTTP_FETCH: &str = "ingest.http_fetch";
pub const TRANSFORM_FILE_IMPORT: &str = "ingest.file_import";
pub const TRANSFORM_VERSION: &str = "1";

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("store error: {0}")]
    Store(#[from] kokin_store::StoreError),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("blob error: {0}")]
    Blob(#[from] kokin_blob::BlobError),

    #[error("network error: {0}")]
    Http(#[from] kokin_net::http::HttpError),

    #[error("url error: {0}")]
    Url(#[from] kokin_net::url_norm::UrlError),

    #[error("random number generation failed: {0}")]
    Random(String),

    #[error("the response was {status}, which is not a successful fetch")]
    NotSuccessful { status: u16 },

    #[error("blob {content_hash} is stored but has no row in this case, and its key cannot be recovered")]
    BlobOrphaned { content_hash: String },

    #[error("{path} could not be read: {reason}")]
    FileUnreadable { path: String, reason: String },
}

pub type Result<T> = std::result::Result<T, IngestError>;

/// What a completed ingest produced, so a caller can point at the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ingested {
    pub run_id: String,
    pub source_id: String,
    pub capture_id: String,
    pub artifact_id: String,
    pub content_hash: String,
    pub size_bytes: u64,
    /// True when the bytes were already present. Recorded because "we have seen
    /// this exact content before" is an investigative fact, not just a storage
    /// optimisation.
    pub deduplicated: bool,
    /// True when this came from a fixture rather than a live fetch.
    pub from_fixture: bool,
}

/// Who is asking. Threaded through to `capture` so every retrieval is
/// attributable.
#[derive(Debug, Clone)]
pub struct IngestContext<'a> {
    pub connector: &'a str,
    pub job_id: &'a str,
}

/// Fetch a URL and record it as provenance.
pub fn ingest_url(
    conn: &mut Connection,
    blobs: &BlobStore,
    http: &dyn HttpCapability,
    cmk: &CaseMasterKey,
    url: &str,
    ctx: &IngestContext<'_>,
) -> Result<Ingested> {
    let normalised = url_norm::normalise(url)?;
    let params = params_hash(&normalised.canonical_locator);

    with_run(conn, TRANSFORM_HTTP_FETCH, &params, |conn, run_id| {
        let request = HttpRequest::get(&normalised.raw_locator);
        let response = http.send(&request)?;

        if !(200..300).contains(&response.status) {
            return Err(IngestError::NotSuccessful {
                status: response.status,
            });
        }

        // Bytes go to the blob store before the transaction opens: it writes to
        // the filesystem, which cannot participate in a SQL transaction anyway.
        let bytes = response.body.as_bytes();
        let stored = store_stream(conn, blobs, cmk, || Ok(bytes))?;

        let retrieved = Retrieved {
            source_kind: "url",
            raw_locator: normalised.raw_locator.clone(),
            canonical_locator: normalised.canonical_locator.clone(),
            http_status: Some(i64::from(response.status)),
            request_json: serde_json::to_string(&request).unwrap_or_else(|_| "{}".into()),
            response_headers_json: serde_json::to_string(&response.headers)
                .unwrap_or_else(|_| "{}".into()),
            // Phase 1 fetches static HTML only. Recording this on every capture
            // is what stops a partial snapshot of a JavaScript-heavy page being
            // mistaken for what a user would have seen.
            completeness: "static_html_only",
            from_fixture: response.from_fixture,
            media_type: media_type_of(&response.headers),
        };

        record(conn, &retrieved, &stored, ctx, run_id)
    })
}

/// Import a local file and record it as provenance.
///
/// Takes no [`HttpCapability`], so a file import structurally cannot reach the
/// network — the same reasoning that keeps `ReplayHttp` free of a client.
pub fn ingest_file(
    conn: &mut Connection,
    blobs: &BlobStore,
    cmk: &CaseMasterKey,
    path: &Path,
    ctx: &IngestContext<'_>,
) -> Result<Ingested> {
    // Verbatim, as given. The canonical form below is what identity rests on;
    // this is what was actually asked for, and editing it would falsify the
    // record (increment 5b, applied to paths instead of URLs).
    let raw_locator = path.display().to_string();
    let canonical_locator = canonical_file_locator(path)?;
    let params = params_hash(&canonical_locator);

    with_run(conn, TRANSFORM_FILE_IMPORT, &params, |conn, run_id| {
        // Streamed rather than read into memory, so the blob store's byte cap is
        // enforced *as* the file is read (A-011). A file larger than the cap
        // must not become an allocation that size before anything checks it.
        let stored = store_stream(conn, blobs, cmk, || {
            std::fs::File::open(path).map_err(|e| IngestError::FileUnreadable {
                path: raw_locator.clone(),
                reason: e.to_string(),
            })
        })?;

        let retrieved = Retrieved {
            source_kind: "file",
            raw_locator: raw_locator.clone(),
            canonical_locator: canonical_locator.clone(),
            // There was no request and no server, so these stay empty rather
            // than being filled with something that looks like an HTTP exchange.
            http_status: None,
            request_json: "{}".to_string(),
            response_headers_json: "{}".to_string(),
            // Unlike a static-HTML fetch, every byte of the file was read.
            completeness: "complete",
            from_fixture: false,
            media_type: media_type_of_path(path),
        };

        record(conn, &retrieved, &stored, ctx, run_id)
    })
}

/// Open a `transform_run`, do the work, and close the run either way.
///
/// The run row is written and committed **before** the work starts, so a crash
/// mid-ingest leaves a visible `running` row rather than no evidence that
/// anything was attempted. A pipeline that records only its successes cannot be
/// audited, and "nothing happened" must not look like "something happened and
/// failed".
fn with_run<F>(
    conn: &mut Connection,
    transform_name: &str,
    params_hash: &str,
    work: F,
) -> Result<Ingested>
where
    F: FnOnce(&mut Connection, &str) -> Result<Ingested>,
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
        Ok(ingested) => {
            complete_run(conn, &run_id, "succeeded", None, None)?;
            Ok(ingested)
        }
        Err(e) => {
            // The failure is recorded on the run before it is returned, so a
            // failed attempt leaves a trace an analyst can find later.
            let (code, detail) = classify(&e);
            let _ = complete_run(conn, &run_id, "failed", Some(code), Some(&detail));
            Err(e)
        }
    }
}

/// One retrieval, in the form the L0 tables need.
///
/// Shared by both ingest paths so that a file and a URL cannot drift into
/// writing provenance with different shapes.
struct Retrieved {
    source_kind: &'static str,
    raw_locator: String,
    canonical_locator: String,
    http_status: Option<i64>,
    request_json: String,
    response_headers_json: String,
    completeness: &'static str,
    from_fixture: bool,
    media_type: String,
}

/// Write the provenance and lineage rows for one retrieval.
///
/// Everything happens in one database transaction: if any step fails, the case
/// is left exactly as it was rather than holding a capture with no artifact or
/// an artifact with no blob.
fn record(
    conn: &mut Connection,
    retrieved: &Retrieved,
    stored: &StoredBytes,
    ctx: &IngestContext<'_>,
    run_id: &str,
) -> Result<Ingested> {
    let StoredBytes {
        content_hash,
        size,
        wrapped,
        deduplicated,
    } = stored;

    let new_source_id = new_id()?;
    let capture_id = new_id()?;
    let artifact_id = new_id()?;
    let now = now_utc();

    let tx = conn.transaction()?;

    // A source is identity, so an existing canonical locator is reused rather
    // than duplicated - the same page fetched twice is one source with two
    // captures, which is what makes "when did this change?" answerable.
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM source WHERE canonical_locator = ?1",
            [&retrieved.canonical_locator],
            |r| r.get(0),
        )
        .ok();

    let source_id = match existing {
        Some(id) => id,
        None => {
            tx.execute(
                "INSERT INTO source (id, kind, raw_locator, canonical_locator, first_seen_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    new_source_id,
                    retrieved.source_kind,
                    retrieved.raw_locator,
                    retrieved.canonical_locator,
                    now
                ],
            )?;
            new_source_id
        }
    };

    if let Some((nonce, ciphertext)) = wrapped {
        tx.execute(
            "INSERT INTO blob (content_hash, size_bytes, wrapped_key, wrapped_nonce, stored_utc, shredded_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
            rusqlite::params![content_hash, *size as i64, ciphertext, nonce, now],
        )?;
    }

    tx.execute(
        "INSERT INTO capture
            (id, source_id, content_hash, requested_utc, http_status, request_json,
             response_headers_json, redirect_chain_json, capture_completeness,
             from_fixture, connector, job_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '[]', ?8, ?9, ?10, ?11)",
        rusqlite::params![
            capture_id,
            source_id,
            content_hash,
            now,
            retrieved.http_status,
            retrieved.request_json,
            retrieved.response_headers_json,
            retrieved.completeness,
            i64::from(retrieved.from_fixture),
            ctx.connector,
            ctx.job_id,
        ],
    )?;

    tx.execute(
        "INSERT INTO artifact (id, content_hash, media_type, byte_length, created_utc)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            artifact_id,
            content_hash,
            retrieved.media_type,
            *size as i64,
            now
        ],
    )?;

    for (input_kind, input_id, output_kind, output_id) in [
        ("source", source_id.as_str(), "capture", capture_id.as_str()),
        (
            "capture",
            capture_id.as_str(),
            "artifact",
            artifact_id.as_str(),
        ),
    ] {
        tx.execute(
            "INSERT INTO derivation (id, run_id, input_kind, input_id, output_kind, output_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                new_id()?,
                run_id,
                input_kind,
                input_id,
                output_kind,
                output_id
            ],
        )?;
    }

    kokin_store::audit::append(
        &tx,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action: "evidence.ingested",
            subject_kind: Some("capture"),
            subject_id: Some(&capture_id),
            payload_json: &format!(
                "{{\"canonical_locator\":{},\"from_fixture\":{}}}",
                serde_json::to_string(&retrieved.canonical_locator)
                    .unwrap_or_else(|_| "\"\"".into()),
                retrieved.from_fixture
            ),
        },
    )?;

    tx.commit()?;

    Ok(Ingested {
        run_id: run_id.to_string(),
        source_id,
        capture_id,
        artifact_id,
        content_hash: content_hash.clone(),
        size_bytes: *size,
        deduplicated: *deduplicated,
        from_fixture: retrieved.from_fixture,
    })
}

/// What the blob store did with the bytes, reconciled against this case.
struct StoredBytes {
    content_hash: String,
    size: u64,
    /// `Some` when a `blob` row still has to be written. Nonce, then ciphertext.
    wrapped: Option<(Vec<u8>, Vec<u8>)>,
    deduplicated: bool,
}

/// Store bytes, and make sure the case can still read them afterwards.
///
/// `put` reports `AlreadyPresent` from the presence of the file alone, and
/// **discards the per-blob key when it does**. That is right for the store,
/// which cannot know about cases, but it is not enough here. If an earlier
/// ingest wrote the file and then its transaction rolled back, the file exists
/// with no `blob` row and no key that could ever decrypt it. Believing the
/// dedupe hit in that state would insert a `capture` referencing a
/// `content_hash` that has no row - a foreign-key failure, repeated on every
/// retry, that would make exactly those bytes permanently un-ingestable.
///
/// So a dedupe hit is trusted only when the row backing it exists. Otherwise
/// the orphan is discarded and the bytes are stored again under a fresh key.
/// That recovery is safe precisely because the store is content-addressed: the
/// bytes are identical either way, and only the unreadable copy is lost.
///
/// `open` yields the bytes and may be called **twice**, since recovering from an
/// orphan means storing them again. It takes a factory rather than a reader for
/// exactly that reason: a consumed `Read` cannot be replayed, and a file that
/// changed between the two calls would produce a different hash — which the
/// second `put` would then store under its own name, leaving the first one's
/// row to be written for content that is no longer there.
fn store_stream<R, F>(
    conn: &Connection,
    blobs: &BlobStore,
    cmk: &CaseMasterKey,
    open: F,
) -> Result<StoredBytes>
where
    R: std::io::Read,
    F: Fn() -> Result<R>,
{
    let (content_hash, size) = match blobs.put(open()?, cmk)? {
        PutOutcome::Stored(b) => return Ok(newly_stored(b)),
        PutOutcome::AlreadyPresent { content_hash, size } => (content_hash, size),
    };

    if blob_row_exists(conn, &content_hash)? {
        return Ok(StoredBytes {
            content_hash,
            size,
            wrapped: None,
            deduplicated: true,
        });
    }

    blobs.remove(&content_hash, cmk)?;
    match blobs.put(open()?, cmk)? {
        PutOutcome::Stored(b) => Ok(newly_stored(b)),
        // The file was removed a line ago, so reaching this means something
        // outside this process is writing the same blob store concurrently.
        PutOutcome::AlreadyPresent { content_hash, .. } => {
            Err(IngestError::BlobOrphaned { content_hash })
        }
    }
}

fn newly_stored(b: kokin_blob::BlobRef) -> StoredBytes {
    StoredBytes {
        content_hash: b.content_hash,
        size: b.size,
        wrapped: Some((b.wrapped_key.nonce.to_vec(), b.wrapped_key.ciphertext)),
        deduplicated: false,
    }
}

fn blob_row_exists(conn: &Connection, content_hash: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM blob WHERE content_hash = ?1",
        [content_hash],
        |r| r.get(0),
    )?;
    Ok(count > 0)
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

/// Map an error to a stable code.
///
/// Stable because these end up in `transform_run.error_code`, where they are
/// queried and counted. A message is for a human; a code is for a query.
fn classify(e: &IngestError) -> (&'static str, String) {
    let code = match e {
        IngestError::Http(kokin_net::http::HttpError::FixtureMissing { .. }) => "net.no_fixture",
        IngestError::Http(_) => "net.failed",
        IngestError::Url(_) => "url.invalid",
        IngestError::NotSuccessful { .. } => "http.not_successful",
        IngestError::Blob(kokin_blob::BlobError::TooLarge { .. }) => "blob.too_large",
        IngestError::Blob(_) => "blob.failed",
        IngestError::BlobOrphaned { .. } => "blob.orphaned",
        IngestError::FileUnreadable { .. } => "file.unreadable",
        IngestError::Store(_) | IngestError::Sqlite(_) => "store.failed",
        IngestError::Random(_) => "random.failed",
    };
    (code, e.to_string())
}

/// Media type from the response, defaulting conservatively.
///
/// A missing or unparseable `Content-Type` becomes `application/octet-stream`
/// rather than a guess: claiming bytes are HTML when the server did not say so
/// is exactly the kind of unlabelled inference this project is meant to avoid.
fn media_type_of(headers: &std::collections::BTreeMap<String, String>) -> String {
    headers
        .get("content-type")
        .map(|v| {
            v.split(';')
                .next()
                .unwrap_or("application/octet-stream")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_else(|| "application/octet-stream".to_string())
}

/// The identity of a file on disk.
///
/// `canonicalize` resolves `.`, `..` and symlinks, so two paths that reach the
/// same file are one source with two captures rather than two unrelated sources.
/// The Windows extended-length `\\?\` prefix is stripped and separators are
/// normalised to `/`, so a case bundle carries the same locator no matter which
/// platform wrote it.
///
/// **Case is deliberately not folded.** Windows paths are case-insensitive, so
/// `C:/A/x.txt` and `c:/a/X.TXT` are one file but become two sources. Folding
/// would be wrong on Linux and would make case files non-portable between
/// platforms, so the failure is left visible and benign — identity fragments,
/// evidence is not corrupted. Same tradeoff as the tracking-parameter list in
/// increment 5b.
fn canonical_file_locator(path: &Path) -> Result<String> {
    let resolved = std::fs::canonicalize(path).map_err(|e| IngestError::FileUnreadable {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let shown = resolved.display().to_string();
    let trimmed = shown.strip_prefix(r"\\?\").unwrap_or(shown.as_str());
    let slashed = trimmed.replace('\\', "/");

    Ok(format!("file:///{}", slashed.trim_start_matches('/')))
}

/// Media type for a file, from its extension.
///
/// This is the **filename's claim**, not an observation: a file named `.html`
/// may hold anything at all. It is recorded because the extractor needs a
/// routing hint and Phase 1 has no content sniffer, and the table is
/// deliberately narrow — anything unrecognised stays `application/octet-stream`
/// rather than becoming a guess. Detection from the bytes belongs with the
/// extractor, which is the code that actually looks at them.
fn media_type_of_path(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "html" | "htm" => "text/html",
        "txt" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// The build identity recorded on every run.
fn code_version() -> String {
    format!("kokin-ingest {}", env!("CARGO_PKG_VERSION"))
}

fn params_hash(canonical: &str) -> String {
    // Deterministic, so two runs with the same parameters are comparable.
    blake3_hex(canonical.as_bytes())
}

fn blake3_hex(bytes: &[u8]) -> String {
    // Re-exported through kokin-store rather than taking a direct dependency
    // for one call.
    kokin_store::blake3_hex(bytes)
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| IngestError::Random(e.to_string()))?;
    Ok(blake3_hex(&bytes)[..32].to_string())
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
    use kokin_net::http::{Fixture, HttpResponse, ReplayHttp};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    const URL: &str = "https://example.com/page";
    const BODY: &str = "<html><head><title>Hello</title></head><body>evidence</body></html>";

    fn temp_dir(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("kokin-ingest-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    /// A case, a blob store inside it, and a replay capability with no fixtures.
    struct Harness {
        conn: Connection,
        blobs: BlobStore,
        http: ReplayHttp,
        cmk: CaseMasterKey,
        dir: PathBuf,
    }

    impl Harness {
        fn new(name: &str) -> Self {
            let dir = temp_dir(name);
            let case_dir = dir.join("case.kokincase");
            let (conn, paths, _recovery) =
                kokin_store::create_case(&case_dir, "case-ingest", "passphrase").unwrap();

            Self {
                conn,
                blobs: BlobStore::new(paths.blobs()),
                http: ReplayHttp::new(dir.join("fixtures")),
                cmk: CaseMasterKey::generate().unwrap(),
                dir,
            }
        }

        /// Record a fixture so the URL can be "fetched" offline.
        fn record(&self, url: &str, status: u16, content_type: &str, body: &str) {
            let request = HttpRequest::get(url);
            let mut headers = BTreeMap::new();
            headers.insert("content-type".to_string(), content_type.to_string());

            self.http
                .store(&Fixture {
                    key: request.fixture_key().unwrap(),
                    request,
                    response: HttpResponse {
                        status,
                        headers,
                        body: body.to_string(),
                        from_fixture: false,
                    },
                    recorded_utc: "2026-08-12T00:00:00Z".to_string(),
                    note: None,
                })
                .unwrap();
        }

        fn ingest(&mut self, url: &str) -> Result<Ingested> {
            let ctx = IngestContext {
                connector: "http_fetch",
                job_id: "job-001",
            };
            ingest_url(
                &mut self.conn,
                &self.blobs,
                &self.http,
                &self.cmk,
                url,
                &ctx,
            )
        }

        /// Lower the blob ceiling, so the streaming cap can be tested without
        /// writing a file the size of the real one.
        fn with_max_blob_bytes(mut self, limit: u64) -> Self {
            self.blobs = self.blobs.clone().with_max_blob_bytes(limit);
            self
        }

        fn write_file(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
            path
        }

        fn import(&mut self, path: &Path) -> Result<Ingested> {
            let ctx = IngestContext {
                connector: "file_import",
                job_id: "job-002",
            };
            ingest_file(&mut self.conn, &self.blobs, &self.cmk, path, &ctx)
        }

        fn count(&self, sql: &str) -> i64 {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }

        fn cleanup(self) {
            let dir = self.dir;
            drop(self.conn);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The whole point of the increment: one fetch leaves a complete, connected
    /// trail from what was asked for to the bytes that came back.
    #[test]
    fn a_fetched_url_becomes_provenance_and_lineage() {
        let mut h = Harness::new("happy");
        h.record(URL, 200, "text/html; charset=utf-8", BODY);

        let ingested = h.ingest(URL).unwrap();

        assert!(ingested.from_fixture, "a replayed fetch must say so");
        assert!(!ingested.deduplicated);
        assert_eq!(ingested.size_bytes, BODY.len() as u64);

        // Each L0 row exists exactly once.
        assert_eq!(h.count("SELECT count(*) FROM source"), 1);
        assert_eq!(h.count("SELECT count(*) FROM blob"), 1);
        assert_eq!(h.count("SELECT count(*) FROM capture"), 1);
        assert_eq!(h.count("SELECT count(*) FROM artifact"), 1);

        let (source_id, status, completeness, from_fixture, connector, job): (
            String,
            i64,
            String,
            i64,
            String,
            String,
        ) = h
            .conn
            .query_row(
                "SELECT source_id, http_status, capture_completeness, from_fixture, connector, job_id
                   FROM capture WHERE id = ?1",
                [&ingested.capture_id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(source_id, ingested.source_id);
        assert_eq!(status, 200);
        // A static-HTML fetch must never be mistaken for what a user would see.
        assert_eq!(completeness, "static_html_only");
        assert_eq!(from_fixture, 1, "a replayed capture claimed to be live");
        assert_eq!(connector, "http_fetch");
        assert_eq!(job, "job-001");

        // The media type is what the server said, with parameters dropped.
        let media_type: String = h
            .conn
            .query_row(
                "SELECT media_type FROM artifact WHERE id = ?1",
                [&ingested.artifact_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(media_type, "text/html");

        // The canonical locator, not the raw one, is what identity rests on.
        let canonical: String = h
            .conn
            .query_row(
                "SELECT canonical_locator FROM source WHERE id = ?1",
                [&ingested.source_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            canonical,
            url_norm::normalise(URL).unwrap().canonical_locator
        );

        // Lineage: the chain is walkable in both directions, via this run.
        let edges: Vec<(String, String, String, String)> = {
            let mut stmt = h
                .conn
                .prepare(
                    "SELECT input_kind, input_id, output_kind, output_id
                       FROM derivation WHERE run_id = ?1 ORDER BY input_kind",
                )
                .unwrap();
            let rows = stmt
                .query_map([&ingested.run_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })
                .unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            edges,
            vec![
                (
                    "capture".to_string(),
                    ingested.capture_id.clone(),
                    "artifact".to_string(),
                    ingested.artifact_id.clone()
                ),
                (
                    "source".to_string(),
                    ingested.source_id.clone(),
                    "capture".to_string(),
                    ingested.capture_id.clone()
                ),
            ]
        );

        // The run is closed, and closed as a success.
        let (run_status, finished, error_code): (String, Option<String>, Option<String>) = h
            .conn
            .query_row(
                "SELECT status, finished_utc, error_code FROM transform_run WHERE id = ?1",
                [&ingested.run_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(run_status, "succeeded");
        assert!(finished.is_some(), "a finished run must record when");
        assert_eq!(error_code, None);

        h.cleanup();
    }

    /// The same page fetched twice is one source with two captures - which is
    /// what makes "when did this change?" a question the case can answer.
    #[test]
    fn refetching_a_url_adds_a_capture_without_duplicating_the_source() {
        let mut h = Harness::new("refetch");
        h.record(URL, 200, "text/html", BODY);

        let first = h.ingest(URL).unwrap();
        let second = h.ingest(URL).unwrap();

        assert_eq!(first.source_id, second.source_id, "identity must be stable");
        assert_ne!(first.capture_id, second.capture_id);
        assert_eq!(first.content_hash, second.content_hash);

        assert_eq!(h.count("SELECT count(*) FROM source"), 1);
        assert_eq!(h.count("SELECT count(*) FROM capture"), 2);
        // Identical bytes are stored once.
        assert_eq!(h.count("SELECT count(*) FROM blob"), 1);
        assert!(
            second.deduplicated,
            "seeing the same content twice is an investigative fact, not just a storage detail"
        );

        h.cleanup();
    }

    /// A pipeline that records only its successes cannot be audited.
    #[test]
    fn a_missing_fixture_leaves_a_failed_run_and_no_evidence() {
        let mut h = Harness::new("nofixture");

        let err = h.ingest(URL).unwrap_err();
        assert!(
            matches!(
                err,
                IngestError::Http(kokin_net::http::HttpError::FixtureMissing { .. })
            ),
            "a cache miss under replay must be a failure, never a live call: {err}"
        );

        // The attempt is visible, with a code a query can count.
        let (status, code): (String, Option<String>) = h
            .conn
            .query_row("SELECT status, error_code FROM transform_run", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(code.as_deref(), Some("net.no_fixture"));

        // But nothing is claimed as evidence.
        assert_eq!(h.count("SELECT count(*) FROM capture"), 0);
        assert_eq!(h.count("SELECT count(*) FROM artifact"), 0);
        assert_eq!(h.count("SELECT count(*) FROM source"), 0);

        h.cleanup();
    }

    /// A 404 body is not the page. Recording it as an artifact would let an
    /// error page be cited as evidence of what was at that URL.
    #[test]
    fn an_unsuccessful_response_is_not_recorded_as_evidence() {
        let mut h = Harness::new("notfound");
        h.record(URL, 404, "text/html", "<h1>Not Found</h1>");

        let err = h.ingest(URL).unwrap_err();
        assert!(matches!(err, IngestError::NotSuccessful { status: 404 }));

        let code: Option<String> = h
            .conn
            .query_row("SELECT error_code FROM transform_run", [], |r| r.get(0))
            .unwrap();
        assert_eq!(code.as_deref(), Some("http.not_successful"));

        assert_eq!(h.count("SELECT count(*) FROM capture"), 0);
        assert_eq!(h.count("SELECT count(*) FROM artifact"), 0);
        assert_eq!(h.count("SELECT count(*) FROM blob"), 0);

        h.cleanup();
    }

    /// A blob file left behind by a rolled-back ingest must not make those exact
    /// bytes permanently un-ingestable.
    ///
    /// `put` reports `AlreadyPresent` from the file alone and throws away the
    /// per-blob key, so without reconciliation the retry would insert a capture
    /// referencing a `content_hash` with no row - and the control below proves
    /// that reference really is refused, so this is a live failure and not a
    /// hypothetical one.
    #[test]
    fn an_orphaned_blob_does_not_poison_later_ingests() {
        let mut h = Harness::new("orphan");
        h.record(URL, 200, "text/html", BODY);

        // Control: a capture pointing at a blob with no row is rejected. If this
        // ever stops failing, the test below has stopped proving anything.
        let dangling = h.conn.execute(
            "INSERT INTO capture
                (id, source_id, content_hash, requested_utc, capture_completeness,
                 from_fixture, connector, job_id)
             VALUES ('c1', 's1', 'deadbeef', '2026-01-01T00:00:00Z', 'static_html_only', 1, 'x', 'y')",
            [],
        );
        assert!(
            dangling.is_err(),
            "the foreign key onto blob is not enforced - this test proves nothing"
        );

        // Simulate the aftermath of a rolled-back ingest: the bytes are on disk
        // and the key that could read them is gone.
        let orphan = h.blobs.put(BODY.as_bytes(), &h.cmk).unwrap();
        let orphan_hash = orphan.content_hash().to_string();
        assert_eq!(h.count("SELECT count(*) FROM blob"), 0);

        let ingested = h.ingest(URL).unwrap();

        assert_eq!(ingested.content_hash, orphan_hash);
        assert!(
            !ingested.deduplicated,
            "an unreadable orphan is not a dedupe hit"
        );
        assert_eq!(h.count("SELECT count(*) FROM blob"), 1);

        // And the recovery is real: the bytes read back through the key that was
        // actually stored in the case.
        let (size, key, nonce): (i64, Vec<u8>, Vec<u8>) = h
            .conn
            .query_row(
                "SELECT size_bytes, wrapped_key, wrapped_nonce FROM blob WHERE content_hash = ?1",
                [&ingested.content_hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();

        let blob_ref = kokin_blob::BlobRef {
            content_hash: ingested.content_hash.clone(),
            size: size as u64,
            wrapped_key: kokin_keys::WrappedKey {
                nonce: nonce.try_into().unwrap(),
                ciphertext: key,
            },
        };
        let plaintext = h.blobs.get(&blob_ref, &h.cmk).unwrap();
        assert_eq!(String::from_utf8(plaintext).unwrap(), BODY);

        h.cleanup();
    }

    /// The file half of the increment: a local file becomes the same shape of
    /// provenance as a fetch, without going near the network.
    #[test]
    fn a_local_file_becomes_provenance_and_lineage() {
        let mut h = Harness::new("file");
        let path = h.write_file("evidence.html", BODY);

        let ingested = h.import(&path).unwrap();

        assert!(!ingested.from_fixture, "a real file read is not a fixture");
        assert_eq!(ingested.size_bytes, BODY.len() as u64);

        assert_eq!(h.count("SELECT count(*) FROM source"), 1);
        assert_eq!(h.count("SELECT count(*) FROM blob"), 1);
        assert_eq!(h.count("SELECT count(*) FROM capture"), 1);
        assert_eq!(h.count("SELECT count(*) FROM artifact"), 1);

        let (kind, canonical): (String, String) = h
            .conn
            .query_row(
                "SELECT kind, canonical_locator FROM source WHERE id = ?1",
                [&ingested.source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "file");
        assert!(
            canonical.starts_with("file:///") && !canonical.contains('\\'),
            "a file locator must be platform-neutral, got: {canonical}"
        );

        // There was no server, so nothing pretends there was one.
        let (status, completeness, from_fixture, connector): (Option<i64>, String, i64, String) = h
            .conn
            .query_row(
                "SELECT http_status, capture_completeness, from_fixture, connector
                   FROM capture WHERE id = ?1",
                [&ingested.capture_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(status, None, "a file read has no HTTP status");
        assert_eq!(completeness, "complete");
        assert_eq!(from_fixture, 0);
        assert_eq!(connector, "file_import");

        // The run is attributed to the file transform, not the fetch one.
        let (name, run_status): (String, String) = h
            .conn
            .query_row(
                "SELECT transform_name, status FROM transform_run WHERE id = ?1",
                [&ingested.run_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, TRANSFORM_FILE_IMPORT);
        assert_eq!(run_status, "succeeded");

        // Lineage has the same shape as the URL path.
        assert_eq!(
            h.count(&format!(
                "SELECT count(*) FROM derivation WHERE run_id = '{}'",
                ingested.run_id
            )),
            2
        );

        h.cleanup();
    }

    /// Two paths that reach the same file are one source. Without resolving
    /// `..`, the same evidence imported from a different working directory
    /// would fragment into two unrelated sources.
    #[test]
    fn two_paths_to_the_same_file_are_one_source() {
        let mut h = Harness::new("samefile");
        let direct = h.write_file("nested/evidence.html", BODY);
        let indirect = h.dir.join("nested").join("..").join("nested/evidence.html");

        let first = h.import(&direct).unwrap();
        let second = h.import(&indirect).unwrap();

        assert_eq!(first.source_id, second.source_id);
        assert_eq!(h.count("SELECT count(*) FROM source"), 1);
        assert_eq!(h.count("SELECT count(*) FROM capture"), 2);
        assert!(second.deduplicated);

        // The raw locator still records what was actually asked for, verbatim.
        let raw: String = h
            .conn
            .query_row(
                "SELECT raw_locator FROM source WHERE id = ?1",
                [&first.source_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, direct.display().to_string());

        h.cleanup();
    }

    /// The same bytes reached two different ways are stored once, but remain
    /// two sources — where evidence came from is not the same question as what
    /// it contains.
    #[test]
    fn a_file_and_a_url_with_the_same_bytes_share_one_blob() {
        let mut h = Harness::new("crosspath");
        h.record(URL, 200, "text/html", BODY);
        let path = h.write_file("same.html", BODY);

        let fetched = h.ingest(URL).unwrap();
        let imported = h.import(&path).unwrap();

        assert_eq!(fetched.content_hash, imported.content_hash);
        assert!(imported.deduplicated);
        assert_ne!(fetched.source_id, imported.source_id);

        assert_eq!(h.count("SELECT count(*) FROM blob"), 1);
        assert_eq!(h.count("SELECT count(*) FROM source"), 2);
        assert_eq!(h.count("SELECT count(*) FROM artifact"), 2);

        h.cleanup();
    }

    /// A path that is not there must fail by name, and leave the attempt on the
    /// record like any other failure.
    #[test]
    fn a_missing_file_leaves_a_failed_run_and_no_evidence() {
        let mut h = Harness::new("nofile");
        let missing = h.dir.join("not-here.html");

        let err = h.import(&missing).unwrap_err();
        assert!(
            matches!(err, IngestError::FileUnreadable { .. }),
            "expected a named unreadable-file error, got: {err}"
        );

        assert_eq!(h.count("SELECT count(*) FROM source"), 0);
        assert_eq!(h.count("SELECT count(*) FROM capture"), 0);

        h.cleanup();
    }

    /// The byte cap is enforced during the read, so an oversized file is
    /// stopped rather than stored and then measured (A-011).
    #[test]
    fn an_oversized_file_is_refused_and_recorded_as_such() {
        let mut h = Harness::new("toobig").with_max_blob_bytes(64);
        let path = h.write_file("big.txt", &"x".repeat(4096));

        let err = h.import(&path).unwrap_err();
        assert!(
            matches!(
                err,
                IngestError::Blob(kokin_blob::BlobError::TooLarge { .. })
            ),
            "expected the cap to refuse it, got: {err}"
        );

        let code: Option<String> = h
            .conn
            .query_row("SELECT error_code FROM transform_run", [], |r| r.get(0))
            .unwrap();
        assert_eq!(code.as_deref(), Some("blob.too_large"));

        // Nothing was kept: not the bytes, not a row claiming them.
        assert_eq!(h.count("SELECT count(*) FROM blob"), 0);
        assert_eq!(h.count("SELECT count(*) FROM artifact"), 0);

        h.cleanup();
    }

    /// The media type is the filename's claim and is labelled narrowly. An
    /// unrecognised extension must not become a guess.
    #[test]
    fn an_unknown_extension_is_not_guessed_at() {
        let mut h = Harness::new("mediatype");
        let known = h.write_file("page.HTM", BODY);
        let unknown = h.write_file("mystery.qqq", "some other bytes");

        let a = h.import(&known).unwrap();
        let b = h.import(&unknown).unwrap();

        let media_of = |id: &str| -> String {
            h.conn
                .query_row("SELECT media_type FROM artifact WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .unwrap()
        };

        assert_eq!(media_of(&a.artifact_id), "text/html", "extension is cased");
        assert_eq!(media_of(&b.artifact_id), "application/octet-stream");

        h.cleanup();
    }

    /// Ingest is a case event, and the chain must still verify with it in.
    #[test]
    fn an_ingest_is_recorded_in_the_audit_chain() {
        let mut h = Harness::new("audit");
        h.record(URL, 200, "text/html", BODY);

        let ingested = h.ingest(URL).unwrap();

        let (action, subject, payload): (String, String, String) = h
            .conn
            .query_row(
                "SELECT action, subject_id, payload_json FROM audit_event
                  ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();

        assert_eq!(action, "evidence.ingested");
        assert_eq!(subject, ingested.capture_id);
        assert!(
            payload.contains("\"from_fixture\":true"),
            "the log must not let a replay pass as a live fetch: {payload}"
        );

        assert!(matches!(
            kokin_store::verify_chain(&h.conn).unwrap(),
            kokin_store::ChainStatus::Intact { .. }
        ));

        h.cleanup();
    }
}
