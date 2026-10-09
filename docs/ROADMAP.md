# Authoritative roadmap

Updated 2026-10-08. This roadmap reconstructs the remaining scope from accepted ADRs,
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
| Evidence, Entities, Search, Activity workbench | Implemented | Navigation; typed paginated reads and own-evidence detail tests |
| Seven dimensions, evidence-grounded decisions, merge/reject/split | Implemented | Record analysis; graph/write/read acceptance tests |
| Export, verify and restore | Implemented | Case actions and closed-case restore; export round-trip tests |
| Merge-aware search partition | Implemented | Search/count/facet equivalence tests; performance gate remains separately measured |
| Desktop automation | Implemented; Mac verified | Debug-only WDIO lifecycle and eleven-step workflow repeat passed; exact persisted identities/lineage survive reopen/restore. Windows run remains pending |
| Graph exploration (increment 30) | Implemented | Entity detail → Explore relationships; bounded/cancellable indexed expansion, visible truncation, evidence-linked nodes/edges and list alternative |
| PDF/image evidence (increment 31) | Planned | PDF locations, EXIF, thumbnails, hashes; optional OCR page/region citations |
| Public-source connectors | Planned | Validated manifests/capabilities, disclosure, cancellation/fixtures/health; DNS/RDAP then crt.sh then Wayback |
| Hardware-aware local AI and semantic search | Planned | Optional Ollama, reviewed pinned models, opt-in installation, encrypted rebuildable embeddings, human-accepted cited suggestions |
| Offline maps | Planned | MapLibre/local PMTiles, attribution, uncertainty, explicit basemap download, zero tile egress |
| Public-source pivots, translation/timelines/image similarity/challenge assistant | Proposed | Separate feasibility reports after core gate |
| Browser capture, import-only interoperability, external handoff | Proposed | Isolated containment/licensing/egress probes; Docker alone is insufficient |
| Deepfake detection | Proposed | Experimental until representative measurements; no invented authenticity percentage |
| PR #1 adoption or licensing change | Proposed | Separate decision record and scope review; existing policy retained |

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
