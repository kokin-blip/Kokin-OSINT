# Increment 35 — optional local AI and encrypted semantic retrieval

2026-10-14. All previous local work and Night Ledger workflows are retained.

The AI destination provides optional loopback runtime inspection, reviewed GGUF
resource installation/native import, local Ollama blob/create import (no remote
pull), cited summaries/entity/relationship candidates, paginated history and human
review explanations. Reviewed candidates open the existing grounded decision
forms; reviewed summaries enter editable cited report drafts. Reviewing a suggestion
creates no entity, relationship, merge or assessment. The seven dimensions remain
separate. Case jobs use generation/cancellation checks and reverify citations at
commit; global resource/runtime installation survives case navigation/close.

Hardware reads report CPU, RAM/storage and advertised GPU/Metal capability; Apple
unified memory is distinct from dedicated VRAM. Profiles require twenty synthetic
samples, valid citation output, measured memory with a 6 GiB reserve, current model
artifact/manifest identity, hardware/runtime identity and task compatibility.
Balanced has a 30-second p95 budget; alternatives expose latency/quality tradeoffs.
No runtime/model is installed silently and no unmeasured recommendation is made.
**Ollama is absent on this Mac by the owner's choice.** The real Qwen generations,
accelerator execution, imports and model benchmarks remain unverified; transport
and review are verified with a clearly labelled synthetic native fixture.

Pinned bge-small int8 and tokenizer files were installed through the approved
native opt-in flow (33.1 MiB, outside case packages). Bundled ONNX Runtime 1.22.0
uses safe wrappers, CPU fallback, two intra-op threads and telemetry disabled;
model/binary-fetch features are disabled. The tokenizer is local. CLS vectors are
normalized, 384-dimensional, encrypted in the existing SQLCipher instance and
rebuildable. ADR-0018 isolates only the typed sqlite-vec initialization FFI; all
application crates retain `unsafe_code = forbid` and no arbitrary extension loading.

The real desktop retrieval gate passed **8/8** synthetic paraphrase queries versus
**0/8** through the existing lexical query compiler. Semantic/hybrid remain
unavailable before this gate; lexical remains default. Retrieval is bounded to
1,000 nearest observations, 200 lexical rows and 256 linked records per observation,
with explicit candidate totals/truncation, kind/facet/withdrawal filters, canonical
entity grouping and original evidence navigation. English/first-512-token scope,
unindexed observations and unread evidence remain explicit. Similarity is not
confidence. Pipeline identity changes require rebuilding.

The release-mode 100,000 × 384 synthetic encrypted-vector benchmark matched exact
reference top-10 results for all five queries (recall 1.0). Thirty warm top-50 queries
measured p50 227.3 ms, p95 315.7 ms, p99 353.2 ms on this M4 Pro/24 GiB Mac. These
are vector backend results, not lexical p95, cold disk, concurrent-write performance,
production FPS or input latency. The previously scoped merged lexical measurements
remain recorded separately in increment 32.

Verification: encrypted suggestion/review/vector package restore; no automatic
analytical writes; wrong IDs/incomplete output/injection containment; resource
size/hash/cancellation checks; native AI review and human form opening; actual BGE
installation/rebuild/semantic/hybrid retrieval. Full Rust 1.85.1 suite and UI/regression
results are recorded with the final map report. No Ollama/Qwen weights downloaded.

Dependency gates now pass: patched MapLibre, scoped approved CA-root data notice,
and guarded Rust-1.85 security compatibility sources (ADR-0019/0020). The time
version-only advisory exception is conditional on exact source/lock guards and a
bounded deep-comment regression, not approval of an unfixed vulnerability.

Translation and challenge-assistant reports conclude **defer** with adoption gates.
Windows, manual VoiceOver and visible production timing remain deferred. No release
publication or PR #1 adoption is included.
