# Authoritative roadmap

Updated 2026-10-14. This roadmap reconstructs the remaining scope from accepted ADRs,
increment reports and the eleven-step reference workflow. External master-plan
files were unavailable. Historical reports describe their original increments;
this file and the new increment report describe current capabilities.

Status vocabulary: **implemented** means code and a desktop entry point exist;
verification scope is recorded separately. **Backend only** has no desktop entry
point. **Planned** follows accepted architecture. **Proposed** needs a separate
feasibility/decision record. No status is a release certification.

| Capability | Status | Entry point / evidence |
|---|---|---|
| Encrypted cases and recovery-key acknowledgement | Implemented | Case gate; lifecycle and recovery-key tests |
| Native local import, explicit HTML extraction | Implemented | Case toolbar; selection and ingest/extract contracts |
| Static public HTTP collection, offline default | Implemented | Collect a public page; net policy tests; live-service verification remains explicit |
| Recording transport | Backend only | Explicit `RecordingHttp`; synthetic raw-byte replay test; no automatic fixture commit |
| Evidence, Entities, Search, Activity, Graph, Export workbench | Implemented | Navigation; typed paginated reads and own-evidence detail tests |
| Seven dimensions, evidence-grounded decisions, merge/reject/split | Implemented | Record analysis; graph/write/read acceptance tests |
| Export, verify and restore | Implemented | Case actions and closed-case restore; export round-trip tests |
| Merge-aware search partition | Implemented | Search/count/facet equivalence tests; performance gate remains separately measured |
| Desktop automation | Implemented; Mac verified | Debug-only WDIO lifecycle and eleven-step workflow repeat passed; exact persisted identities/lineage survive reopen/restore. Windows run remains pending |
| Graph exploration and encrypted boards | Implemented | Graph destination; multiple roots, bounded shared budgets, direction/kind filters, saved camera/positions/pins and real-evidence inspector |
| Optional real-case onboarding | Implemented | Introduction plus case-encrypted checklist; recovery acknowledgement and collection consent preserved |
| Cited report drafts and readable exports | Implemented | Export destination; revision checks, real case citations, graph illustrations, escaped Markdown/self-contained HTML; PDF deferred |
| PDF/image evidence (increment 31) | Implemented; acceptance scope bounded | Evidence inspector and explicit media jobs; PDF character locations, EXIF, encrypted previews/hashes, isolated native worker, opt-in English OCR. See [increment 33](increments/33-media.md) for tested limits and remaining packaging work |
| Public-source connectors | Implemented; Mac offline fixtures verified | Sources disclosure and raw response capture; DNS/RDAP, crt.sh, paginated Wayback discovery and separate static capture; [increment 34](increments/34-public-sources.md). Live provider health remains unverified |
| Face comparisons and related discovery | Implemented; optional native/model setup | Image/PDF/video sampling, exact case search, suggested groups and reviewed links; text-only local SearXNG and browser handoff. [Setup, bounds and acceptance](increments/37-face-search.md) |
| Hardware-aware local AI and semantic search | Implemented; optional runtime gates explicit | AI setup/import, case-bound cited jobs/review, hardware reads and measured-profile controls; real bge-small int8 quality gate and encrypted semantic/hybrid retrieval pass. Ollama/Qwen execution and model benchmarks unverified because Ollama is absent; [increment 35](increments/35-local-ai-and-semantic.md) |
| Offline maps | Implemented; resource/live scope explicit | Maps, native PMTiles import, bounded world z0–z6 installation, grounded locations, encrypted camera and cited illustrations. Offline rendered tiles and zero observed egress verified; live metadata size review passed. [Increment 36](increments/36-offline-maps.md) |
| Public-source pivots, translation/timelines/image similarity/challenge assistant | Proposed | Separate feasibility reports after core gate |
| Browser capture, import-only interoperability, external handoff | Proposed | Isolated containment/licensing/egress probes; Docker alone is insufficient |
| Deepfake detection | Proposed | Experimental until representative measurements; no invented authenticity percentage |
| PR #1 adoption or licensing change | Proposed | Separate decision record and scope review; existing policy retained |

Maximum-board, fault and merged-search scope is recorded in [increment 32](increments/32-reliability-and-expansion.md). Windows, manual VoiceOver and visible production timing remain explicitly deferred and do not block the authorized expansion sequence.

## Core release gate

The first implementation slice adds workbench/search/activity, native selections,
background operations, analytical writes, live transport and package controls.
It is **not** a tagged personal release. Required next work:

1. Run the eleven-step desktop repeat on Windows. The actual Mac passed both
   specs, including offline replay, analysis and exact export/restore identity/lineage
   comparison; see [increment 30 report](increments/30-graph-and-desktop-acceptance.md).
2. Exercise interrupted imports, disk-full, cancellation, corrupt bytes/packages,
   wrong credentials/recovery typos and failed migrations. Existing contract tests
   cover several of these, but integrated fault injection is not complete.
3. Confirm merged-case p95 ≤ 200 ms on the documented reference setup; publish
   warm, cold-cache and concurrent-write measurements with their actual scope.
4. Verify dialogs, keyboard/focus, scaling/narrow windows, installation/launch and
   recovery on the actual Mac and Windows. CI compilation is not manual acceptance.
5. Verify production dependency trees/assets exclude test drivers, run boundary,
   SQLCipher, citation, markup and dependency checks, then tag a personal-use build
   with changelog and known unsigned/unnotarized limitations.

## Expansion contracts

Graph development began alongside desktop acceptance at the user’s explicit request.
The remaining production expansion order is media, connectors, AI, then maps;
shipping remains subject to the core release gate. Persist new facts only through append-only migrations;
derived indexes must remain rebuildable. Never rewrite historical observations
or assessments. No layout, ranking or similarity value becomes confidence.

Model selection detects architecture, RAM/free storage and supported GPU/runtime
memory, treating Apple unified memory differently from dedicated VRAM. Detect
installed Ollama models and retain CPU fallback. A reviewed registry records
pinned identity, license, size, task compatibility and synthetic benchmarks.
Recommend the best validated candidate inside the balanced latency/memory budget;
offer Faster and Higher quality overrides with resource estimates. Show download
size and destination before opt-in. Reassess only explicitly or after model/hardware
changes or repeated resource failures; never silently download a larger model.
Inference uses a dedicated configured-loopback capability, without weakening
collection SSRF policy. Begin embeddings with the previously approved candidate
and measure retrieval against lexical search before enabling semantic search.
AI outputs remain cited suggestions, never automatic assessments/merges/relationships.

Produce separate public-source, media/local-AI, and crawler/external-tool feasibility
reports after the core gate. Each concludes adopt/defer/reject with measured evidence,
implementation cost and acceptance criteria. Reports cannot silently change policy.

## Guided investigation continuation

See [delivery/parity matrix](ui-rebuild/05-investigation-delivery.md) and
[implementation evidence](increments/ui-guided-investigation.md). The six core
workspaces are implemented; the Mac production interaction/maximum-board gate
remains open. Windows and manual VoiceOver checks are explicitly deferred.
The earlier statement that media, public connectors, local AI/semantic search and maps remained planned is superseded by increments 33–37: those workspaces and native workers exist. Their remaining acceptance limits are recorded in those increment reports. The new research expansion is tracked separately in [the expansion status](expansion/implementation-status.md) and [142-entry matrix](expansion/feature-matrix.md); incomplete phases must not be advertised as operational. No automatic model download is introduced.
