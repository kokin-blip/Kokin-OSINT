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
import { listen } from "@tauri-apps/api/event";

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
  session_generation: number;
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
  session_generation: number;
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
  | { agreement: "structured_exact"; excerpt: ExcerptView; basis: string }
  | { agreement: "derived_exact"; excerpt: ExcerptView; basis: string }
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

export interface SearchRequest {
  query: string;
  limit: number | null;
  offset: number;
  kinds: Array<string>;
  facets: Array<string>;
  include_superseded: boolean;
  resolve_merged: boolean | null;
  typeahead: boolean;
}

export interface HitView {
  subject_kind: string;
  subject_id: string;
  title: string;
  snippet: string;
  facet: string;
  superseded: boolean;
  canonical_id: string | null;
}

export interface CoverageView {
  artifacts: number;
  never_attempted: number;
  in_progress: number;
  failed: number;
  partial: number;
  complete: number;
  incomplete: number;
  is_complete: boolean;
}

export interface SearchView {
  hits: Array<HitView>;
  total: number;
  coverage: CoverageView;
}

export interface IngestFileRequest {
  path: string;
}

export interface IngestView {
  run_id: string;
  source_id: string;
  capture_id: string;
  artifact_id: string;
  content_hash: string;
  size_bytes: number;
  deduplicated: boolean;
  from_fixture: boolean;
}

export interface ExtractRequest {
  artifact_id: string;
}

export interface ExtractView {
  run_id: string;
  artifact_id: string;
  observations: number;
  skipped_oversize: number;
  text_truncated_bytes: number;
  superseded: number;
  complete: boolean;
}

export interface GroundingInput {
  kind: string;
  id: string;
  role: string;
}

export interface NewEntityRequest {
  type_key: string;
  display_name: string;
  notes: string;
  evidence: Array<GroundingInput>;
}

export interface NewIdentifierRequest {
  entity_id: string;
  namespace: string;
  value: string;
  evidence: Array<GroundingInput>;
}

export interface NewRelationshipRequest {
  from_entity: string;
  to_entity: string;
  kind: string;
  started_utc: string | null;
  ended_utc: string | null;
  evidence: Array<GroundingInput>;
}

export interface AssessRequest {
  subject_kind: string;
  subject_id: string;
  dimension: string;
  value_key: string;
  contributing_factors: Array<string>;
}

export interface MergeRequest {
  left: string;
  right: string;
  rationale: string;
  candidate_id: string | null;
}

export interface RejectRequest {
  left: string;
  right: string;
  rationale: string;
  candidate_id: string | null;
}

export interface SplitRequest {
  entity: string;
  rationale: string;
}

export interface WriteView {
  id: string;
  actor: string;
}

export interface ResolutionView {
  decision_id: string;
  canonical_id: string;
  actor: string;
}

export interface PageRequest {
  offset: number;
  limit: number | null;
}

export interface PageView<T> {
  rows: Array<T>;
  total: number;
  offset: number;
  limit: number;
}

export interface AuditRowView {
  id: number;
  occurred_utc: string;
  actor: string;
  action: string;
  subject_kind: string | null;
  subject_id: string | null;
  payload_json: string;
}

export interface ActivityView {
  page: PageView<AuditRowView>;
  chain_status: string;
  chain_detail: string;
}

export interface SubjectView {
  subject_kind: string;
  subject_id: string;
  title: string;
  fields: Array<[string, string]>;
  evidence: Array<EvidenceView>;
  confidence: Array<DimensionView>;
}

export interface ScaleOptionView {
  dimension: string;
  title: string;
  values: Array<[string, string]>;
}

