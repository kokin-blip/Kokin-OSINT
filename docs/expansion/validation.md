# Expansion validation — 2026-10-20

This records a partial implementation, not the complete release gate. Machine-readable commands and log hashes are in [validation.json](validation.json).

## Baseline and preservation

Before changing infrastructure, the existing Rust workspace/all-target suite, frontend type check and 70 frontend tests passed against schema 13. The original HEAD, tracked binary patch, NUL-delimited Git status and hashes of existing untracked files are saved beside this document. No preexisting untracked file was removed. Existing faces/media/AI/maps work remains in place. Small existing clippy issues were corrected without changing behavior; the 14 files flagged by the existing formatter were copied to the local backup recorded in pre-format.json before formatting.

## Passed

| Check | Evidence and scope |
|---|---|
| Rust regression | 420 tests across 49 test executables, zero failures; 13 explicitly ignored native/transport gates listed in the log. Two owned-loopback gates were subsequently run separately and passed. |
| Final connector contracts | 13/13, including conservative DNS/RDAP/CT classification added after the full regression. |
| Final network contracts | 59/59, including credential-echo withholding, cancellation, shared admission and URL handoff validation. |
| Frontend | 74/74 tests across 17 files; type check has zero errors/warnings. |
| macOS research acceptance | Synthetic case: complete catalog page, three independent source captures, blocked-source distinction, cited evidence, definitive-only cache reuse, collections and consented atomic JSON export. Screenshots reviewed. Test-only WebdriverIO 10 compatibility shim; real Tauri UI/commands, synthetic network fixtures. |
| Persistence and evidence | Encrypted case bytes, versioned JSON observations/quotes, eight-hour cache age, authenticated cache rejection after ciphertext corruption, immutable result/revision records, package restore and source-shred hiding. |
| Authorization | Single-use/expiry/generation/digest tests; provider options and outgoing request fingerprints are bound; image metadata records exact transmitted-body hash without retaining base64 upload bytes in immutable request JSON. |
| Performance | 10,000 synthetic stored findings over ten runs; bounded 50-result paging and selected projection. Seed plus queries measured 327 ms in in-memory SQLite on aarch64. This excludes encrypted-disk/network/model latency. Full directory UI mounts 50 list entries; 20,000-row import UI mounts 50 preview rows and selects at most 15. |
| Static boundaries | Egress scan outside kokin-net clean; fixture-secret scan clean; SQL ordering, research citations, internal references and dependency-source patch guards pass. |
| Build hygiene | Production frontend builds. Default production dependency tree and frontend assets contain no test-driver matches. Rust fmt, strict clippy and whitespace checks pass. |
| Catalog provenance | Exact pinned MIT Git blob ff4107ba8904f04588636ccee1f61ef1e7540dbc verified, with attribution and notice. |

## Remaining gates

- `cargo deny` is unavailable in this environment; the license/advisory CI command was not claimed as passed. The only new direct dependency use is already-resolved permissive `zeroize`; the MIT catalog has its exact notice. Further native/model/renderer dependencies remain unselected.
- The full suite intentionally leaves 13 native/process/explicit-runtime tests ignored. The owned-loopback inference and SearXNG tests were run separately. Other native-model/resource/process tests retain their existing dedicated gates; this change did not download models or revalidate every packaged engine.
- Live provider entitlements, usage/credit contracts, module normalization, streaming and paid checks remain incomplete. No paid request, image upload, credential creation or provider purchase was performed.
- Windows runtime, complete keyboard/focus/VoiceOver/light-theme/narrow-window work and production packaging remain open. Vite retains the existing large-chunk advisory for the app bundle.
- All later expansion phases and partial feature clauses remain explicit in [implementation-status.md](implementation-status.md) and [feature-matrix.md](feature-matrix.md).

Mac screenshots: [directory](screenshots/directory-mac.png), [results](screenshots/research-results-mac.png). Native model measurements remain in the existing face benchmark; this slice adds no new model or accuracy claim.

## Phase 1 continuation acceptance

