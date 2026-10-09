# Core workflow implementation slice (increments 25–26)

Date: 2026-10-08. Personal-use pre-alpha; release gate remains open.

## Changes

The desktop replaces stacked read panels with Evidence, Entities, Search and
Activity navigation and shared details/back history. Artifact/entity/observation,
relationship and activity reads have typed page metadata. Search is explicitly
submitted with filters, paging and coverage even on nonempty results. Identifier
and relationship details show their own supporting/opposing evidence and all
seven dimensions. Activity displays verification and its tamper-evident limit.

Native evidence/package dialogs issue expiring, single-use, generation-bound
selection tokens. The arbitrary-path import IPC command is removed. Tokens
canonicalize input paths and revalidate metadata; underlying ingestion still
refuses case self-ingestion. Import/extraction, static collection, analyst writes
and package operations use background jobs. Network I/O finishes before acquiring
the case mutex; writes are serialized and guarded by generation. Closing disables
online mode, invalidates selections and cancels outstanding work. Once an atomic
write begins its cancellation control is disabled and its actual result is reported.

`kokin-net` owns LiveHttp and explicit RecordingHttp. Live GETs vet/pin public
addresses with TLS hostname checks, redirect revalidation, no inherited proxy,
time/rate/response limits and offline process policy. Captures preserve raw bytes
and redirect metadata, labeled static_html_only. Recording scrubs transport
headers/refuses credential/query/secret patterns, emits versioned fixtures and
never commits them automatically.

Analyst controls create entities, identifiers and relationships with citations;
record per-dimension assessments with explanations; preview/confirm merges;
reject or split. Package controls disclose omissions, missing recovery key and
absence of signature. Export publishes no-clobber after assembly; restore verifies
and stages before publication, validates credentials without adding a hidden open
event, and cleans failed stages. A closed-case restore entry point is available.

Search partitions merge-affected entity rows for the alias window; hits/totals/
facets use the same representative rules. Result snippets are delayed until after
page selection. Earlier intermediate measurements failed the performance target;
see the benchmark report for final measurements and scope.

## Verification and limitations

Rust contracts include new selection expiry/change/no-clobber cases, generation
rejection after reopening, paginated reads, own-evidence details and failed-restore
cleanup. Frontend suite adds paging and nonempty-result coverage/history tests.
WDIO lifecycle/navigation automation uses optional debug-only Rust plugins, a
separate configuration and a frontend import removed from normal builds. Release
compilation explicitly refuses desktop-test. Full desktop reference automation,
fixture collection instrumentation and integrated fault injection remain open.

Dialogs and actual app interaction still require bounded Mac and Windows checks.
Case folder entry is typed. Progress currently describes operation phases rather
than byte percentages. Cancellation stops collection before commit; atomic import,
extraction/export stages finish rather than rolling back halfway through writes.
Batch extraction is HTML-only and reports unsupported coverage through artifacts;
its stop-after-current control reports unprocessed remainder explicitly. Filesystem metadata
checks cannot prove immunity to every hostile-local-writer race. Restore checks
for an existing destination before rename; cross-process races need fault probes.

Production expansion has not begun: graph, media, connectors, optional AI and maps
remain gated by reliability and personal-release acceptance. No license/PR #1
policy change, public-release tag or notarization is claimed.

The embedded Mac desktop lifecycle/navigation/reopen test passed on 2026-10-08.
The case gate was visually inspected. Native dialog and populated workflow
acceptance remain open; no Windows manual verification is claimed.

Local debug merged-search p95 was 151 ms warm / 152 ms after SQLite-cache
 eviction / 275 ms with concurrent audit writes. Reference-machine and true
 cold-disk acceptance remain open; see `docs/benchmarks/core-search-2026-10-08.md`.
Production dependency-tree inspection and normal frontend asset inspection found
no WDIO driver. The optional test build compiled and its Mac lifecycle test passed.
The dependency policy is unchanged. Patched basic-ftp is pinned in test tooling;
seven npm high-severity reports remain in the test runner's unpatched archive
helper/transitive dependents. Avoid runner-managed external browser/driver
archive downloads; review/replace that helper before completing the release gate.

Bounded Mac inspection additionally verified native file selection, synthetic
HTML import, explicit extraction, and observation-to-source-byte lineage in the
shared detail area. The remaining manual acceptance scope is still open.

Final deterministic verification: 342 Rust tests and 28 frontend tests passed;
Svelte reported zero errors/warnings, frontend production build passed, Rust
formatting and Clippy passed, and the Mac embedded-driver lifecycle test passed.
Network-boundary and no-markup/source binding checks passed. `cargo-deny` was
not available locally; its existing CI check remains required.

A subsequent run after compilation passed the warm p95 target at 179 ms; evicted
SQLite cache measured 157 ms and concurrent audit appends 157 ms. Both this run
and the earlier 275 ms concurrent tail are retained in the benchmark report.
The explicit desktop CI workflow now builds debug-only instrumentation and runs
the lifecycle check on macOS and Windows. It has not been dispatched here.
