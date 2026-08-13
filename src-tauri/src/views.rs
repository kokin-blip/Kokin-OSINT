//! Every type that crosses the IPC boundary, in either direction.
//!
//! # Why they are all in one file
//!
//! The webview is the least trusted part of this process, and `OpenCase` holds
//! the case master key — which blob filenames are keyed with, so a leak does not
//! merely decrypt a case, it locates it (A-029). The structural guarantee is
//! that the domain crates take no `serde` dependency: `OpenCase`, `CaseHeader`
//! and `CaseMasterKey` *cannot* be serialised, because the crates defining them
//! do not know what `Serialize` is.
//!
//! That guarantee only stays legible if the types which *do* serialise are
//! somewhere a person can read in one sitting. So they live here, and
//! `serialize_is_derived_only_where_the_boundary_is_declared` fails if a
//! `derive(Serialize)` or `derive(Deserialize)` appears anywhere else in this
//! crate — including in a module added later by someone who has not read this
//! note.
//!
//! # Two things this file deliberately does not carry
//!
//! **A relevance score.** `kokin_search::Hit` has one; [`HitView`] does not
//! (D-030). It is a BM25 figure where lower is better and the scale depends on
//! the corpus, so the only thing it can honestly do is order the list — which
//! it has already done by the time these values are built.
//!
//! **A composite confidence.** ADR-0007 has seven dimensions and no total, and
//! this is the layer where a total would get invented, because a total is what
//! sorts and colours conveniently. [`DimensionView`] carries value *keys* on
//! named ordinal scales; there is nothing here to average, and
//! `the_confidence_block_carries_nothing_that_could_be_added_up` asserts it.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// The case itself
// ---------------------------------------------------------------------------

/// What the interface may know about an open case.
///
/// Everything here is already visible to whoever unlocked the case. There is no
/// key material, no derived key material, and no handle that could be used to
/// reach any: commands find the case through the session, never through a value
/// the interface hands back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseView {
    pub case_id: String,
    /// Displayed in the title bar, so the analyst can see which case this is.
    pub root: String,
    pub schema_version: i64,
    /// Whether the case can still be opened with a recovery key. False means a
    /// forgotten passphrase is permanent, which the interface should say out
    /// loud rather than leave to be discovered.
    pub has_recovery_key: bool,
}

/// A newly created case, and the one thing that will never be shown again.
///
/// `recovery_key` is the single value in this entire layer that carries key
/// material across the IPC boundary, and it does so exactly once (D-026). It is
/// not stored in usable form anywhere — only its wrap of the case master key is
/// — so if the interface does not put it in front of the user now, it is gone
/// and a forgotten passphrase becomes permanent.
///
/// There is deliberately no command that returns it later. A "show me my
/// recovery key again" command would have to either keep it in memory for the
/// session or re-derive it, and neither is a thing this design permits.
#[derive(Debug, Clone, Serialize)]
pub struct NewCaseView {
    pub case: CaseView,
    pub recovery_key: String,
}

/// Whether anything is open, for an interface deciding what to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionView {
    pub case: Option<CaseView>,
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// What the interface asks for.
///
/// Every field has a default, so a caller that wants "search for this" writes
/// exactly that and inherits the defaults argued for in `kokin_search`— in
/// particular `resolve_merged`, which is on because listing two rows an analyst
/// has already decided are one person asserts that two people exist.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub limit: Option<usize>,
    pub offset: usize,
    /// Restrict to these subject kinds. Empty means all.
    pub kinds: Vec<String>,
    /// Restrict to these facets. Empty means all.
    pub facets: Vec<String>,
    /// Include observations a rerun has withdrawn.
    pub include_superseded: bool,
    /// Set false to see entity rows rather than resolved entities.
    pub resolve_merged: Option<bool>,
    /// Treat the last word as a prefix, for a box the user is still typing in.
    pub typeahead: bool,
}

