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
//!
//! # And a third: no request names its own actor
//!
//! Every write in this product records who made it, and `rule:` and `ai:` actors
//! are refused the decisions only a person may make (ADR-0008). `kokin_graph`
//! says plainly that no schema can catch automation writing `actor =
//! "user:local"` — it is a lie rather than a bypass.
//!
//! The IPC boundary is where that lie becomes *reachable*, because a request
//! field is written by the webview. So no `Deserialize` type in this file has an
//! `actor` field, every write request is `deny_unknown_fields`, and
//! [`crate::write::ACTOR`] is a constant (D-032, A-034). The webview names the
//! evidence and the reasoning; it does not name the author.

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
    /// (A-032), and this is the only state in this enum that is an accusation.
    Damaged { detail: String },
}

// ---------------------------------------------------------------------------
// Evidence and lineage
// ---------------------------------------------------------------------------

/// One artifact in a list of them.
///
/// `extraction` is the same five-state coverage vocabulary the case-wide figure
/// is counted from, per artifact, so "this document has never been read" is
/// visible in the list rather than only in an aggregate. It is computed by
/// `kokin_search::coverage::artifact_coverage` and not here — the rule has one
/// definition (D-029's reasoning, applied to a single row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactRowView {
    pub artifact_id: String,
    pub media_type: String,
    pub byte_length: i64,
    pub content_hash: String,
    pub collected_utc: String,
    /// Where this copy came from, if the case recorded a capture for it.
    pub canonical_locator: Option<String>,
    /// Observations recorded against it, superseded ones excluded.
    pub observations: i64,
    /// `never_attempted`, `in_progress`, `failed`, `partial` or `complete`.
    pub extraction: String,
}

/// The observations one artifact yielded, and what the reading missed.
///
/// `gaps` travels in the same payload for the reason coverage travels with
/// search results: a table of forty observations from a document whose text was
/// truncated is more misleading than an empty one, because a full table does not
/// get interrogated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceListView {
    pub artifact: ArtifactRowView,
    pub observations: Vec<ObservationRowView>,
    /// Superseded rows in this artifact, whether or not they were returned. A
    /// count rather than a silence: the case previously asserted these.
    pub superseded: i64,
    pub gaps: Vec<GapView>,
}

/// Something the run that read this document reported it did not cover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GapView {
    /// A stable code, for counting: `text_truncated`, `value_oversize`.
    pub gap_kind: String,
    /// The same thing in words, for showing.
    pub detail: String,
    /// `None` when the run genuinely did not know how much it missed, which is
    /// a real answer and not a zero — a parser that stopped early cannot report
    /// what was left. An interface rendering it as 0 has invented a measurement.
    pub magnitude: Option<i64>,
    pub unit: Option<String>,
}

/// One observation as a row in a table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservationRowView {
    pub observation_id: String,
    pub kind: String,
    pub value: String,
    pub observed_utc: String,
    /// True when a later run of the same extractor withdrew this row (ADR-0006).
    pub superseded: bool,
}

/// One observation, everything that produced it, and the bytes it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservationView {
    pub observation_id: String,
    pub kind: String,
    pub value: String,
    pub observed_utc: String,
    /// Present when a later run withdrew this observation.
    pub superseded: Option<SupersessionView>,
    pub locator: LocatorView,
    /// What the document actually says at that locator — see [`QuoteView`].
    pub quote: QuoteView,
    pub lineage: LineageView,
}

/// A withdrawal, and what replaced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupersessionView {
    /// The observation that took its place. `None` means a rerun read the same
    /// document and no longer found this at all, which is a stronger statement
    /// than a corrected value and must not render as one.
    pub superseded_by: Option<String>,
    pub run_id: String,
    pub recorded_utc: String,
}

/// Where in the artifact an observation came from.
///
/// `raw_json` is always present and always verbatim. The parsed fields are this
/// build's reading of a locator scheme, and a case may hold a scheme this build
/// has never seen — in which case the parsed fields are empty and the raw form
/// is the only honest thing to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocatorView {
    /// `html_byte_range` today. `unrecorded` when the stored JSON names none.
    pub scheme: String,
    /// What was being looked for, when the scheme records it.
    pub selector: Option<String>,
    pub start: Option<i64>,
    pub end: Option<i64>,
    pub raw_json: String,
}

