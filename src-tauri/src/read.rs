//! The reads behind the interface: search, a document, an entity.
//!
//! Nothing here decides anything. Every judgement — what ordering means, what
//! coverage counts, what an entity resolves to — was made in a `kokin-*` crate
//! and is being assembled into a shape the webview can render. What this module
//! *does* decide is what the interface is not allowed to get wrong, and there
//! are three of those:
//!
//! 1. A result list cannot be rendered without its coverage caveat, because
//!    they arrive in the same value ([`search`], D-029).
//! 2. A document that cannot be shown says which of five reasons applies, and
//!    "somebody destroyed this on purpose" is never rendered as "something is
//!    broken" ([`artifact_document`]).
//! 3. An entity shows all seven confidence dimensions whether or not anyone has
//!    assessed them, because a missing dimension reads as a settled one
//!    ([`entity`], D-031).

use std::collections::BTreeMap;

use kokin_store::OpenCase;
use rusqlite::Connection;

use crate::error::{CommandError, ErrorCode};
use crate::views::{
    ArtifactRowView, CaptureRefView, CollectionView, CoverageView, DecisionView,
    DimensionValueView, DimensionView, DocumentContent, DocumentView, EntityRefView, EntityRowView,
    EntityView, EvidenceListView, EvidenceView, ExcerptView, GapView, HitView, IdentifierView,
    LineageView, LocatorView, ObservationRowView, ObservationView, QuoteView, RunView,
    SearchRequest, SearchView, SourceRefView, SupersessionView,
};

/// A database failure that is not an answer about anything.
///
/// Every `map_err` in this module that reaches for it is saying the same thing:
/// the query itself broke, which is a fault, as distinct from the query
/// returning nothing — which is usually a fact about the case and is handled
/// explicitly wherever it is one.
fn storage(e: rusqlite::Error) -> CommandError {
    CommandError::new(ErrorCode::Storage, e.to_string())
}

/// The most results one call will return.
///
/// A clamp rather than a rejection, because an interface asking for too many is
/// not making an error the analyst should have to see. It is honest only because
/// [`SearchView::total`] travels with it: a clamped page is visibly a page.
const MAX_HITS: usize = 200;

/// How much of a document is sent to the webview at once.
///
/// The excerpt exists because this crosses IPC as a JSON string and a case may
/// legitimately hold a two-gigabyte artifact. It is not a limit on what was
/// *verified*: the whole document is decrypted and hashed, and only then is the
/// head returned — so an excerpt describes a document known to be intact.
const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search the case, and say how much of it the search could reach.
///
/// Coverage is computed on every call rather than cached or offered separately.
/// It scans lineage, which is small next to the documents it describes, and the
/// alternative — a caller that must remember to go and fetch it — is a caveat
/// that does not get fetched and a gap that stays exactly as invisible as it was
/// before any of this existed.
pub fn search(open: &OpenCase, request: &SearchRequest) -> Result<SearchView, CommandError> {
    let query = if request.typeahead {
        kokin_search::Query::parse_for_typeahead(&request.query)
    } else {
        kokin_search::Query::parse(&request.query)
    };

    let options = kokin_search::SearchOptions {
        limit: request.limit.unwrap_or(50).min(MAX_HITS),
        offset: request.offset,
        kinds: request.kinds.clone(),
        facets: request.facets.clone(),
        include_superseded: request.include_superseded,
        resolve_merged: request.resolve_merged.unwrap_or(true),
    };

    let hits = kokin_search::search(&open.conn, &query, &options)?
        .into_iter()
        .map(|hit| HitView {
            subject_kind: hit.subject_kind,
            subject_id: hit.subject_id,
            title: hit.title,
            snippet: hit.snippet,
            facet: hit.facet,
            superseded: hit.superseded,
            canonical_id: hit.canonical_id,
            // `hit.relevance` is dropped here, deliberately. See views.rs.
        })
        .collect();

    Ok(SearchView {
        hits,
        total: kokin_search::count(&open.conn, &query, &options)?,
        coverage: coverage(&open.conn)?,
    })
}