/// One search result.
///
/// No relevance score: see the module note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HitView {
    /// `observation`, `entity`, `identifier` or `relationship` — which read
    /// command this result opens into.
    pub subject_kind: String,
    pub subject_id: String,
    pub title: String,
    /// The matching text in context, with matches bracketed.
    pub snippet: String,
    /// The row's own type: observation kind, entity type, identifier namespace.
    pub facet: String,
    /// True if an extractor rerun withdrew this observation (ADR-0006). Only
    /// ever set when the caller asked for withdrawn results, and an interface
    /// showing one without saying so has the case asserting something it has
    /// retracted.
    pub superseded: bool,
    /// Set when this hit is an entity that has been merged into another, and
    /// holds the entity it now resolves to (ADR-0008). The hit still reports its
    /// own `subject_id`, because that is the row the text matched.
    pub canonical_id: Option<String>,
}

/// How much of this case a search can reach.
///
/// `incomplete` and `is_complete` are computed here rather than left to the
/// interface. They are the two figures a caveat is written from, and a caller
/// that has to derive them is a caller that can derive them wrong — in the
/// direction of a case looking better read than it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoverageView {
    pub artifacts: i64,
    pub never_attempted: i64,
    pub in_progress: i64,
    pub failed: i64,
    pub partial: i64,
    pub complete: i64,
    pub incomplete: i64,
    pub is_complete: bool,
}

/// Results and the caveat that belongs with them, in one payload.
///
/// `coverage` is not an `Option` and there is no command that returns hits
/// without it (D-029). Coverage cannot be query-scoped — deciding whether an
/// unread document matches a query means reading it — so the honest statement is
/// the case-scoped one, and it has to travel with every result list rather than
/// only with the empty ones. Forty hits from a case that is 60% read is more
/// misleading than none, because nobody interrogates a full page of results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchView {
    pub hits: Vec<HitView>,
    /// Matching rows in the whole case, so a clamped `limit` is visibly a page
    /// rather than an answer.
    pub total: i64,
    pub coverage: CoverageView,
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

/// An artifact, and whatever the case can still show of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocumentView {
    pub artifact_id: String,
    pub media_type: String,
    /// What the document weighed when it was ingested. Present in every state,
    /// including the ones where the bytes are gone: it is provenance, and
    /// outliving the evidence is the point of provenance.
    pub byte_length: i64,
    pub content_hash: String,
    pub content: DocumentContent,
}

/// Why the interface can or cannot show a document, as five answers.
///
/// `kokin_store::blob_access` returns three, and deliberately stops there: the
/// remaining two need I/O to tell apart. This is the layer that does the I/O, so
/// it is the layer where they become distinguishable — and the difference
/// between them is the difference between a retention policy working and a case
/// being damaged.
///
/// Rendered as `{ "state": "shredded", ... }` so an interface branches on a
/// stable string rather than on the shape of the object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DocumentContent {
    /// The bytes are here and every one of them authenticated.
    ///
    /// `text` may be an excerpt — see `truncated_bytes` — but the hash was
    /// verified over the *whole* document before this was returned, so an
    /// excerpt is a short view of a document known to be intact, not an
    /// unverified prefix.
    Readable {
        text: String,
        /// Bytes past the excerpt limit. Non-zero means the interface is showing
        /// part of a document and must say so.
        truncated_bytes: u64,
        /// True if the bytes were not valid UTF-8 and were replaced lossily.
        /// A binary artifact shown as text is a rendering, not the evidence.
        lossy: bool,
    },
    /// The wrapped key was destroyed on purpose. The case is working correctly;
    /// this document is not coming back, and that was a decision somebody made.
    Shredded { shredded_utc: Option<String> },
    /// The case has no `blob` row for this hash at all. Not a policy outcome —
    /// an artifact referring to provenance the case does not have is a broken
    /// case.
    Unrecorded,
    /// The key is here and the ciphertext is not. Nobody shredded this; the file
    /// is gone from the case directory. Reporting it as `Shredded` would file
    /// data loss as retention policy at the exact moment somebody is looking at
    /// it.
    Lost,
    /// The ciphertext is here and failed authentication, or did not hash to what
    /// the case recorded. Someone or something modified the case directory
    /// (A-031), and this is the only state in this enum that is an accusation.
    Damaged { detail: String },
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

