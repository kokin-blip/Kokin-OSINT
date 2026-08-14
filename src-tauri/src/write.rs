//! The writes: what the interface may put into a case, and under whose name.
//!
//! # The actor is not an input
//!
//! Every write in this product records who made it. `kokin_graph` refuses `rule:`
//! and `ai:` actors the decisions only a person may make (ADR-0008), migration 7
//! refuses them again in a `CHECK` constraint — and the module note in
//! `kokin_graph::resolution` says exactly where that ends:
//!
//! > What neither can catch is automation writing `actor = "user:local"`. That is
//! > a lie rather than a bypass, no schema can detect it, and the audit chain is
//! > where it is answerable.
//!
//! This is the layer where that lie stops being hypothetical, because a request
//! field is written by the webview — the least trusted thing in this process, and
//! the one that renders hostile HTML (A-031). Had `actor` been a parameter, every
//! guarantee above would have reduced to a naming convention the caller chooses to
//! honour, and the audit chain would record a person where a machine acted: intact
//! and false, which is worse than absent.
//!
//! So [`ACTOR`] is a constant, no request type carries an `actor` field, and every
//! write request is `deny_unknown_fields` so a caller cannot smuggle one in
//! (D-032). A command arriving here *is* a person: something happened in a window
//! a human is looking at. When the rules engine and the model come, they call
//! `kokin_graph` in-process under their own names — never through here.
//!
//! The corollary is that `MachineMayNotAct` is unreachable from this crate. That
//! is a stronger statement than mapping it well, and it is why the mapping in
//! [`crate::error`] says so out loud rather than quietly never firing.
//!
//! # A judgement with no stated reason is not recorded
//!
//! `kokin_graph` accepts an empty rationale and an empty factor list; the column
//! defaults to `[]`. This layer refuses both (D-033). ADR-0007's "why" panel is
//! the entire mechanism that forces reasoning to be written down, and a merge
//! whose rationale is blank cannot be reconsidered six weeks later — which is the
//! only reason ADR-0008 went to the trouble of making merges reversible.

use rusqlite::Connection;

use kokin_graph::{EvidenceKind, EvidenceRole, Grounding, NewEntity, Subject, SubjectKind};
use kokin_store::OpenCase;

use crate::error::{CommandError, ErrorCode};
use crate::views::{
    AssessRequest, ExtractRequest, ExtractView, GroundingInput, IngestFileRequest, IngestView,
    MergeRequest, NewEntityRequest, NewIdentifierRequest, NewRelationshipRequest, RejectRequest,
    ResolutionView, SplitRequest, WriteView,
};

/// Who a command records as the author of a write.
///
/// One constant, not a parameter. See the module note — this is the control, and
/// `a_request_cannot_name_its_own_actor` is what holds it in place.
///
/// It says `local` rather than a name because a case does not yet know which
/// human is at the keyboard. That is a known limitation and a real one: it means
/// two analysts sharing a machine are indistinguishable in the audit chain. It is
/// not a reason to accept the name from the webview, which would make *every*
/// actor unverifiable rather than merely coarse.
pub const ACTOR: &str = "user:local";

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

/// Import a local file and record it as provenance.
pub fn ingest_file(
    open: &mut OpenCase,
    request: &IngestFileRequest,
) -> Result<IngestView, CommandError> {
    let path = std::path::Path::new(&request.path);
    refuse_the_case_itself(open, path)?;

    let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
    let ingested = kokin_ingest::ingest_file(
        &mut open.conn,
        &blobs,
        &open.cmk,
        path,
        &kokin_ingest::IngestContext {
            connector: "file_import",
            // Every capture is attributable to something. A file import has no
            // job queue behind it yet, so it is attributed to the action that
            // caused it, which is the honest answer and not a placeholder.
            job_id: "user_action",
        },
    )?;

    Ok(IngestView {
        run_id: ingested.run_id,
        source_id: ingested.source_id,
        capture_id: ingested.capture_id,
        artifact_id: ingested.artifact_id,
        content_hash: ingested.content_hash,
        size_bytes: ingested.size_bytes,
        deduplicated: ingested.deduplicated,
        from_fixture: ingested.from_fixture,
    })
}