export const artifactPage = (offset = 0) => invoke<PageView<ArtifactRowView>>("artifact_page", {request:{offset,limit:50}});
export const entityPage = (offset = 0) => invoke<PageView<EntityRowView>>("entity_page", {request:{offset,limit:50}});
export const observationPage = (artifactId: string, includeSuperseded = false, offset = 0) => invoke<[EvidenceListView, PageView<ObservationRowView>]>("observation_page", {artifactId,includeSuperseded,request:{offset,limit:50}});
export const searchCase = (request: Partial<SearchRequest>) => invoke<SearchView>("search", {request});
export const activityPage = (offset = 0) => invoke<ActivityView>("activity", {request:{offset,limit:50}});
export const subject = (subjectKind:string,subjectId:string) => invoke<SubjectView>("subject",{subjectKind,subjectId});
export const scaleOptions = () => invoke<ScaleOptionView[]>("scales");
export const createEntity = (request:NewEntityRequest) => invoke<WriteView>("create_entity",{request});
export const addIdentifier = (request:NewIdentifierRequest) => invoke<WriteView>("add_identifier",{request});
export const relate = (request:NewRelationshipRequest) => invoke<WriteView>("relate",{request});
export const assess = (request:AssessRequest) => invoke<WriteView>("assess",{request});
export const mergeEntities = (request:MergeRequest) => invoke<ResolutionView>("merge_entities",{request});
export const rejectMerge = (request:RejectRequest) => invoke<WriteView>("reject_merge",{request});
export const splitEntity = (request:SplitRequest) => invoke<ResolutionView>("split_entity",{request});
export const extractArtifact = (artifactId:string) => invoke<ExtractView>("extract",{request:{artifact_id:artifactId}});

export interface OperationView {
  operation_id: string;
  status: string;
  stage: string;
  cancellable: boolean;
  result_json: string | null;
  error: CommandError | null;
}

export const selectPath = (purpose: "research_csv"|"research_json"|"research_import"|"import"|"export"|"package"|"restore"|"report_markdown"|"report_html", expectedGeneration:number) => invoke<string|null>("select_path",{purpose,expectedGeneration});
export const startOperation = (kind:string,args:unknown,expectedGeneration:number) => invoke<string>("start_operation",{kind,args,expectedGeneration});
export const operationStatus = (operationId:string) => invoke<OperationView>("operation_status",{operationId});
export const cancelOperation = (operationId:string) => invoke<void>("cancel_operation",{operationId});
export const networkMode = () => invoke<boolean>("network_mode");
export const setNetworkMode = (online:boolean) => invoke<boolean>("set_network_mode",{online});
export const entityTypes = () => invoke<Array<[string,string]>>("entity_types");

export const evidenceTarget=(kind:string,id:string)=>invoke<[string,string]>("evidence_target",{kind,id});
export const relationships=(id:string,offset=0)=>invoke<PageView<SubjectView>>("relationships",{id,request:{offset,limit:50}});

export interface GraphNodeView {
  id: string;
  name: string;
  canonical_id: string;
  depth: number;
}

export interface GraphEdgeView {
  id: string;
  from: string;
  to: string;
  kind: string;
}

export interface GraphView {
  truncation_reasons?:string[];
  nodes: Array<GraphNodeView>;
  edges: Array<GraphEdgeView>;
  truncated: boolean;
  completed_hops: number;
}

export type WorkspaceKind='board'|'report'|'guide';
export interface WorkspaceView {id:string;kind:WorkspaceKind;name:string;revision:number;payload:Record<string,unknown>;archived:boolean;updated_utc:string;}
export interface SaveWorkspace {id:string;kind:WorkspaceKind;name:string;expected_revision:number;payload:Record<string,unknown>;archived?:boolean;}
export const workspacePage=(generation:number,kind:WorkspaceKind,offset=0)=>invoke<PageView<WorkspaceSummaryView>>('workspace_page',{generation,kind,offset});
export const workspaceRead=(generation:number,id:string)=>invoke<WorkspaceView>('workspace_read',{generation,id});
export const workspaceSave=(generation:number,request:SaveWorkspace)=>invoke<WorkspaceView>('workspace_save',{generation,request});

export interface GuideStatus {artifacts:number;complete:number;entities:number;relationships:number;human_assessments:number;boards:number;reports:number;}
export const guideStatus=(generation:number)=>invoke<GuideStatus>('guide_status',{generation});

