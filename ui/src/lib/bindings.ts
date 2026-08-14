/**
 * The command boundary, typed.
 *
 * Every call into Rust goes through this file and nothing else calls `invoke`.
 * That is the same rule `src-tauri/src/views.rs` holds on the other side — one
 * file lists what crosses, so a person can read the whole boundary in one
 * sitting — and `the_typescript_bindings_cover_every_view_field` fails if a
 * field is added to a Rust view and not mirrored here.
 *
 * # Errors are codes, not prose
 *
 * `CommandError.code` is a closed set and it is the only thing any caller may
 * branch on. Matching on `message` is what D-028 exists to prevent: a reworded
 * message silently removes a branch, and a wrong-passphrase dialog that stops
 * appearing because somebody improved the wording is a bug nobody catches in
 * review. Messages are for showing a person, never for deciding.
 */

import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/** Mirrors `ErrorCode` in `src-tauri/src/error.rs`, serialised snake_case. */
export type ErrorCode =
  | "no_case_open"
  | "case_already_open"
  | "wrong_passphrase"
  | "mistyped_recovery_key"
  | "not_a_case"
  | "case_exists"
  | "storage"
  | "not_found"
  | "search_index_corrupt"
  | "evidence_required"
  | "explanation_required"
  | "already_merged"
  | "not_merged"
  | "invalid_value"
  | "machine_may_not_act"
  | "file_unreadable"
  | "extraction_failed"
  | "evidence_unavailable"
  | "internal";

export interface CommandError {
  code: ErrorCode;
  message: string;
}

/**
 * Narrow an unknown rejection to a `CommandError`.
 *
 * Tauri rejects with whatever the command's error type serialised to, so in
 * practice this is always a `CommandError` — but "in practice" is not a type,
 * and a panic in the command layer arrives here as a string. Anything that is
 * not recognisably one becomes `internal`, which is documented on the Rust side
 * as a bug rather than a bucket, so it reads correctly either way.
 */
export function asCommandError(error: unknown): CommandError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof (error as CommandError).code === "string"
  ) {
    return error as CommandError;
  }
  return {
    code: "internal",
    message: typeof error === "string" ? error : String(error),
  };
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

export interface CaseView {
  case_id: string;
  root: string;
  schema_version: number;
  /**
   * False means a forgotten passphrase is permanent. The interface must say so
   * out loud rather than leave it to be discovered.
   */
  has_recovery_key: boolean;
}

/**
 * The one payload in this entire boundary that carries key material, and it
 * does so exactly once (D-026).
 *
 * There is no command that returns `recovery_key` again — not by design
 * oversight but because keeping it for the session or re-deriving it are both
 * things this design refuses. If the interface does not put it in front of the
 * user now, it is gone and a forgotten passphrase becomes permanent.
 */
export interface NewCaseView {
  case: CaseView;
  recovery_key: string;
}

export interface SessionView {
  case: CaseView | null;
}

// ---------------------------------------------------------------------------
// Evidence and lineage
// ---------------------------------------------------------------------------

/**
 * How completely one artifact has been read.
 *
 * The same five words the case-wide coverage figure is counted from, so a
 * document's own state and the caveat on a result list cannot disagree.
 * `complete` means the extractor reported no shortfall — not that the document
 * was understood.
 */
export type ExtractionState =
  | "never_attempted"
  | "in_progress"
  | "failed"
  | "partial"
  | "complete";

export interface ArtifactRowView {
  artifact_id: string;
  media_type: string;
  byte_length: number;
  content_hash: string;
  collected_utc: string;
  /** Null when the case recorded no capture for this artifact. */
  canonical_locator: string | null;
  observations: number;
  extraction: ExtractionState;
}

/** Something the run that read this document reported it did not cover. */
export interface GapView {
  gap_kind: string;
  detail: string;
  /**
   * Null when the run did not know how much it missed. That is a real answer
   * and not a zero — a parser that stopped early cannot report what was left —
   * so it must never be rendered as `0`.
   */
  magnitude: number | null;
  unit: string | null;
}