/// A case may not ingest its own storage.
///
/// Narrow on purpose, and **not** the mitigation for A-034. Ingest is the one
/// command that reads outside the case directory, and the real control is that
/// the path comes from a native file picker the webview cannot forge — which
/// needs a dialog this application does not have yet. What this check does cover
/// is a case swallowing its own database or blob store, which grows without bound
/// and encrypts a copy of the case inside itself.
fn refuse_the_case_itself(open: &OpenCase, path: &std::path::Path) -> Result<(), CommandError> {
    // Canonicalised because `case/../case/db.sqlite` is the same file, and a
    // string comparison would not say so. A path that cannot be canonicalised
    // does not exist, and ingest will report that far more usefully than this.
    let (Ok(target), Ok(root)) = (path.canonicalize(), open.paths.root.canonicalize()) else {
        return Ok(());
    };
    if target.starts_with(&root) {
        return Err(CommandError::new(
            ErrorCode::FileUnreadable,
            "that file is inside the open case's own directory; a case cannot ingest itself",
        ));
    }
    Ok(())
}

/// Read an artifact already in the case and record what it says.
pub fn extract(open: &mut OpenCase, request: &ExtractRequest) -> Result<ExtractView, CommandError> {
    let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
    let extracted = kokin_extract::extract_artifact(
        &mut open.conn,
        &blobs,
        &open.cmk,
        &request.artifact_id,
        kokin_extract::Limits::default(),
    )?;

    Ok(ExtractView {
        run_id: extracted.run_id,
        artifact_id: extracted.artifact_id,
        observations: extracted.observation_ids.len(),
        skipped_oversize: extracted.skipped_oversize,
        text_truncated_bytes: extracted.text_truncated_bytes,
        superseded: extracted.superseded,
        complete: extracted.skipped_oversize == 0 && extracted.text_truncated_bytes == 0,
    })
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Create an entity, grounded in the evidence given.
pub fn create_entity(
    open: &mut OpenCase,
    request: &NewEntityRequest,
) -> Result<WriteView, CommandError> {
    let evidence = grounding(&request.evidence)?;
    let id = kokin_graph::create_entity(
        &mut open.conn,
        NewEntity {
            type_key: &request.type_key,
            display_name: &request.display_name,
            notes: &request.notes,
        },
        &evidence,
    )?;
    Ok(wrote(id))
}

/// Attach an identifier to an entity, grounded in evidence.
pub fn add_identifier(
    open: &mut OpenCase,
    request: &NewIdentifierRequest,
) -> Result<WriteView, CommandError> {
    let evidence = grounding(&request.evidence)?;
    let id = kokin_graph::add_identifier(
        &mut open.conn,
        &request.entity_id,
        &request.namespace,
        &request.value,
        &evidence,
    )?;
    Ok(wrote(id))
}

/// Relate two entities, grounded in evidence.
pub fn relate(
    open: &mut OpenCase,
    request: &NewRelationshipRequest,
) -> Result<WriteView, CommandError> {
    let evidence = grounding(&request.evidence)?;
    let id = kokin_graph::relate(
        &mut open.conn,
        &request.from_entity,
        &request.to_entity,
        &request.kind,
        (request.started_utc.as_deref(), request.ended_utc.as_deref()),
        &evidence,
    )?;
    Ok(wrote(id))
}

/// Record one dimension of confidence about one subject.
pub fn assess(open: &mut OpenCase, request: &AssessRequest) -> Result<WriteView, CommandError> {
    let factors = explanation(&request.contributing_factors, "an assessment")?;
    // Serialised here rather than accepted as a string, so the column always
    // holds well-formed JSON of a known shape. A caller handing over raw text
    // would be writing into a field the "why" panel reads as structure.
    let contributing_factors_json = serde_json::to_string(&factors).map_err(|e| {
        CommandError::new(
            ErrorCode::Internal,
            format!("factors are not encodable: {e}"),
        )
    })?;

    let id = kokin_graph::assess(
        &mut open.conn,
        Subject {
            kind: subject_kind(&request.subject_kind)?,
            id: &request.subject_id,
        },
        &request.dimension,
        &request.value_key,
        &contributing_factors_json,
        ACTOR,
    )?;
    Ok(wrote(id))
}

/// Record that two entities are the same, and project it.
pub fn merge(open: &mut OpenCase, request: &MergeRequest) -> Result<ResolutionView, CommandError> {
    let rationale = rationale(&request.rationale, "a merge")?;
    let decision_id = kokin_graph::resolution::merge(
        &mut open.conn,
        &request.left,
        &request.right,
        ACTOR,
        rationale,
        request.candidate_id.as_deref(),
    )?;
    resolved(&open.conn, decision_id, &request.left)
}

/// Record that two entities are *not* the same.
///
/// Open to automated actors in `kokin_graph`, and still written under [`ACTOR`]
/// here — this command is a person clicking, whatever else may also be allowed to
/// call the crate underneath.
pub fn reject(open: &mut OpenCase, request: &RejectRequest) -> Result<WriteView, CommandError> {
    let rationale = rationale(&request.rationale, "a rejection")?;
    let id = kokin_graph::resolution::reject(
        &mut open.conn,
        &request.left,
        &request.right,
        ACTOR,
        rationale,
        request.candidate_id.as_deref(),
    )?;
    Ok(wrote(id))
}

/// Take an entity back out of its cluster.
pub fn split(open: &mut OpenCase, request: &SplitRequest) -> Result<ResolutionView, CommandError> {
    let rationale = rationale(&request.rationale, "a split")?;
    let decision_id =
        kokin_graph::resolution::split(&mut open.conn, &request.entity, ACTOR, rationale)?;
    resolved(&open.conn, decision_id, &request.entity)
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn wrote(id: String) -> WriteView {
    WriteView {
        id,
        actor: ACTOR.to_string(),
    }
}

/// Where an entity ended up, read back rather than assumed.
///
/// A merge picks its survivor by age inside `kokin_graph`, and a split returns an
/// entity to itself. Neither is something this layer should reimplement to fill in
/// a response field — identity is a read (ADR-0008), so this reads it.
fn resolved(
    conn: &Connection,
    decision_id: String,
    entity: &str,
) -> Result<ResolutionView, CommandError> {
    Ok(ResolutionView {
        decision_id,
        canonical_id: kokin_graph::resolution::canonical_id(conn, entity)?,
        actor: ACTOR.to_string(),
    })
}

/// Translate the wire form of evidence, refusing anything not on the list.
///
/// The empty check is here as well as in `kokin_graph` and in the trigger. It is
/// not redundant: this is the one of the three that can name the field the caller
/// left out.
fn grounding(input: &[GroundingInput]) -> Result<Vec<Grounding<'_>>, CommandError> {
    if input.is_empty() {
        return Err(CommandError::new(
            ErrorCode::EvidenceRequired,
            "this write must cite at least one observation, artifact or capture",
        ));
    }

    input
        .iter()
        .map(|g| {
            Ok(Grounding {
                kind: match g.kind.as_str() {
                    "observation" => EvidenceKind::Observation,
                    "artifact" => EvidenceKind::Artifact,
                    "capture" => EvidenceKind::Capture,
                    other => {
                        return Err(CommandError::new(
                            ErrorCode::InvalidValue,
                            format!(
                                "{other} is not a kind of evidence; \
                                 an analytical row is never evidence for another"
                            ),
                        ))
                    }
                },
                id: g.id.as_str(),
                role: match g.role.as_str() {
                    "supports" => EvidenceRole::Supports,
                    "contradicts" => EvidenceRole::Contradicts,
                    "context" => EvidenceRole::Context,
                    other => {
                        return Err(CommandError::new(
                            ErrorCode::InvalidValue,
                            format!("{other} is not a role evidence can take"),
                        ))
                    }
                },
            })
        })
        .collect()
}

fn subject_kind(kind: &str) -> Result<SubjectKind, CommandError> {
    match kind {
        "entity" => Ok(SubjectKind::Entity),
        "identifier" => Ok(SubjectKind::Identifier),
        "relationship" => Ok(SubjectKind::Relationship),
        other => Err(CommandError::new(
            ErrorCode::InvalidValue,
            format!("{other} is not something this product assesses"),
        )),
    }
}

/// A judgement must say what it rests on (D-033).
fn explanation<'a>(factors: &'a [String], what: &str) -> Result<Vec<&'a str>, CommandError> {
    let kept: Vec<&str> = factors
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .collect();
    if kept.is_empty() {
        return Err(CommandError::new(
            ErrorCode::ExplanationRequired,
            format!("{what} must record what it is based on"),
        ));
    }
    Ok(kept)
}

/// The same rule, for the decisions that carry prose rather than a list.
fn rationale<'a>(rationale: &'a str, what: &str) -> Result<&'a str, CommandError> {
    let trimmed = rationale.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new(
            ErrorCode::ExplanationRequired,
            format!("{what} must record why"),
        ));
    }
    Ok(trimmed)
}