/// How much of this case a search can reach, on its own.
///
/// Exposed as a command as well as inside [`SearchView`] because a case overview
/// wants it before anybody has searched. The dependency runs the right way:
/// coverage without results is fine, results without coverage is the thing that
/// is not representable.
pub fn coverage(conn: &Connection) -> Result<CoverageView, CommandError> {
    let c = kokin_search::coverage::coverage(conn)?;
    Ok(CoverageView {
        artifacts: c.artifacts,
        never_attempted: c.never_attempted,
        in_progress: c.in_progress,
        failed: c.failed,
        partial: c.partial,
        complete: c.complete,
        incomplete: c.incomplete(),
        is_complete: c.is_complete(),
    })
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

/// Show an artifact, or say precisely why it cannot be shown.
///
/// The artifact row is required — a missing one is `NotFound`, a genuine command
/// failure — but everything about the *bytes* is reported as content rather than
/// as an error. `blob_access` answers three of the five states from the database
/// alone; the other two need the read to be attempted, and this is the only
/// place in the application that attempts it.
pub fn artifact_document(open: &OpenCase, artifact_id: &str) -> Result<DocumentView, CommandError> {
    let (content_hash, media_type, byte_length): (String, String, i64) = open
        .conn
        .query_row(
            "SELECT content_hash, media_type, byte_length FROM artifact WHERE id = ?1",
            [artifact_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CommandError::new(
                ErrorCode::NotFound,
                format!("no artifact {artifact_id} in this case"),
            ),
            other => CommandError::new(ErrorCode::Storage, other.to_string()),
        })?;

    let content = match kokin_store::blob_access(&open.conn, &content_hash)? {
        kokin_store::BlobAccess::Shredded { shredded_utc } => {
            DocumentContent::Shredded { shredded_utc }
        }
        kokin_store::BlobAccess::Unrecorded => DocumentContent::Unrecorded,
        kokin_store::BlobAccess::Readable(reference) => read_excerpt(open, &reference),
    };

    Ok(DocumentView {
        artifact_id: artifact_id.to_string(),
        media_type,
        byte_length,
        content_hash,
        content,
    })
}

/// Decrypt the whole document, keep the first [`MAX_DOCUMENT_BYTES`] of it.
///
/// The failures here are answers about this document, so none of them is an
/// error: the sink counts what it discards, and the blob store's own verdict —
/// gone, or modified — becomes a state rather than a red box.
fn read_excerpt(open: &OpenCase, reference: &kokin_blob::BlobRef) -> DocumentContent {
    let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
    let mut sink = HeadSink::default();

    match blobs.read_to(reference, &open.cmk, &mut sink) {
        Ok(()) => {}
        // The key is in the database and the ciphertext is not. Nobody decided
        // this; it is loss, and calling it shredding would file it as policy.
        Err(kokin_blob::BlobError::NotFound { .. }) => return DocumentContent::Lost,
        // Authentication or the content hash failed. Something modified the case
        // directory, which is the one thing in this enum worth alarming about.
        Err(
            e
            @ (kokin_blob::BlobError::Corrupt { .. } | kokin_blob::BlobError::HashMismatch { .. }),
        ) => {
            return DocumentContent::Damaged {
                detail: e.to_string(),
            }
        }
        // An I/O or key failure is about the machine or the case, not about this
        // document, and it is honest to report it as damage rather than to
        // invent a sixth state that says "we do not know".
        Err(e) => {
            return DocumentContent::Damaged {
                detail: e.to_string(),
            }
        }
    }

    let text = String::from_utf8_lossy(&sink.head);
    DocumentContent::Readable {
        lossy: matches!(text, std::borrow::Cow::Owned(_)),
        text: text.into_owned(),
        truncated_bytes: sink.discarded,
    }
}

/// A sink that keeps the head and counts the tail.
///
/// The tail still passes through the AEAD and the hasher on its way here, which
/// is the entire reason this is a sink and not a `get()` followed by a
/// truncation: a two-gigabyte artifact is verified without ever being held.
#[derive(Default)]
struct HeadSink {
    head: Vec<u8>,
    discarded: u64,
}

impl std::io::Write for HeadSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let room = MAX_DOCUMENT_BYTES.saturating_sub(self.head.len());
        let take = room.min(buf.len());
        self.head.extend_from_slice(&buf[..take]);
        self.discarded += (buf.len() - take) as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Evidence and lineage
// ---------------------------------------------------------------------------

/// The most artifacts one call will list.
const MAX_ARTIFACTS: usize = 200;

/// The most observations one call will list for an artifact.
const MAX_OBSERVATIONS: usize = 500;

/// Bytes of document either side of a quoted range.
///
/// Enough to see the markup a value sits inside, which is the difference between
/// "the page contains this email address" and "the page contains this email
/// address in a link labelled *press office*".
const QUOTE_CONTEXT_BYTES: u64 = 200;

/// The largest range this layer will quote.
///
/// Far above anything the HTML extractor produces — its values are capped at
/// 8 KiB — so reaching this means the locator is describing something other than
/// a token, and declining is better than a verdict that only holds one way. See
/// [`QuoteView::TooLarge`].
const MAX_QUOTE_BYTES: u64 = 256 * 1024;

/// Every artifact in the case, newest first, with how much of each was read.
pub fn artifacts(
    open: &OpenCase,
    limit: Option<usize>,
) -> Result<Vec<ArtifactRowView>, CommandError> {
    let limit = limit.unwrap_or(MAX_ARTIFACTS).min(MAX_ARTIFACTS);
    let mut stmt = open
        .conn
        .prepare("SELECT id FROM artifact ORDER BY created_utc DESC, rowid DESC LIMIT ?1")
        .map_err(storage)?;
    let rows = stmt
        .query_map([limit as i64], |r| r.get::<_, String>(0))
        .map_err(storage)?;

    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(storage)?);
    }

    // One pass per artifact rather than one query with joins, because the
    // extraction state is `kokin_search`'s to decide and duplicating its
    // latest-run window here is how two definitions of "read" come to exist.
    // The cost is bounded by MAX_ARTIFACTS and recorded as a limitation.
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(artifact_row(&open.conn, &id)?);
    }
    Ok(out)
}