/// The document's own bytes around a locator, split so the interface never has
/// to do offset arithmetic.
///
/// Three strings rather than one string and two indices, because the one thing
/// this panel exists to get right is *which bytes are being cited*, and an
/// off-by-one in the webview would highlight the wrong span while looking
/// entirely convincing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExcerptView {
    pub before: String,
    /// The bytes the locator names.
    pub quoted: String,
    pub after: String,
    /// True if these bytes were not valid UTF-8 and were replaced lossily.
    pub lossy: bool,
}

/// Whether the document still says what the observation claims it says.
///
/// An observation stores a `value` and a `locator_json`, and nothing has ever
/// checked that the second supports the first (A-037). Showing them side by side
/// without checking invites the reader to assume a correspondence that no code
/// asserts, which is the whole failure mode of a citation.
///
/// So the check happens here, over bytes the AEAD and the content hash have both
/// accepted, and the verdict is the tag (D-038). `differs` is a real outcome and
/// is rendered as one rather than hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "agreement", rename_all = "snake_case")]
pub enum QuoteView {
    /// The bytes at the locator are exactly the recorded value.
    Exact { excerpt: ExcerptView },
    /// The recorded value appears within the bytes at the locator. Normal: a
    /// locator commonly spans an element while the value is its text or one
    /// attribute.
    Contains { excerpt: ExcerptView },
    /// The bytes at the locator do not contain the recorded value. Either the
    /// extractor recorded the wrong range or the value was transformed on its
    /// way to the row; either way the citation does not support the claim.
    Differs { excerpt: ExcerptView },
    /// The locator names a range this artifact does not have.
    OutOfRange {
        byte_length: i64,
        start: i64,
        end: i64,
    },
    /// The locator names more bytes than this panel will hold in memory.
    ///
    /// Distinct from every other outcome on purpose: a truncated quote could be
    /// compared for `contains` but never for `differs`, because the value might
    /// be in the part that was dropped. Rather than emit a verdict that is sound
    /// in one direction only, the check declines.
    TooLarge { bytes: i64, limit: i64 },
    /// A locator scheme this build cannot resolve. Not a fault — see
    /// [`LocatorView`] — and the raw locator is still shown.
    UnknownScheme { scheme: String },
    /// The bytes are not available to check against. Carries the
    /// [`DocumentContent`] state that explains why, so a shredded document and a
    /// damaged one do not both read as "no quote".
    Unavailable { document_state: String },
}

/// Everything the case knows about how an observation came to exist.
///
/// Read from the `derivation` edges rather than from the convenient foreign
/// keys. `observation.artifact_id` and `capture.source_id` are denormalisations
/// of the same facts; the edges are what the provenance model actually promises,
/// and reading the shortcut would leave the promise untested by anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineageView {
    /// The run that read the document and recorded this observation.
    pub extraction: RunView,
    pub artifact: ArtifactRowView,
    /// How this artifact was collected. `None` means the case holds an artifact
    /// with no recorded capture — a broken case rather than a normal one, and
    /// worth showing as such instead of rendering an empty panel.
    pub collection: Option<CollectionView>,
    /// Other artifacts in this case holding these exact bytes.
    ///
    /// Not a storage detail. The same document reaching a case from two places
    /// is an investigative fact, and a lineage panel naming one of them tells
    /// the analyst this evidence has a single origin when the case knows it does
    /// not.
    pub also_collected: Vec<CollectionView>,
}