export interface AddEvidenceRequest {subject_kind:string;subject_id:string;evidence:GroundingInput[];}
export type WorkspacePage=PageView<WorkspaceSummaryView>;
export type WorkspaceSaveRequest=SaveWorkspace;
export interface Citation {kind:string;id:string;role:string;}
export interface Finding {heading:string;text:string;citations:Citation[];}
export interface ReportPayload {version:number;summary:string;findings:Finding[];limitations:string;boards:string[];}

export type WorkspaceSummaryView=Omit<WorkspaceView,'payload'>;

export interface MediaView {
  artifact_id: string;
  page: number;
  pages: number;
  preview_png: string;
  engine: string;
  limitations: string[];
  duplicate_candidates: string[];
  candidate_scan_truncated: boolean;
}
export function mediaRead(generation:number,artifact_id:string,page=0):Promise<MediaView|null>{return invoke("media_read",{generation,artifactId:artifact_id,page});}

export interface OcrResourceView {
  size_bytes: number;
  destination: string;
  installed: boolean;
  license: string;
  sha256: string;
}
export function ocrResourceInfo():Promise<OcrResourceView>{return invoke("ocr_resource_info");}
export function installOcrResource():Promise<boolean>{return invoke("install_ocr_resource");}

export interface MediaProcessRequest {
  version: number;
  artifact_id: string;
  page: number;
  ocr: boolean;
}

export interface SourceRequest {version:number;connector:string;input:string;record_type:string;page:number;timestamp:string;consent:boolean;}
export interface SourceDisclosureView {connector:string;destinations:string[];transmitted_fields:string[];request_urls:string[];normalized_input:string;limitation:string;max_response_bytes:number;health:string;fixtures_version:number;}
export const sourceDisclosure=(request:SourceRequest)=>invoke<SourceDisclosureView>('source_disclosure',{request});

export interface SourceResultView {artifact_id:string;run_id:string|null;observations:number;size_bytes:number;deduplicated:boolean;from_fixture:boolean;http_status:number;capture_completeness:string;limitation:string;complete:boolean;processing_error:string|null;}

export interface ResourceView {id:string;file:string;bytes:number;sha256:string;url:string;license:string;destination:string;present:boolean;}
export interface ResourceJobView {operation_id:string;resource_id:string;status:string;stage:string;transferred:number;bytes:number;error:string|null;}
export interface ResourceInstallRequest {version:number;resource_id:string;mode:string;consent:boolean;}
export interface AiRequest {version:number;endpoint:string;model:string;task:string;observation_ids:string[];consent:boolean;}
export interface AiSuggestionView {id:string;model:string;model_digest:string;kind:string;text:string;cited_observation_ids:string[];proposed_name:string|null;proposed_type:string|null;relationship_kind:string|null;created_utc:string;review:string|null;review_rationale:string|null;review_actor:string|null;review_utc:string|null;}
export interface AiPageView {rows:AiSuggestionView[];total:number;offset:number;limit:number;}
export interface AiReviewRequest {id:string;decision:string;rationale:string;}
export interface AiModelView {name:string;digest:string;bytes:number;reviewed_artifact:boolean;}
export interface AiStatusView {architecture:string;cpu_threads:number;total_memory_bytes:number|null;available_memory_bytes:number|null;free_storage_bytes:number|null;memory_kind:string;accelerator_status:string;ollama_status:string;models:AiModelView[];loaded_models:unknown;recommendation:string|null;faster:string|null;higher_quality:string|null;}
export interface SemanticRequest {version:number;mode:string;search:SearchRequest;}
export interface SemanticStatusView {indexed:number;observations:number;model_identity:string|null;resources_present:boolean;quality_gate:boolean;limitation:string;}
export interface SemanticView {results:SearchView;indexed:number;candidate_limit:number;truncated:boolean;notice:string;}
export const resourceList=()=>invoke<ResourceView[]>('resource_list');
export const resourceInstall=(request:ResourceInstallRequest)=>invoke<string>('resource_install',{request});
export const resourceStatus=(operationId:string)=>invoke<ResourceJobView>('resource_status',{operationId});
export const resourceCancel=(operationId:string)=>invoke<void>('resource_cancel',{operationId});
export const aiStatus=(endpoint:string)=>invoke<AiStatusView>('ai_status',{endpoint});
export const aiPage=(generation:number,offset=0)=>invoke<AiPageView>('ai_page',{generation,offset});
export const aiReview=(generation:number,request:AiReviewRequest)=>invoke<void>('ai_review',{generation,request});
export const semanticStatus=(generation:number)=>invoke<SemanticStatusView>('semantic_status',{generation});
export const semanticSearch=(generation:number,request:SemanticRequest)=>invoke<SemanticView>('semantic_search',{generation,request});