/// What one artifact yielded, and what the run that read it says it missed.
pub fn artifact_observations(
    open: &OpenCase,
    artifact_id: &str,
    include_superseded: bool,
) -> Result<EvidenceListView, CommandError> {
    let artifact = artifact_row(&open.conn, artifact_id)?;

    let sql = "SELECT o.id, o.kind, o.value, o.observed_utc,
                      EXISTS (SELECT 1 FROM observation_supersession s
                               WHERE s.superseded_id = o.id)
                 FROM observation o
                WHERE o.artifact_id = ?1
                  AND (?2 = 1 OR NOT EXISTS (SELECT 1 FROM observation_supersession s
                                              WHERE s.superseded_id = o.id))
                ORDER BY o.kind, o.rowid
                LIMIT ?3";
    let mut stmt = open.conn.prepare(sql).map_err(storage)?;
    let rows = stmt
        .query_map(
            rusqlite::params![
                artifact_id,
                i64::from(include_superseded),
                MAX_OBSERVATIONS as i64
            ],
            |r| {
                Ok(ObservationRowView {
                    observation_id: r.get(0)?,
                    kind: r.get(1)?,
                    value: r.get(2)?,
                    observed_utc: r.get(3)?,
                    superseded: r.get::<_, i64>(4)? != 0,
                })
            },
        )
        .map_err(storage)?;

    let mut observations = Vec::new();
    for row in rows {
        observations.push(row.map_err(storage)?);
    }

    // Counted whether or not they were returned. A table that quietly hides
    // withdrawn rows and does not say how many it hid reports a document the
    // case has changed its mind about as one it never did.
    let superseded: i64 = open
        .conn
        .query_row(
            "SELECT COUNT(*) FROM observation o
              WHERE o.artifact_id = ?1
                AND EXISTS (SELECT 1 FROM observation_supersession s
                             WHERE s.superseded_id = o.id)",
            [artifact_id],
            |r| r.get(0),
        )
        .map_err(storage)?;

    Ok(EvidenceListView {
        artifact,
        observations,
        superseded,
        gaps: gaps(&open.conn, artifact_id)?,
    })
}

/// One observation, its lineage, and the bytes it cites.
pub fn observation(open: &OpenCase, observation_id: &str) -> Result<ObservationView, CommandError> {
    let (kind, value, locator_json, observed_utc, artifact_id, run_id): (
        String,
        String,
        String,
        String,
        String,
        String,
    ) = open
        .conn
        .query_row(
            "SELECT kind, value, locator_json, observed_utc, artifact_id, run_id
               FROM observation WHERE id = ?1",
            [observation_id],
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
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CommandError::new(
                ErrorCode::NotFound,
                format!("no observation {observation_id} in this case"),
            ),
            other => CommandError::new(ErrorCode::Storage, other.to_string()),
        })?;

    let artifact = artifact_row(&open.conn, &artifact_id)?;
    let locator = parse_locator(&locator_json);
    let quote = quote(open, &artifact, &locator, &value);

    Ok(ObservationView {
        observation_id: observation_id.to_string(),
        kind,
        value,
        observed_utc,
        superseded: supersession(&open.conn, observation_id)?,
        locator,
        quote,
        lineage: LineageView {
            extraction: run(&open.conn, &run_id)?,
            collection: collection(&open.conn, &artifact_id)?,
            also_collected: also_collected(&open.conn, &artifact)?,
            artifact,
        },
    })
}