/// One collection of one artifact: the fetch, and where it was fetched from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionView {
    pub artifact_id: String,
    /// The run that performed the collection.
    pub ingest: Option<RunView>,
    pub capture: CaptureRefView,
    pub source: SourceRefView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureRefView {
    pub capture_id: String,
    pub requested_utc: String,
    pub http_status: Option<i64>,
    /// `full`, `partial`, and whatever a later collector records. What the
    /// collector says it got, not what it wanted.
    pub capture_completeness: String,
    /// True when these bytes were replayed from a recorded fixture rather than
    /// fetched. An analyst reading a fixture as a live collection has the wrong
    /// date on the evidence.
    pub from_fixture: bool,
    pub connector: String,
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRefView {
    pub source_id: String,
    pub kind: String,
    /// Exactly what was asked for.
    pub raw_locator: String,
    /// The normalised form the case deduplicates on. Shown beside the raw one
    /// rather than instead of it: the normalisation is this product's opinion,
    /// and the raw locator is what the analyst typed or the connector produced.
    pub canonical_locator: String,
    pub first_seen_utc: String,
}

/// One transform run, named by what it was and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunView {
    pub run_id: String,
    pub transform_name: String,
    pub transform_version: String,
    /// The build that ran it. A case re-read by a later build holds two runs
    /// whose only difference is this, which is how "why does this document say
    /// something different now" becomes answerable.
    pub code_version: String,
    pub started_utc: String,
    pub finished_utc: Option<String>,
    /// `succeeded`, `failed`, `running`.
    pub status: String,
    pub error_code: Option<String>,
    pub error_detail: Option<String>,
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

/// One entity in a list of them.
///
/// Only canonical entities appear: a list showing both halves of a merge asserts
/// that two people exist, which is the same failure `resolve_merged` exists to
/// prevent in search (ADR-0008).
///
/// `assessed_dimensions` is a count of dimensions with at least one assessment,
/// out of the seven the case knows. It is deliberately **not** a score and
/// cannot become one: it counts whether somebody looked, not what they
/// concluded, so "7 of 7" describes a thoroughly examined entity that may be
/// thoroughly doubtful (A-033).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityRowView {
    pub entity_id: String,
    pub type_key: String,
    pub display_name: String,
    /// Rows merged into this one. Non-zero means this record is a projection
    /// over several, which changes what its identifier list means.
    pub merged_count: i64,
    pub identifier_count: i64,
    pub evidence_count: i64,
    pub assessed_dimensions: i64,
    pub total_dimensions: i64,
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

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

/// A local file to bring into the case.
///
/// There is deliberately no `ingest_url` request beside this one. Fetching needs
/// an `HttpCapability`, the workspace holds no live HTTP client, and wiring one
/// is the egress policy, the resolver, the rate limiter and the network activity
/// log — not a field on a struct.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestFileRequest {
    pub path: String,
}

/// What arrived, and where it came from.
///
/// The provenance ids are returned rather than kept private because the next
/// thing an analyst does with a document is cite it, and a caller that cannot
/// name the observation it is citing will cite the artifact instead — a weaker
/// claim recorded as if it were the same one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestView {
    pub run_id: String,
    pub source_id: String,
    pub capture_id: String,
    pub artifact_id: String,
    pub content_hash: String,
    pub size_bytes: u64,
    /// True when the case already held these exact bytes. An investigative fact
    /// — the same document reached this case twice — not a storage detail, and
    /// an interface that renders it as "imported" has thrown that away.
    pub deduplicated: bool,
    pub from_fixture: bool,
}

/// Which artifact to read.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractRequest {
    pub artifact_id: String,
}

/// What one extraction produced, and what it could not.
///
/// `skipped_oversize` and `text_truncated_bytes` are not optional and not
/// diagnostics. An extraction that succeeded while silently dropping values is
/// the same lie as a result list without its coverage (D-029): the case now holds
/// a document it has only partly read, and nothing downstream can tell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractView {
    pub run_id: String,
    pub artifact_id: String,
    /// How many observations this run recorded.
    pub observations: usize,
    /// Values too long to record, and therefore absent from the case.
    pub skipped_oversize: usize,
    /// Prose this document holds that search will not find.
    pub text_truncated_bytes: usize,
    /// Observations from an earlier run of this extractor that this run
    /// withdrew. Non-zero means the case previously asserted something it no
    /// longer does.
    pub superseded: usize,
    /// Computed here, for the same reason [`CoverageView::is_complete`] is: it is
    /// the figure a caveat is written from, and a caller deriving it can derive
    /// it wrong in the direction of a document looking fully read.
    pub complete: bool,
}

// ---------------------------------------------------------------------------
// Analysis: what the interface may write
// ---------------------------------------------------------------------------

/// One piece of evidence a write is grounded in.
///
/// `kind` is `observation`, `artifact` or `capture` — L0 and L1 only. An
/// analytical row is never evidence for another analytical row, because that is
/// how a conclusion comes to support itself.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundingInput {
    pub kind: String,
    pub id: String,
    /// `supports`, `contradicts` or `context`. Defaults to `supports`, which is
    /// the common case; the other two exist so that evidence arguing *against* a
    /// conclusion has somewhere to live.
    #[serde(default = "supports")]
    pub role: String,
}