export interface ObservationRowView {
  observation_id: string;
  kind: string;
  value: string;
  observed_utc: string;
  superseded: boolean;
}

export interface EvidenceListView {
  artifact: ArtifactRowView;
  observations: ObservationRowView[];
  /** Withdrawn rows in this artifact, counted whether or not they were sent. */
  superseded: number;
  gaps: GapView[];
}

export interface SupersessionView {
  /**
   * Null means a rerun read the same document and no longer found this at all,
   * which is a stronger statement than a corrected value.
   */
  superseded_by: string | null;
  run_id: string;
  recorded_utc: string;
}

export interface LocatorView {
  scheme: string;
  selector: string | null;
  start: number | null;
  end: number | null;
  /** Always present, always verbatim, whatever the scheme. */
  raw_json: string;
}

export interface ExcerptView {
  before: string;
  quoted: string;
  after: string;
  lossy: boolean;
}

/**
 * Whether the document still says what the observation claims it says.
 *
 * Resolved in Rust against bytes the AEAD and the content hash both accepted
 * (A-037, D-038). The interface renders the verdict and does not compute one:
 * a panel that compared these itself would be comparing two strings it was
 * handed, which is how a citation comes to agree with itself.
 */
export type QuoteView =
  | { agreement: "exact"; excerpt: ExcerptView }
  | { agreement: "contains"; excerpt: ExcerptView }
  | { agreement: "differs"; excerpt: ExcerptView }
  | { agreement: "out_of_range"; byte_length: number; start: number; end: number }
  | { agreement: "too_large"; bytes: number; limit: number }
  | { agreement: "unknown_scheme"; scheme: string }
  | { agreement: "unavailable"; document_state: string };

export interface RunView {
  run_id: string;
  transform_name: string;
  transform_version: string;
  code_version: string;
  started_utc: string;
  finished_utc: string | null;
  status: string;
  error_code: string | null;
  error_detail: string | null;
}

export interface CaptureRefView {
  capture_id: string;
  requested_utc: string;
  http_status: number | null;
  capture_completeness: string;
  /** True when these bytes were replayed from a fixture rather than fetched. */
  from_fixture: boolean;
  connector: string;
  job_id: string;
}

export interface SourceRefView {
  source_id: string;
  kind: string;
  raw_locator: string;
  canonical_locator: string;
  first_seen_utc: string;
}

export interface CollectionView {
  artifact_id: string;
  ingest: RunView | null;
  capture: CaptureRefView;
  source: SourceRefView;
}

export interface LineageView {
  extraction: RunView;
  artifact: ArtifactRowView;
  /** Null means an artifact with no recorded capture — a broken case. */
  collection: CollectionView | null;
  /**
   * Other artifacts holding these exact bytes. Not a storage detail: the same
   * document reaching a case from two places is an investigative fact.
   */
  also_collected: CollectionView[];
}

export interface ObservationView {
  observation_id: string;
  kind: string;
  value: string;
  observed_utc: string;
  superseded: SupersessionView | null;
  locator: LocatorView;
  quote: QuoteView;
  lineage: LineageView;
}

/**
 * Why a document can or cannot be shown, as five answers.
 *
 * `shredded` is the case working correctly and must never be presented as a
 * fault; `lost` and `damaged` are. Reporting loss as shredding would file an
 * undetected failure as retention policy (A-032).
 */
export type DocumentContent =
  | { state: "readable"; text: string; truncated_bytes: number; lossy: boolean }
  | { state: "shredded"; shredded_utc: string | null }
  | { state: "unrecorded" }
  | { state: "lost" }
  | { state: "damaged"; detail: string };

export interface DocumentView {
  artifact_id: string;
  media_type: string;
  byte_length: number;
  content_hash: string;
  content: DocumentContent;
}

// ---------------------------------------------------------------------------
// Entities and confidence
// ---------------------------------------------------------------------------