export interface MapArchiveView {id:string;bytes:number;min_zoom:number;max_zoom:number;tile_type:number;bounds:number[];attribution:string;layers:string[];}
export interface MapSelectionView {selection:string;filename:string;bytes:number;min_zoom:number;max_zoom:number;destination:string;}
export interface MapImportRequest {selection:string;attribution:string;consent:boolean;}
export interface LocationView {id:string;observation_id:string;entity_id:string|null;label:string;longitude:number;latitude:number;uncertainty:string;rationale:string;created_utc:string;}
export interface LocationPageView {rows:LocationView[];total:number;offset:number;limit:number;}
export interface LocationRequest {observation_id:string;entity_id:string|null;label:string;longitude:number;latitude:number;uncertainty:string;rationale:string;}
export interface MapStateView {revision:number;archive_id:string|null;longitude:number;latitude:number;zoom:number;bearing:number;pitch:number;show_locations:boolean;}
export const mapArchives=()=>invoke<MapArchiveView[]>('map_archives');
export const mapSelect=()=>invoke<MapSelectionView|null>('map_select');
export const mapImport=(request:MapImportRequest)=>invoke<string>('map_import',{request});
export const mapRange=(id:string,offset:number,length:number)=>invoke<string>('map_range',{id,offset,length});
export const locationPage=(generation:number,offset=0)=>invoke<LocationPageView>('location_page',{generation,offset});
export const locationCreate=(generation:number,request:LocationRequest)=>invoke<string>('location_create',{generation,request});
export const mapStateRead=(generation:number)=>invoke<MapStateView>('map_state_read',{generation});
export const mapStateSave=(generation:number,request:MapStateView)=>invoke<MapStateView>('map_state_save',{generation,request});

export interface MapIllustrationRequest {image:string;archive_id:string;attribution:string;location_ids:string[];}
export interface MapIllustrationView {id:string;created_utc:string;archive_id:string;attribution:string;}
export const mapIllustrations=(generation:number)=>invoke<MapIllustrationView[]>('map_illustrations',{generation});

export const resourceJobs=()=>invoke<ResourceJobView[]>('resource_jobs');

export interface WorldSelectionView {selection:string;url:string;etag:string;download_bytes:number;maximum_output_bytes:number;destination:string;}
export const mapWorldReview=(consent:boolean)=>invoke<WorldSelectionView>('map_world_review',{consent});
export const mapWorldInstall=(selection:string,consent:boolean)=>invoke<string>('map_world_install',{selection,consent});

export interface AiModelInstallRequest {version:number;model:string;endpoint:string;consent:boolean;}
export const aiModelInstall=(request:AiModelInstallRequest)=>invoke<string>('ai_model_install',{request});

