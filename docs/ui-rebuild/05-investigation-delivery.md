# Guided investigation delivery

Accepted 2026-10-12. Preserve all local Night Ledger work. Windows and manual VoiceOver checks remain deferred. Core stages must pass before expansion ships.

| Existing capability | Replacement / parity requirement |
|---|---|
| Encrypted create/open/recovery/default path | Case gate plus optional introduction; recovery acknowledgement never bypassed |
| Import/explicit and batch extraction/static collection | Evidence actions backed by a case-scoped operation controller; native tokens, disclosure, offline default preserved |
| Entity/identifier/relationship writes | Contextual forms and evidence/record pickers; manual-reference advanced path retained |
| Assessments/merge/reject/split | Contextual assessment and advanced decisions; all seven dimensions, factors and actors preserved |
| Entity/relationship/identifier/observation/source reads | Shared detail history and graph inspector; opposing/supporting evidence and alias provenance preserved |
| Search/coverage/paging | Dedicated Search; explicit submission and unread-evidence caveat retained |
| Audit/verification | Activity with chain disclaimer retained |
| Bounded graph | Dedicated Graph with encrypted named boards, multi-seed expansion, node/edge budgets, filters, saved camera/layout |
| Package/export/verify/restore | Dedicated Export plus encrypted report drafts and escaped Markdown/HTML export |
| Settings/palette/layout/motion | Existing tokens/preferences/shortcuts retained; commands extended only for operational features |

Sequence: case-scoped operations → six destinations → graph and named boards → contextual writes/pickers → optional real-case guide → cited reports/package workspace → Mac reliability/performance gate. Append migrations; never change shipped SQL or evidence/assessment history. Revision checks refuse stale saves. Case data never enters display preferences.

Expansion order after the core gate: PDF/images/OCR → DNS/RDAP, crt.sh, Wayback → optional hardware-aware AI/semantic search → local PMTiles maps. Feasibility tracks remain separate adopt/defer/reject reports. Existing dependency-license and egress policies stay intact. Do not describe a planned module as operational.

## Implementation status

Core stages 1–6 have working entry points and contract/desktop proof recorded in
[the increment report](../increments/ui-guided-investigation.md). The release
gate remains open: visible production checks require an unlocked Mac; maximum
board/large-case profiling and the full fault matrix remain. Windows and manual
VoiceOver are deferred. No expansion is advertised as operational or shipped.

## Command and analytical-field parity

| Registered command(s) | Current replacement / preservation |
|---|---|
| `storage_encryption_status` | Startup SQLCipher status and failure notice |
| `create_case`, `open_case`, `open_case_with_recovery_key`, `close_case`, `session_status` | Case gate/recovery acknowledgement/close; automatic blank-location behavior retained |
| `artifact_page`, `observation_page`, `artifact_document`, `observation`, `evidence_target` | Evidence pagination, artifact detail, observation/source inspector and evidence history |
| `artifacts`, `artifact_observations` | Legacy read APIs retained; normal lists use paginated counterparts |
| `entity_page`, `entity`, `relationships`, `subject` | Entities, canonical/alias dossier, paginated relationships, identifier/relationship own evidence |
| `entities` | Legacy capped API retained; pickers and library use paginated reads |
| `search`, `coverage` | Explicit Search, filters/paging and coverage warning, including nonempty results |
| `activity` | Paginated history, subject links and chain-verification disclaimer |
| `entity_types`, `scales` | Backend-provided type and all seven scale choices in contextual forms |
| `extract` | Direct compatible command retained; common UI uses case-scoped `extract`/batch operations |
| `create_entity`, `add_identifier`, `relate`, `assess` | Direct commands retained; contextual forms dispatch the same backend writes through operation ownership |
| `merge_entities`, `reject_merge`, `split_entity` | Direct commands retained; advanced merge preview/confirmation, rejection and split operations |
| `select_path` | Rust-owned generation-bound import/package/restore/report save tokens; no arbitrary import path |
| `start_operation`, `operation_status`, `cancel_operation` | Persistent controller, progress/cancellation/error results and late-response guards |
| `network_mode`, `set_network_mode` | Offline initial state, explicit enable/disclosure/consent; settings controls retained |
| `workspace_page`, `workspace_read`, `workspace_save` | Additive encrypted board/report/guide metadata and revision checks |
| `guide_status` | Successful backend counts/coverage feed checklist completion |
| `test_control` | Debug desktop fixture only, excluded from production |

Operation kinds retain import, collect, extract, graph, create_entity,
add_identifier, relate, assess, merge, reject, split, export, verify and restore.
Batch extraction serializes supported-document extract jobs in the controller.
Additive kinds are graph_explore, add_evidence, report_preview and report_export.

Analytical fields retained: entity type/name/notes; identifier kind/value; relationship
from/to/kind/start/end; selected subject identity; dimension/value/factors/actor;
merge absorbed/survivor/effects/confirmation/rationale; reject pair/rationale;
split absorbed identifier/rationale; manual document/capture/observation references.
Evidence roles remain `supports`, `contradicts` and `context`. Pickers show real
observations/locators/source excerpts before adding a reference.

Confidence dimensions remain extraction certainty, source reliability, identifier
match, relationship confidence, temporal consistency, geospatial precision and
review status. Individual values, explanations, actors, disagreements and stored
record attribution remain accessible; no total or inferred confidence is added.