/// An entity as an analyst reads it: the person, not the row.
///
/// Identity is a read, not a property of a row (ADR-0008), so asking for a
/// merged-away entity returns the one it resolves to — and says that it did.
/// `requested_id` and `redirected` exist so an interface can show "you asked for
/// X, which was merged into this" rather than silently substituting, which is
/// how an analyst loses track of which of two records a name came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityView {
    /// What the caller asked for.
    pub requested_id: String,
    /// What is actually described below: the canonical entity.
    pub entity_id: String,
    pub redirected: bool,
    pub type_key: String,
    pub display_name: String,
    pub notes: String,
    /// The rows merged into this one, still named, because "which record did
    /// that come from" is the question a merge makes hard to ask.
    pub merged_from: Vec<EntityRefView>,
    /// Across the whole cluster, each carrying the row it hangs on.
    pub identifiers: Vec<IdentifierView>,
    /// Across the whole cluster, chronologically. A merge does not move
    /// evidence, so reading only the survivor's would quietly omit the absorbed
    /// record's — the case's strongest material about that person going missing
    /// at exactly the moment they were identified.
    pub evidence: Vec<EvidenceView>,
    /// All seven dimensions, always, assessed or not. See [`DimensionView`].
    pub confidence: Vec<DimensionView>,
    /// Every merge, split and rejection touching this entity, newest first.
    pub history: Vec<DecisionView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityRefView {
    pub entity_id: String,
    pub display_name: String,
    pub type_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdentifierView {
    pub identifier_id: String,
    /// The entity row it is attached to, which after a merge is often not the
    /// canonical one.
    pub entity_id: String,
    pub namespace: String,
    /// What the evidence said.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceView {
    pub link_id: String,
    /// The entity row this link hangs on.
    pub entity_id: String,
    pub evidence_kind: String,
    pub evidence_id: String,
    /// `supports`, `contradicts` or `context`. Evidence arguing *against* a
    /// conclusion has somewhere to live, and an interface that renders all three
    /// the same has thrown that away.
    pub role: String,
}

/// One confidence dimension, whether or not anybody has assessed it.
///
/// **All seven are always present** (D-031). A sparse list renders as absence,
/// and absence reads as "no concern" — which is the opposite of what an
/// unassessed dimension means. ADR-0007 makes `insufficient_information`
/// first-class precisely so that "we do not know" is a statement rather than a
/// blank, and this is where that becomes visible or does not.
///
/// `question` travels with the value because a dimension read under the wrong
/// question is worse than one not shown: the whole point of a named scale is
/// that everybody is answering the same thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DimensionView {
    /// The scale id: `source_reliability`, `identifier_match`, and so on.
    pub dimension: String,
    pub title: String,
    /// The scale's own question, from the case's copy of the scale.
    pub question: String,
    /// `unassessed`, `assessed`, or `disagreed`.
    pub agreement: String,
    /// Empty when unassessed. More than one when rows in this cluster were
    /// assessed differently and nobody has reconciled them — reported as a
    /// disagreement rather than resolved here, because picking a winner is the
    /// composite score wearing a different hat.
    pub values: Vec<DimensionValueView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DimensionValueView {
    /// The entity row assessed. After a merge this is how a disagreement is
    /// attributable rather than merely visible.
    pub entity_id: String,
    /// A key on an ordinal scale, never a number. `insufficient_information` is
    /// a value like any other and is stored, not inferred.
    pub value_key: String,
    pub label: String,
    /// Which version of the scale the assessment was made under, from the case's
    /// own copy — so a judgement made under version 1 still reads correctly in a
    /// build that ships version 2.
    pub scale_version: i64,
    /// `user:...` or `rule:` / `ai:`. An automated assessment shown as a
    /// person's is the provenance failure ADR-0008 names.
    pub actor: String,
    pub assessed_utc: String,
    /// What the assessment says it was based on, verbatim JSON from the case.
    pub contributing_factors_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionView {
    /// `merge`, `split` or `reject`.
    pub action: String,
    pub actor: String,
    /// Why the analyst thought so. The product, not a debugging aid.
    pub rationale: String,
    pub decided_utc: String,
}