export interface FaceAnalysisRequest {version:number;artifact_ids:string[];page:number;start_ms:number;end_ms:number|null;}
export interface FaceStatusView {eligible_artifacts:number;analyzed_artifacts:number;faces:number;auto_analyze:boolean;resources_present:boolean;native_available:boolean;model_identity:string;limitation:string;}
export interface FaceView {id:string;artifact_id:string;run_id:string;model_identity:string;bbox:number[];image_size:number[];page:number;timestamp_ms:number|null;detection_score:number;crop_png:string|null;stale:boolean;entity_id:string|null;entity_name:string|null;}
export interface FacePageView {faces:FaceView[];total:number;offset:number;limitations:string[];}
export interface FaceSearchRequest {version:number;face_id:string;query:string;text_mode:string;both:boolean;threshold:number;offset:number;limit:number;}
export interface FaceMatchView {face:FaceView;similarity:number;occurrences:FaceView[];occurrence_count:number;}
export interface FaceSearchView {matches:FaceMatchView[];total:number;offset:number;related:SearchView;coverage:FaceStatusView;truncated:boolean;}
export interface FaceReviewRequest {face_id:string;entity_id:string|null;group_id:string|null;decision:string;rationale:string;}
export interface FaceGroupView {id:string;faces:FaceView[];member_total:number;member_offset:number;}
export interface DiscoveryRequest {query:string;page:number;consent:boolean;}
export interface DiscoveryHitView {title:string;url:string;snippet:string;}
export interface DiscoveryView {hits:DiscoveryHitView[];artifact_id:string;query:string;limitation:string;}
export const faceStatus=(generation:number)=>invoke<FaceStatusView>('face_status',{generation});
export const faceAutoAnalyze=(generation:number,enabled:boolean)=>invoke<void>('face_auto_analyze',{generation,enabled});
export const facePage=(generation:number,artifactId='',offset=0,includeStale=false,pageFilter:number|null=null)=>invoke<FacePageView>('face_page',{generation,artifactId,offset,includeStale,pageFilter});
export const faceSearch=(generation:number,request:FaceSearchRequest)=>invoke<FaceSearchView>('face_search',{generation,request});
export const faceReview=(generation:number,request:FaceReviewRequest)=>invoke<void>('face_review',{generation,request});
export const faceGroups=(generation:number,offset=0)=>invoke<FaceGroupView[]>('face_groups',{generation,offset});
export const discoveryHealth=()=>invoke<string>('discovery_health');
export const researchBrowser=(provider:string,query='')=>invoke<void>('research_browser',{provider,query});
export interface FacePreviewView {preview_png:string;face:FaceView;}
export const facePreview=(generation:number,faceId:string)=>invoke<FacePreviewView>('face_preview',{generation,faceId});

export interface FacePersonRequest {face_id:string;name:string;rationale:string;}
export const facePersonCreate=(generation:number,request:FacePersonRequest)=>invoke<string>('face_person_create',{generation,request});
export const personFaces=(generation:number,entityId:string,offset=0)=>invoke<FacePageView>('person_faces',{generation,entityId,offset});

export interface FaceGroupMembersView {group_id:string;page:FacePageView;}
export const faceGroupMembers=(generation:number,groupId:string,offset=0)=>invoke<FaceGroupMembersView>('face_group_members',{generation,groupId,offset});
export const faceRead=(generation:number,faceId:string)=>invoke<FaceView>('face_read',{generation,faceId});