fn artifact_row(conn: &Connection, artifact_id: &str) -> Result<ArtifactRowView, CommandError> {
    let (media_type, byte_length, content_hash, collected_utc): (String, i64, String, String) =
        conn.query_row(
            "SELECT media_type, byte_length, content_hash, created_utc
               FROM artifact WHERE id = ?1",
            [artifact_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => CommandError::new(
                ErrorCode::NotFound,
                format!("no artifact {artifact_id} in this case"),
            ),
            other => CommandError::new(ErrorCode::Storage, other.to_string()),
        })?;

    // Through the derivation edge, not through capture.content_hash: the same
    // bytes may have arrived several times, and matching on the hash would
    // attribute this artifact to whichever collection sorted first.
    let canonical_locator: Option<String> = conn
        .query_row(
            "SELECT s.canonical_locator
               FROM derivation d
               JOIN capture c ON c.id = d.input_id
               JOIN source s ON s.id = c.source_id
              WHERE d.output_kind = 'artifact' AND d.output_id = ?1
                AND d.input_kind = 'capture'
              ORDER BY c.requested_utc, c.rowid
              LIMIT 1",
            [artifact_id],
            |r| r.get(0),
        )
        .ok();

    let observations: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM observation o
              WHERE o.artifact_id = ?1
                AND NOT EXISTS (SELECT 1 FROM observation_supersession s
                                 WHERE s.superseded_id = o.id)",
            [artifact_id],
            |r| r.get(0),
        )
        .map_err(storage)?;

    let extraction = match kokin_search::coverage::artifact_coverage(conn, artifact_id)? {
        Some(c) => coverage_state_key(&c.state),
        // Unreachable: the artifact row was read three statements ago. Reported
        // rather than unwrapped, because "the artifact vanished mid-read" is a
        // storage failure and not a coverage answer.
        None => {
            return Err(CommandError::new(
                ErrorCode::Storage,
                format!("artifact {artifact_id} disappeared while being read"),
            ))
        }
    };

    Ok(ArtifactRowView {
        artifact_id: artifact_id.to_string(),
        media_type,
        byte_length,
        content_hash,
        collected_utc,
        canonical_locator,
        observations,
        extraction,
    })
}

/// The wire name of a coverage state.
///
/// A `match` rather than a `Debug` format, so renaming a variant in
/// `kokin_search` is a compile error here instead of a silently changed string
/// the interface branches on.
fn coverage_state_key(state: &kokin_search::coverage::CoverageState) -> String {
    use kokin_search::coverage::CoverageState as S;
    match state {
        S::NeverAttempted => "never_attempted",
        S::InProgress => "in_progress",
        S::Failed { .. } => "failed",
        S::Partial => "partial",
        S::Complete => "complete",
    }
    .to_string()
}

fn gaps(conn: &Connection, artifact_id: &str) -> Result<Vec<GapView>, CommandError> {
    Ok(
        kokin_search::coverage::gaps_for_artifact(conn, artifact_id)?
            .into_iter()
            .map(|g| GapView {
                gap_kind: g.gap_kind,
                detail: g.detail,
                magnitude: g.magnitude,
                unit: g.unit,
            })
            .collect(),
    )
}