/**
 * One entity in a list.
 *
 * `assessed_dimensions` counts dimensions somebody has judged, out of the seven
 * the case knows. It is **not** a score and must never be rendered as one: it
 * says whether anyone looked, not what they concluded, so "7 of 7" describes a
 * thoroughly examined entity that may be thoroughly doubtful (A-033).
 */
export interface EntityRowView {
  entity_id: string;
  type_key: string;
  display_name: string;
  merged_count: number;
  identifier_count: number;
  evidence_count: number;
  assessed_dimensions: number;
  total_dimensions: number;
}

export interface EntityRefView {
  entity_id: string;
  display_name: string;
  type_key: string;
}

export interface IdentifierView {
  identifier_id: string;
  /** The row it hangs on, which after a merge is often not the canonical one. */
  entity_id: string;
  namespace: string;
  /** Exactly as the evidence gave it. */
  value: string;
}

export interface EvidenceView {
  link_id: string;
  entity_id: string;
  evidence_kind: string;
  evidence_id: string;
  /**
   * `supports`, `contradicts` or `context`. Evidence arguing against a
   * conclusion has somewhere to live, and rendering all three alike throws that
   * away.
   */
  role: string;
}

/**
 * One assessment.
 *
 * `value_key` is a key on a named ordinal scale, never a number, and
 * `scale_version` is the only numeric field in this whole block. There is
 * deliberately nothing here to average.
 */
export interface DimensionValueView {
  entity_id: string;
  value_key: string;
  label: string;
  scale_version: number;
  actor: string;
  assessed_utc: string;
  contributing_factors_json: string;
}

/**
 * One dimension, whether or not anybody has assessed it.
 *
 * All seven are always present (D-031). A sparse list renders as absence and
 * absence reads as "no concern", which is the opposite of what an unassessed
 * dimension means.
 */
export interface DimensionView {
  dimension: string;
  title: string;
  /** The scale's own question, from the case's copy of the scale. */
  question: string;
  /** `unassessed`, `assessed`, or `disagreed`. */
  agreement: string;
  values: DimensionValueView[];
}

export interface DecisionView {
  action: string;
  actor: string;
  /** Why. The product, not a debugging aid (D-033). */
  rationale: string;
  decided_utc: string;
}

export interface EntityView {
  requested_id: string;
  entity_id: string;
  redirected: boolean;
  type_key: string;
  display_name: string;
  notes: string;
  merged_from: EntityRefView[];
  identifiers: IdentifierView[];
  evidence: EvidenceView[];
  confidence: DimensionView[];
  history: DecisionView[];
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

export function storageEncryptionStatus(): Promise<string> {
  return invoke("storage_encryption_status");
}

export function createCase(
  path: string,
  caseId: string,
  passphrase: string,
): Promise<NewCaseView> {
  return invoke("create_case", { path, caseId, passphrase });
}

export function openCase(path: string, passphrase: string): Promise<CaseView> {
  return invoke("open_case", { path, passphrase });
}

export function openCaseWithRecoveryKey(
  path: string,
  recoveryKey: string,
): Promise<CaseView> {
  return invoke("open_case_with_recovery_key", { path, recoveryKey });
}

export function closeCase(): Promise<void> {
  return invoke("close_case");
}

export function sessionStatus(): Promise<SessionView> {
  return invoke("session_status");
}

export function artifacts(limit?: number): Promise<ArtifactRowView[]> {
  return invoke("artifacts", { limit });
}

export function artifactObservations(
  artifactId: string,
  includeSuperseded: boolean,
): Promise<EvidenceListView> {
  return invoke("artifact_observations", { artifactId, includeSuperseded });
}

export function observation(observationId: string): Promise<ObservationView> {
  return invoke("observation", { observationId });
}

export function artifactDocument(artifactId: string): Promise<DocumentView> {
  return invoke("artifact_document", { artifactId });
}

export function entities(limit?: number): Promise<EntityRowView[]> {
  return invoke("entities", { limit });
}

export function entity(entityId: string): Promise<EntityView> {
  return invoke("entity", { entityId });
}
