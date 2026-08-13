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
    CoverageView, DecisionView, DimensionValueView, DimensionView, DocumentContent, DocumentView,
    EntityRefView, EntityView, EvidenceView, HitView, IdentifierView, SearchRequest, SearchView,
};

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