fn supersession(
    conn: &Connection,
    observation_id: &str,
) -> Result<Option<SupersessionView>, CommandError> {
    let found = conn.query_row(
        "SELECT superseded_by, run_id, recorded_utc
           FROM observation_supersession WHERE superseded_id = ?1",
        [observation_id],
        |r| {
            Ok(SupersessionView {
                superseded_by: r.get(0)?,
                run_id: r.get(1)?,
                recorded_utc: r.get(2)?,
            })
        },
    );
    match found {
        Ok(view) => Ok(Some(view)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(other) => Err(storage(other)),
    }
}

fn run(conn: &Connection, run_id: &str) -> Result<RunView, CommandError> {
    conn.query_row(
        "SELECT id, transform_name, transform_version, code_version,
                started_utc, finished_utc, status, error_code, error_detail
           FROM transform_run WHERE id = ?1",
        [run_id],
        |r| {
            Ok(RunView {
                run_id: r.get(0)?,
                transform_name: r.get(1)?,
                transform_version: r.get(2)?,
                code_version: r.get(3)?,
                started_utc: r.get(4)?,
                finished_utc: r.get(5)?,
                status: r.get(6)?,
                error_code: r.get(7)?,
                error_detail: r.get(8)?,
            })
        },
    )
    .map_err(storage)
}

/// How one artifact was collected, walked back along the derivation edges.
fn collection(
    conn: &Connection,
    artifact_id: &str,
) -> Result<Option<CollectionView>, CommandError> {
    let found = conn.query_row(
        "SELECT d.run_id,
                c.id, c.requested_utc, c.http_status, c.capture_completeness,
                c.from_fixture, c.connector, c.job_id,
                s.id, s.kind, s.raw_locator, s.canonical_locator, s.first_seen_utc
           FROM derivation d
           JOIN capture c ON c.id = d.input_id
           JOIN source s ON s.id = c.source_id
          WHERE d.output_kind = 'artifact' AND d.output_id = ?1
            AND d.input_kind = 'capture'
          ORDER BY c.requested_utc, c.rowid
          LIMIT 1",
        [artifact_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                CaptureRefView {
                    capture_id: r.get(1)?,
                    requested_utc: r.get(2)?,
                    http_status: r.get(3)?,
                    capture_completeness: r.get(4)?,
                    from_fixture: r.get::<_, i64>(5)? != 0,
                    connector: r.get(6)?,
                    job_id: r.get(7)?,
                },
                SourceRefView {
                    source_id: r.get(8)?,
                    kind: r.get(9)?,
                    raw_locator: r.get(10)?,
                    canonical_locator: r.get(11)?,
                    first_seen_utc: r.get(12)?,
                },
            ))
        },
    );

    let (run_id, capture, source) = match found {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(other) => return Err(storage(other)),
    };

    Ok(Some(CollectionView {
        artifact_id: artifact_id.to_string(),
        ingest: Some(run(conn, &run_id)?),
        capture,
        source,
    }))
}

/// Every other artifact in this case holding the same bytes.
///
/// Deduplication stores one blob and keeps every arrival, so this is where "we
/// have seen this document before, somewhere else" is answerable. A panel that
/// showed only the artifact being read would state a single origin for evidence
/// the case knows arrived more than once.
fn also_collected(
    conn: &Connection,
    artifact: &ArtifactRowView,
) -> Result<Vec<CollectionView>, CommandError> {
    let mut stmt = conn
        .prepare(
            "SELECT id FROM artifact
              WHERE content_hash = ?1 AND id <> ?2
              ORDER BY created_utc, rowid",
        )
        .map_err(storage)?;
    let rows = stmt
        .query_map(
            rusqlite::params![artifact.content_hash, artifact.artifact_id],
            |r| r.get::<_, String>(0),
        )
        .map_err(storage)?;

    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(storage)?);
    }

    let mut out = Vec::new();
    for id in ids {
        if let Some(c) = collection(conn, &id)? {
            out.push(c);
        }
    }
    Ok(out)
}