The continuation preserved the existing tracked diff and 653 untracked file hashes in `continuation-2026-10-20/`. The final offline regression passes **423 Rust tests across 49 executables**, with 13 dedicated native/transport gates ignored; **76 frontend tests** and the type check pass. Strict workspace clippy, formatting, reference/SQL/citation/dependency guards pass. The macOS synthetic test verifies whole-run filters and counts, private notes, post-removal focus return, slash focus, both themes and a 900-point viewport without horizontal overflow. Profile recipes reject unrecognized 404 bodies, soft 404s, redirects, wrong identities and oversized responses. Cache policies may shorten but cannot exceed eight hours; recipe changes invalidate reuse. No live provider request was made. See [phase-1-validation.json](phase-1-validation.json) for exact log hashes and [the scope contract](phase-1-contract.md) for all retained/excluded clauses.

## Phase 2 hybrid selector/bulk acceptance

The full offline workspace regression passed **448 tests across51 executables**, with13 dedicated runtime gates ignored. Subsequent focused native-drop and expanded encrypted-package/provider-card acceptance passed, and strict workspace clippy passed on final sources. **87 frontend tests** and the frontend type check pass.

The macOS synthetic acceptance passed in **6 seconds**, covering the native local fallback with zero external requests, CSV/TSV/JSON/text/Markdown/log file previews, real contained PDF extraction, unselected authenticated candidates, saved/resumed import workspaces, parent authorization, and the earlier directory/theme/focus/source/cache/review/export paths. The test-only bundle enables the WDIO global bridge through a CLI config override; production configuration is unchanged. The production frontend was rebuilt afterward to remove test-driver assets.

Two separately invoked contained native worker tests passed using the matching packaged worker/native directory and existing hash-verified English data. They cover image processing/cancellation and real PDF text, OCR, malformed input and page bounds. Earlier sandbox/wrong-engine-directory test attempts correctly refused execution; the accepted matching contained configuration is recorded in the final receipt. No engine/data was installed or downloaded. The OS drag gesture and manual VoiceOver remain manual release QA; native drop token constraints and frontend event review are tested without claiming those gestures were performed.

The encrypted SQLCipher disk benchmark inserted10,000 synthetic projections across ten bounded runs in260ms and filtered/paged one1,000-result run in18ms. It excludes network, model inference and end-to-end evidence capture; see [the exact measurement](../benchmarks/research-phase2-encrypted-disk-mac.json). JSON parsing/classification/card projection now run outside the serialized source commit; blob/SQL publication remains serialized.

All690 preexisting untracked files in this continuation baseline remain present. Boundary scans found no egress outside kokin-net, default production driver dependency or production frontend driver marker. Matrix IDs remain142 unique original entries. No paid provider request, image upload, credential creation or model download occurred. Vite retains its existing large-chunk advisory; cargo-deny remains unavailable locally.

The Phase2 selector/bulk slice is implemented within [its contract](phase-2-contract.md). Unsupported automated breach lookup remains blocked, complete provider live/stream/eligibility gates remain Phase4, and cross-source graph/timeline presentation remains Phase5. That receipt predates the current Phase 3 continuation. See [the acceptance receipt](phase-2-validation.json), [boundaries](phase-2-boundaries.json), [hybrid API/REA basis](phase-2-hybrid.md) and [the honest142-entry matrix](feature-matrix.md).

## Phase 3 local image continuation

Phase 3 remains in progress. This slice adds contained native query preparation, encrypted L1 recipes and lineage, clipboard Evidence capture, source-key revocation for nested derivatives, and case-scoped exact/pHash candidate search. Existing file/drop and explicitly authorized broker collection are reused. Search never uploads a query, expands actual scan budgets and returns at most 50 candidates per page. Source/publication/index timestamps and unknown metadata remain distinct.

Focused storage tests cover original preservation, current/stale/wrong-page face selection, recipe mismatch refusal, authenticatable recipe observations, package/restore, descendant shredding and 10,001 synthetic candidates. Native tests separately cover real pixel geometry/color, decoder bounds, opaque regions, PDF/OCR, malformed inputs and cancellation. Automated macOS acceptance uses the packaged app and synthetic evidence; no paid lookup, image upload, model download or credential creation is involved. Actual OS clipboard/drop gestures, VoiceOver, broader formats and Windows remain separate release gates.

Commands, actual results and log hashes are recorded in [phase-3-validation.json](phase-3-validation.json). [The contract](phase-3-contract.md) and [matrix](feature-matrix.md) retain semantic categories/CLIP, HEIC/AVIF, C2PA, representative detector evaluation and remaining category/filter work as pending. Earlier-phase receipts remain historical and are not rewritten as new acceptance.