export type SelectorKind='username'|'email'|'phone'|'name'|'domain'|'ip'|'wallet'|'image';
export type Delivery='native'|'provider_backed'|'manual_workflow'|'blocked';
export interface Selector {kind:SelectorKind;original:string;normalized:string;}
export interface ResearchSource {version:number;id:string;title:string;category:string;selectors:SelectorKind[];destinations:string[];methods:string[];transmitted_fields:string[];delivery:Delivery;credential_ref:string|null;max_response_bytes:number;rate_per_second:number;concurrency:number;max_calls:number;health:string;limitation:string;adult:boolean;cache_max_age_seconds:number;profile_rule:ProfileRule|null;}
export interface ProfileRule {source:string;revision:number;request_template:string;public_template:string;syntax:string;methodology:string;max_response_bytes:number;cache_max_age_seconds:number;}
export interface ResearchInputOrigin {selector:Selector;observation_ids:string[];}
export interface ResearchRequest {version:1;selectors:Selector[];sources:string[];rescan:boolean;safe_mode:boolean;options?:ProviderOptions;}
export interface ProviderOptions {paid_authorized:boolean;face_eligible:boolean;premium:boolean;exact_match:boolean;image_category:string;page:number;domain_filter:string;sort_type:string;}
export interface ResearchPlan {parent_import?:ImportParent|null;version:number;generation:number;selectors:Selector[];sources:string[];destinations:string[];request_urls:string[];max_calls:number;rescan:boolean;safe_mode:boolean;limitation:string;provider_options:ProviderOptions;request_digests:string[];input_origins?:ResearchInputOrigin[];image_disclosures:Array<{artifact_id:string;content_hash:string;media_type:string;byte_length:number}>;provider_call_limits:Record<string,number>;provider_credit_upper_bounds:Record<string,number>;}
export interface PreparedResearch {token:string;digest:string;plan:ResearchPlan;}
export interface ResearchResult {id:string;run_id:string;selector:Selector;source:string;outcome:string;url:string;fields:Record<string,unknown>;observed_utc:string;source_reported_utc:string|null;indexed_utc:string|null;artifact_id:string|null;from_cache:boolean;limitation:string;}
export interface ResearchPage<T> {rows:T[];total:number;offset:number;limit:number;}
export interface ResearchRun {id:string;plan:ResearchPlan;status:string;created_utc:string;finished_utc:string|null;expected:number;completed:number;}
export interface ResearchDocument {id:string;revision:number;kind:string;name:string;payload:Record<string,unknown>;archived:boolean;updated_utc:string;}
export interface DirectoryTool {id:string;name:string;category:string;url:string;description:string;status:string;pricing:string;input:string;output:string;best_for:string;opsec:string;opsec_note:string;api:boolean;registration:boolean;local_install:boolean;google_dork:boolean;edit_url:boolean;invitation_only:boolean;deprecated:boolean;}
export interface BulkRow {value:string;kind:SelectorKind;selector:Selector|null;error:string|null;repeats:number;origin_ids?:string[];}
export const researchRegistry=()=>invoke<ResearchSource[]>('research_registry');
export const researchDirectory=()=>invoke<DirectoryTool[]>('research_directory');
export const researchSelector=(kind:SelectorKind,value:string)=>invoke<Selector>('research_selector',{kind,value});
export const researchBulk=(text:string,kind:SelectorKind)=>invoke<BulkRow[]>('research_bulk',{text,kind});
export const researchPrepare=(generation:number,request:ResearchRequest,inputOrigins:ResearchInputOrigin[]=[],parentImport:ImportParent|null=null)=>invoke<PreparedResearch>('research_prepare',{generation,request,inputOrigins,parentImport});
export const researchRuns=(generation:number,offset=0)=>invoke<ResearchPage<ResearchRun>>('research_runs',{generation,offset});
export interface ResearchResultFilter {outcome:string;sources:string[];field?:string;pattern?:string;module?:string;verified?:string;private?:string;min_age?:number|null;max_age?:number|null;date_from?:string;date_to?:string;date_field?:string;}
export interface ResearchResultPage extends ResearchPage<ResearchResult> {run_total:number;source_counts:Record<string,number>;outcome_counts:Record<string,number>;}
export const researchResults=(generation:number,runId:string,offset=0,filter:ResearchResultFilter={outcome:'',sources:[]})=>invoke<ResearchResultPage>('research_results',{generation,runId,offset,filter});
export const researchDocuments=(generation:number,kind:string)=>invoke<ResearchDocument[]>('research_documents',{generation,kind});
export const researchDocumentSave=(generation:number,document:ResearchDocument)=>invoke<ResearchDocument>('research_document_save',{generation,document});

export const researchProviderStatus=(provider:string)=>invoke<{provider:string;credential_account:string;configured:boolean;supported:boolean;usage_state:string;eligibility_state:string;capability_state:string;service:string;storage:string}>('research_provider_status',{provider});
export const researchOpenUrl=(url:string)=>invoke<void>('research_open_url',{url});