/// Read a stored locator without trusting it to be anything in particular.
///
/// The column is `TEXT` and holds whatever wrote it, which today is one scheme
/// and tomorrow may be several. Anything unrecognised keeps its raw form and
/// loses its parsed fields, rather than being coerced into the one shape this
/// build knows — a page number read as a byte offset would quote confidently
/// from the wrong place.
fn parse_locator(raw: &str) -> LocatorView {
    let parsed: Option<serde_json::Value> = serde_json::from_str(raw).ok();
    let scheme = parsed
        .as_ref()
        .and_then(|v| v.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("unrecorded")
        .to_string();

    let field = |name: &str| -> Option<i64> {
        parsed
            .as_ref()
            .and_then(|v| v.get(name))
            .and_then(serde_json::Value::as_i64)
    };

    LocatorView {
        selector: parsed
            .as_ref()
            .and_then(|v| v.get("selector"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        start: field("start"),
        end: field("end"),
        scheme,
        raw_json: raw.to_string(),
    }
}

/// What the document says where the observation says to look.
///
/// The comparison happens over bytes the AEAD and the content hash have already
/// accepted, because the whole blob is streamed through the verifier on the way
/// to the window (A-037). A quote from bytes that failed authentication would be
/// the strongest-looking part of the panel and the least trustworthy thing in
/// the case.
fn quote(
    open: &OpenCase,
    artifact: &ArtifactRowView,
    locator: &LocatorView,
    value: &str,
) -> QuoteView {
    if locator.scheme != "html_byte_range" {
        return QuoteView::UnknownScheme {
            scheme: locator.scheme.clone(),
        };
    }
    let (Some(start), Some(end)) = (locator.start, locator.end) else {
        return QuoteView::UnknownScheme {
            scheme: locator.scheme.clone(),
        };
    };
    if start < 0 || end < start || end > artifact.byte_length {
        return QuoteView::OutOfRange {
            byte_length: artifact.byte_length,
            start,
            end,
        };
    }
    let span = (end - start) as u64;
    if span > MAX_QUOTE_BYTES {
        return QuoteView::TooLarge {
            bytes: end - start,
            limit: MAX_QUOTE_BYTES as i64,
        };
    }

    let reference = match kokin_store::blob_access(&open.conn, &artifact.content_hash) {
        Ok(kokin_store::BlobAccess::Readable(reference)) => reference,
        Ok(kokin_store::BlobAccess::Shredded { .. }) => {
            return QuoteView::Unavailable {
                document_state: "shredded".to_string(),
            }
        }
        Ok(kokin_store::BlobAccess::Unrecorded) => {
            return QuoteView::Unavailable {
                document_state: "unrecorded".to_string(),
            }
        }
        Err(_) => {
            return QuoteView::Unavailable {
                document_state: "unrecorded".to_string(),
            }
        }
    };

    let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
    let mut sink = WindowSink {
        from: (start as u64).saturating_sub(QUOTE_CONTEXT_BYTES),
        quote_from: start as u64,
        quote_to: end as u64,
        to: (end as u64).saturating_add(QUOTE_CONTEXT_BYTES),
        ..WindowSink::default()
    };

    // The same five states artifact_document reports, reached the same way, so a
    // quote and the document it came from never disagree about whether the
    // bytes are there.
    if let Err(e) = blobs.read_to(&reference, &open.cmk, &mut sink) {
        return QuoteView::Unavailable {
            document_state: match e {
                kokin_blob::BlobError::NotFound { .. } => "lost",
                _ => "damaged",
            }
            .to_string(),
        };
    }

    // Each segment is decoded on its own, so a multi-byte character the locator
    // cuts in half shows as a replacement at the boundary. That is the honest
    // rendering: the locator really does end mid-character.
    let (before, l1) = lossy(&sink.before);
    let (quoted, l2) = lossy(&sink.quoted);
    let (after, l3) = lossy(&sink.after);
    let excerpt = ExcerptView {
        before,
        quoted,
        after,
        lossy: l1 || l2 || l3,
    };

    if sink.quoted == value.as_bytes() {
        QuoteView::Exact { excerpt }
    } else if excerpt.quoted.contains(value) {
        QuoteView::Contains { excerpt }
    } else {
        QuoteView::Differs { excerpt }
    }
}

fn lossy(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    let was_lossy = matches!(text, std::borrow::Cow::Owned(_));
    (text.into_owned(), was_lossy)
}

/// A sink that keeps one window of a document and discards the rest.
///
/// Streaming rather than reading the blob and slicing it, for the reason
/// [`HeadSink`] exists: every byte still passes through the AEAD and the hasher,
/// so a two-gigabyte artifact yields a two-hundred-byte quote from a document
/// that was verified in full.
#[derive(Default)]
struct WindowSink {
    from: u64,
    quote_from: u64,
    quote_to: u64,
    to: u64,
    pos: u64,
    before: Vec<u8>,
    quoted: Vec<u8>,
    after: Vec<u8>,
}

impl std::io::Write for WindowSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let chunk_start = self.pos;
        for (i, byte) in buf.iter().enumerate() {
            let at = chunk_start + i as u64;
            if at >= self.to {
                break;
            }
            if at < self.from {
                continue;
            }
            if at < self.quote_from {
                self.before.push(*byte);
            } else if at < self.quote_to {
                self.quoted.push(*byte);
            } else {
                self.after.push(*byte);
            }
        }
        self.pos += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// Read an entity as a person rather than as a row.
///
/// Asking for a row that has been merged away returns the entity it resolves to
/// and sets `redirected`, so an interface can say that it followed the merge.
/// Everything gathered below spans the cluster, because a merge does not move
/// rows: the absorbed record keeps its identifiers, its evidence and its
/// assessments, and a read that only looked at the survivor would drop them.
pub fn entity(open: &OpenCase, entity_id: &str) -> Result<EntityView, CommandError> {
    let conn = &open.conn;
    let canonical = kokin_graph::resolution::canonical_id(conn, entity_id)?;

    let (type_key, display_name, notes) = entity_row(conn, &canonical).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => CommandError::new(
            ErrorCode::NotFound,
            format!("no entity {entity_id} in this case"),
        ),
        other => CommandError::new(ErrorCode::Storage, other.to_string()),
    })?;

    let absorbed = kokin_graph::resolution::cluster(conn, &canonical)?;
    let mut members = absorbed.clone();
    members.push(canonical.clone());
    members.sort_unstable();

    let mut merged_from = Vec::new();
    for id in &absorbed {
        let (type_key, display_name, _) = entity_row(conn, id)
            .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;
        merged_from.push(EntityRefView {
            entity_id: id.clone(),
            display_name,
            type_key,
        });
    }

    let evidence = kokin_graph::resolution::evidence_for_cluster(conn, &canonical)?
        .into_iter()
        .map(|e| EvidenceView {
            link_id: e.evidence.link_id,
            entity_id: e.entity_id,
            evidence_kind: e.evidence.evidence_kind,
            evidence_id: e.evidence.evidence_id,
            role: e.evidence.role,
        })
        .collect();

    Ok(EntityView {
        requested_id: entity_id.to_string(),
        redirected: canonical != entity_id,
        entity_id: canonical.clone(),
        type_key,
        display_name,
        notes,
        merged_from,
        identifiers: identifiers(conn, &members)?,
        evidence,
        confidence: confidence(conn, &members)?,
        history: history(conn, &canonical)?,
    })
}

/// The most entities one call will list.
const MAX_ENTITIES: usize = 500;

/// Every entity in the case, canonical rows only.
///
/// Listing merged-away rows would put both halves of a merge on screen as two
/// people, which is the thing an analyst already decided is not true. They are
/// reachable from the entity they resolve to, where the merge is visible as a
/// merge.
pub fn entities(open: &OpenCase, limit: Option<usize>) -> Result<Vec<EntityRowView>, CommandError> {
    let limit = limit.unwrap_or(MAX_ENTITIES).min(MAX_ENTITIES);
    let conn = &open.conn;

    let mut stmt = conn
        .prepare(
            "SELECT e.id, e.type_key, e.display_name
               FROM entity e
              WHERE NOT EXISTS (SELECT 1 FROM er_merge_map m
                                 WHERE m.entity_id = e.id AND m.active = 1)
              ORDER BY e.display_name, e.rowid
              LIMIT ?1",
        )
        .map_err(storage)?;
    let rows = stmt
        .query_map([limit as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(storage)?;

    let mut found = Vec::new();
    for row in rows {
        found.push(row.map_err(storage)?);
    }

    let total_dimensions = case_scales(conn)?.len() as i64;
    let mut out = Vec::with_capacity(found.len());
    for (entity_id, type_key, display_name) in found {
        // Every count spans the cluster, for the reason `entity` gathers across
        // it: a merge does not move rows, so counting only the survivor's would
        // report the absorbed record's identifiers and evidence as absent at
        // exactly the moment they became most relevant.
        let absorbed = kokin_graph::resolution::cluster(conn, &entity_id)?;
        let mut members = absorbed;
        let merged_count = members.len() as i64;
        members.push(entity_id.clone());

        out.push(EntityRowView {
            merged_count,
            identifier_count: count_over(conn, "identifier", "entity_id", &members)?,
            evidence_count: count_over(conn, "evidence_link", "subject_id", &members)?,
            assessed_dimensions: assessed_dimensions(conn, &members)?,
            total_dimensions,
            entity_id,
            type_key,
            display_name,
        });
    }
    Ok(out)
}

/// Rows in `table` whose `column` is any of `members`.
fn count_over(
    conn: &Connection,
    table: &str,
    column: &str,
    members: &[String],
) -> Result<i64, CommandError> {
    // `table` and `column` are literals from this module, never from a caller;
    // only the member ids are bound, and they are ids this module just read.
    let sql = format!(
        "SELECT COUNT(*) FROM {table} WHERE {column} IN ({})",
        placeholders(members.len())
    );
    conn.query_row(&sql, rusqlite::params_from_iter(members.iter()), |r| {
        r.get(0)
    })
    .map_err(storage)
}

/// How many distinct dimensions anybody has assessed across the cluster.
///
/// A count of *whether somebody looked*, never of what they concluded. Nothing
/// downstream can turn it into a confidence, because it does not know one
/// (A-033).
fn assessed_dimensions(conn: &Connection, members: &[String]) -> Result<i64, CommandError> {
    let sql = format!(
        "SELECT COUNT(DISTINCT dimension) FROM assessment
          WHERE subject_kind = 'entity' AND subject_id IN ({})",
        placeholders(members.len())
    );
    conn.query_row(&sql, rusqlite::params_from_iter(members.iter()), |r| {
        r.get(0)
    })
    .map_err(storage)
}

fn entity_row(conn: &Connection, id: &str) -> rusqlite::Result<(String, String, String)> {
    conn.query_row(
        "SELECT type_key, display_name, notes FROM entity WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
}

/// Every identifier on every row in the cluster.
///
/// Ordered by namespace then value rather than by insertion, because this is a
/// list somebody scans for a specific thing. The tie-break is `rowid`, since
/// `created_utc` is second-resolution and an entity's identifiers are commonly
/// written in one loop inside one transaction (D-022).
fn identifiers(conn: &Connection, members: &[String]) -> Result<Vec<IdentifierView>, CommandError> {
    let sql = format!(
        "SELECT id, entity_id, namespace, value
           FROM identifier
          WHERE entity_id IN ({})
          ORDER BY namespace, value, rowid",
        placeholders(members.len())
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(members.iter()), |r| {
            Ok(IdentifierView {
                identifier_id: r.get(0)?,
                entity_id: r.get(1)?,
                namespace: r.get(2)?,
                value: r.get(3)?,
            })
        })
        .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?);
    }
    Ok(out)
}

/// All seven dimensions, whether or not anybody has assessed them.
///
/// The scales come from the *case*, not from this build. A case is
/// self-describing on purpose: an assessment made under version 1 of a scale
/// still renders under its own labels when opened by a build shipping version 2,
/// and a build that has never heard of a dimension the case holds still shows
/// it. Reading `kokin_graph::scales()` here instead would quietly make the
/// binary the authority on what a case is allowed to say.
fn confidence(conn: &Connection, members: &[String]) -> Result<Vec<DimensionView>, CommandError> {
    let dimensions = case_scales(conn)?;
    let mut assessed: BTreeMap<String, Vec<DimensionValueView>> = BTreeMap::new();

    for id in members {
        for a in kokin_graph::assessments_for(
            conn,
            kokin_graph::Subject {
                kind: kokin_graph::SubjectKind::Entity,
                id,
            },
        )? {
            let label = value_label(conn, &a.dimension, a.scale_version, &a.value_key)?;
            assessed
                .entry(a.dimension.clone())
                .or_default()
                .push(DimensionValueView {
                    entity_id: id.clone(),
                    value_key: a.value_key,
                    label,
                    scale_version: a.scale_version,
                    actor: a.actor,
                    assessed_utc: a.assessed_utc,
                    contributing_factors_json: a.contributing_factors_json,
                });
        }
    }

    Ok(dimensions
        .into_iter()
        .map(|(dimension, title, question)| {
            let values = assessed.remove(&dimension).unwrap_or_default();
            let agreement = match values.len() {
                // Unassessed. Reported as a state rather than as an absence, so
                // an interface has something to render and cannot leave the
                // dimension out of the panel entirely.
                0 => "unassessed",
                1 => "assessed",
                // Rows in this cluster were assessed differently and nobody has
                // reconciled them. Not resolved here: choosing a winner is a
                // composite score with a smaller denominator.
                _ if values.iter().all(|v| v.value_key == values[0].value_key) => "assessed",
                _ => "disagreed",
            };
            DimensionView {
                dimension,
                title,
                question,
                agreement: agreement.to_string(),
                values,
            }
        })
        .collect())
}

/// The dimensions this case knows about, at their newest version each.
fn case_scales(conn: &Connection) -> Result<Vec<(String, String, String)>, CommandError> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.question
               FROM scale s
              WHERE s.version = (SELECT MAX(version) FROM scale WHERE id = s.id)
              ORDER BY s.id",
        )
        .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?);
    }
    Ok(out)
}

/// The label the assessment's *own* scale version gives its value.
fn value_label(
    conn: &Connection,
    dimension: &str,
    version: i64,
    value_key: &str,
) -> Result<String, CommandError> {
    conn.query_row(
        "SELECT label FROM scale_value
          WHERE scale_id = ?1 AND scale_version = ?2 AND value_key = ?3",
        rusqlite::params![dimension, version, value_key],
        |r| r.get(0),
    )
    .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))
}

fn history(conn: &Connection, entity_id: &str) -> Result<Vec<DecisionView>, CommandError> {
    Ok(kokin_graph::resolution::history(conn, entity_id)?
        .into_iter()
        .map(|(action, actor, rationale, decided_utc)| DecisionView {
            action,
            actor,
            rationale,
            decided_utc,
        })
        .collect())
}

/// `?1,?2,…` for an `IN` clause. The count comes from a `Vec` this module built,
/// never from a caller.
fn placeholders(n: usize) -> String {
    (1..=n)
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",")
}