fn supports() -> String {
    "supports".to_string()
}

/// A new entity, and what says it exists.
///
/// `evidence` has no `#[serde(default)]`, so a request that omits it fails to
/// deserialise rather than arriving as an empty list. That is the point of
/// putting the requirement in the signature: `kokin_graph` refuses an ungrounded
/// entity and a trigger refuses it again, but both of those are reached *after* a
/// caller has been allowed to express the idea.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewEntityRequest {
    pub type_key: String,
    pub display_name: String,
    #[serde(default)]
    pub notes: String,
    pub evidence: Vec<GroundingInput>,
}

/// An identifier to attach to an entity.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewIdentifierRequest {
    pub entity_id: String,
    /// `email_address`, `username`, `phone_number`, and so on.
    pub namespace: String,
    /// Exactly as the evidence gave it. The normalised form used for equality is
    /// derived in `kokin_graph` and stored beside it, never instead of it.
    pub value: String,
    pub evidence: Vec<GroundingInput>,
}

/// A relationship between two entities.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRelationshipRequest {
    pub from_entity: String,
    pub to_entity: String,
    /// The relationship's own type. "Same username", "similar photograph" and
    /// "same person" are three different kinds and must not be collapsed into
    /// one.
    pub kind: String,
    /// Both optional and both staying optional. Most relationships are asserted
    /// without a period, and inventing a start date to fill a column is a claim
    /// the evidence does not make.
    #[serde(default)]
    pub started_utc: Option<String>,
    #[serde(default)]
    pub ended_utc: Option<String>,
    pub evidence: Vec<GroundingInput>,
}

/// One dimension of confidence about one subject (ADR-0007).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessRequest {
    /// `entity`, `identifier` or `relationship`.
    pub subject_kind: String,
    pub subject_id: String,
    /// The scale id: `source_reliability`, `identifier_match`, and so on.
    pub dimension: String,
    /// A key on that scale. Never a number, and `insufficient_information` is a
    /// value somebody chooses rather than a default the code supplies.
    pub value_key: String,
    /// Why. Required and required to be non-empty (D-033): this is the "why"
    /// panel, and a factor that is not recorded cannot be displayed.
    ///
    /// Taken as a list of strings and serialised here, so the column always holds
    /// well-formed JSON — a caller cannot put arbitrary text in a field the rest
    /// of the product reads as structure.
    pub contributing_factors: Vec<String>,
}

/// Two entities an analyst says are one.
///
/// There is no "survivor" field. Which row survives is decided by
/// `kokin_graph::resolution` — the older of the two clusters — so that the same
/// two merges always produce the same-looking case, and so that a caller cannot
/// make the choice by accident.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRequest {
    pub left: String,
    pub right: String,
    /// Why they are the same. Required (D-033). A merge dissolves the distinction
    /// between two records, and ADR-0008 makes it reversible precisely so it can
    /// be reconsidered — which is impossible without knowing why it was made.
    pub rationale: String,
    /// The proposal this decision settles, if it settles one.
    #[serde(default)]
    pub candidate_id: Option<String>,
}

/// Two entities an analyst says are *not* one.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectRequest {
    pub left: String,
    pub right: String,
    pub rationale: String,
    #[serde(default)]
    pub candidate_id: Option<String>,
}

/// An entity to take back out of its cluster.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitRequest {
    pub entity: String,
    pub rationale: String,
}

/// A row this command wrote, and who it recorded as the author.
///
/// `actor` is echoed back deliberately. The interface may display who a write is
/// attributed to and cannot choose it — see the module note — and returning it
/// makes that visible rather than merely true.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteView {
    pub id: String,
    pub actor: String,
}

/// A resolution decision, and where the entity ended up.
///
/// `canonical_id` is returned because after a merge the id the interface was
/// holding may no longer be the one to display, and after a split it is again.
/// An interface left to work that out re-derives identity, which is exactly the
/// thing ADR-0008 makes a read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolutionView {
    pub decision_id: String,
    pub canonical_id: String,
    pub actor: String,
}