export interface ResearchTable {columns:string[];rows:string[][];warnings:string[];}
export const researchTable=(text:string,format:string,header:boolean)=>invoke<ResearchTable>('research_table',{text,format,header});
export const researchTableMap=(table:ResearchTable,column:number,kind:SelectorKind,columns?:number[])=>invoke<BulkRow[]>('research_table_map',{table,column,kind,columns});
export interface ColumnSuggestion {column:number;kind:SelectorKind;valid:number;sampled:number;basis:string;}
export interface CandidatePreview {rows:BulkRow[];warnings:string[];segment_count:number;}
export const researchTableSuggestions=(table:ResearchTable)=>invoke<ColumnSuggestion[]>('research_table_suggestions',{table});
export const researchCandidates=(text:string)=>invoke<CandidatePreview>('research_candidates',{text});
export const researchEvidencePreview=(generation:number,artifactId:string)=>invoke<CandidatePreview>('research_evidence_preview',{generation,artifactId});
export const researchImportPreview=(generation:number,selection:string,format:string,header:boolean)=>invoke<ResearchTable>('research_import_preview',{generation,selection,format,header});

export interface ResearchCandidate {result_id:string;proposed_name:string;proposed_type:string;namespace:string;value:string;source:string;url:string;cited_observation_ids:string[];limitation:string;}
export const researchReviewCandidate=(generation:number,resultId:string,cardIndex?:number)=>invoke<ResearchCandidate>('research_review_candidate',{generation,resultId,cardIndex});

export const researchTableCandidates=(table:ResearchTable,column:number)=>invoke<CandidatePreview>('research_table_candidates',{table,column});

export interface ImportParent {id:string;revision:number;}
export interface ImportSessionState {document:ResearchDocument;submitted_keys:string[];runs:ResearchPage<{id:string;status:string;expected:number;completed:number;created_utc:string}>;}
export const researchImportSession=(generation:number,id:string,offset=0)=>invoke<ImportSessionState>('research_import_session',{generation,id,offset});
export interface LocalResearchHit {subject_kind:string;subject_id:string;title:string;snippet:string;artifact_id:string|null;}
export interface LocalResearch {version:number;scope:string;external_requests:number;offset:number;limit:number;queries:Array<{selector:Selector;status:string;hits:LocalResearchHit[];total:number;coverage?:CoverageView;limitation:string}>;}
export const researchLocalSearch=(generation:number,selectors:Selector[],offset=0)=>invoke<LocalResearch>('research_local_search',{generation,selectors,offset});
export interface ResearchDrop {generation:number;selection:string;name:string;}
export const researchDropListen=(handler:(event:ResearchDrop)=>void)=>listen<ResearchDrop>('research-file-drop',event=>handler(event.payload));

export interface ImageRect {x:number;y:number;width:number;height:number;}
export interface ImageRecipe {version:number;crop:ImageRect|null;quarter_turns:number;mirror_horizontal:boolean;mirror_vertical:boolean;brightness:number;contrast:number;red:number;green:number;blue:number;obscure:ImageRect[];}
export interface ImagePrepareRequest {version:number;artifact_id:string;page:number;recipe:ImageRecipe;selected_face_id:string|null;obscure_other_faces:boolean;}
export interface ImageSearchRequest {version:number;artifact_id:string;page:number;mode:string;max_distance:number;scan_limit:number;offset:number;sort:string;domain:string;date_from:string;date_to:string;title:string;min_bytes:number|null;max_bytes:number|null;}
export interface ImageSearchRow {artifact_id:string;page:number;source_url:string|null;title:string|null;domain:string|null;group:string;media_type:string;byte_length:number;artifact_created_utc:string;indexed_utc:string;publication_utc:null;language:null;exact_bytes:boolean;distance:number|null;signal:string;}
export interface ImageSearchPage {version:number;scope:string;delivery:string;external_requests:number;rows:ImageSearchRow[];total:number;offset:number;limit:number;scanned:number;scan_limit:number;scan_truncated:boolean;eligible_artifacts:number;groups:Record<string,number>;coverage:string;}
export const imageQuerySearch=(generation:number,request:ImageSearchRequest)=>invoke<ImageSearchPage>('image_query_search',{generation,request});
export const imageQueryInfo=(generation:number,artifactId:string)=>invoke<Record<string,unknown>|null>('image_query_info',{generation,artifactId});

export interface ClipboardImageRequest {version:number;media_type:string;base64:string;}
