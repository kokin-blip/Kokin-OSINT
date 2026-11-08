# Testing strategy

Updated 2026-10-12. The default Rust and frontend suites are deterministic and
offline. Replay misses fail; they never become live calls. Set `KOKIN_NETWORK=deny`
for backend and desktop fixture runs. Live-service tests and scale probes are
explicit, separate checks.

## Current verification layers

- Rust contracts: encrypted SQLCipher/blob round-trips, tampering, incorrect
  credentials, migration failure, evidence grounding, seven separate confidence
  dimensions, reversible resolution, extraction/citation validation, search
  paging/canonical totals/facets and package omission/integrity/restore behavior.
- Command/read contracts: case generation rejects stale writes; paginated lists
  carry totals; relationship/identifier reads carry their own supporting/opposing
  evidence; native-selection tokens expire, are single-use and case-bound, and
  revalidate metadata. These do not automate native dialogs.
- Frontend components: recovery-key gate, text-only document states and entity
  confidence rendering. Workbench tests now cover paging beyond caps and coverage on nonempty search results; broader analytical-flow regression coverage remains needed.
- Desktop: `ui/tests/desktop/lifecycle.spec.mjs` and `workflow.spec.mjs`, using
  the WDIO embedded driver on a separate debug-only build. The full workflow
  uses replay-only collection and picker-selection instrumentation, then compares
  persisted identities/lineage after reopen and restore and repeats in a fresh case.
  Actual native dialog interaction remains a separate bounded manual check.
- Graph: indexed bounded/cancellable expansion with stable ordering, explicit
  truncation, preserved evidence attribution and UI evidence navigation checks.
- Deliberate scale: `src-tauri/tests/scale.rs`; distinguish warm, evicted SQLite
  cache and concurrent writes. Eviction is not operating-system cold cache.

## Commands

```sh
KOKIN_NETWORK=deny cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
npm --prefix ui ci
npm --prefix ui run check
npm --prefix ui test
npm --prefix ui run build
```

Run the existing CI boundary/fixture/markup checks and `cargo deny check` as well.
Dependency review includes optional desktop tooling; release trees must exclude
it. Sources must not add network clients outside `kokin-net`.

An explicit `.github/workflows/desktop.yml` runs desktop acceptance on Windows
and macOS; it has not been dispatched during this implementation.

For desktop testing build with `VITE_DESKTOP_TEST=1`, `--debug --features
desktop-test --config src-tauri/desktop-test.conf.json`, then run `npm run
test:desktop` from `ui`. The test config grants global Tauri access/driver
permissions exclusively to this build. Release compilation with the feature is
an error; normal frontend production builds eliminate the driver import.

The actual Mac passed both desktop specs on 2026-10-08. The full eleven-step
workflow repeats offline and compares exact persisted identities/lineage after
reopen/restore. Windows execution remains pending. See increment 30.

## Release checks still required

Windows execution of the eleven-step desktop repeat and restored identity/lineage
checks, integrated cancellation/disk-full/interrupted import,
manual Mac and Windows dialog/focus/scaling/launch/recovery checks, and explicit
production driver exclusion. Do not label backend acceptance as desktop acceptance.

## Guided workspace verification

Contextual desktop acceptance imports actual local HTML, extracts an observation
and creates two grounded entities and a relationship through record pickers.
The reference workflow additionally saves a graph board and report, exports
escaped HTML with an illustration, reopens, then compares exact board/report
payloads after package restore. Native picker paths are queued only in the
desktop-test build. `KOKIN_STATIC_UI_ONLY=1` disables finite animation in that
functional test harness; it never establishes motion or performance acceptance.

New contracts cover workspace CAS revisions/append-only history, disk-full saves,
failed migration rollback, alias-preserving graph expansion, malicious report
markup, metadata roundtrip and late operations/native selections after case close.
A visible production run and maximum-board measurements remain required.
Windows and manual VoiceOver are deferred by the user.

The reconciled exact dev dependency lock passes clean install. npm reports nine
high advisories in the WDIO/Puppeteer development ZIP extraction chain; the root
extract-zip advisories have no available patch. This is an open tooling limitation,
not a clean audit. These packages and the embedded driver are excluded from
production UI assets. Avoid untrusted archive inputs to this developer tooling.

## AI and map acceptance (2026-10-14)

The final owned Mac desktop test bundle passes six specs: core workflow repeated
offline with exact restore identities, contextual analysis, media, connector fixtures,
AI human review/maps, and real installed bge-small semantic retrieval. Synthetic
inference proves review/control contracts; it does not benchmark real Ollama quality.
Real bge-small retrieval scores 8/8 against lexical 0/8 on the documented synthetic
quality corpus. Lexical remains default. The 100,000 × 384 vector exact-baseline
check has recall 1.0 for five queries; warm top-50 p95 is 315.7 ms (not the separate
lexical 200 ms target). See increments 35–36 for measurement scope.

The world-resource spec is opt-in: KOKIN_REVIEW_WORLD=1 reviews metadata only;
KOKIN_INSTALL_WORLD=1 also exercises disclosed tile installation. Public-resource
checks stay separate from the deterministic network-denied default suite.

Rust 1.85.1 suite: 387 passed, 12 intentionally ignored; strict Clippy passes. UI:
63 tests passed, zero check errors/warnings. Dependency/backport guards, citation,
SQL ordering, cross-references and research checks pass. Production npm dependencies
have zero advisories; development ZIP-tooling limitations above remain explicit.
Maximum-board/fault evidence is recorded in increment 32. Windows, manual VoiceOver
and visible production timing remain deferred rather than reported as passed.

The approved world-basemap transfer and local reuse acceptance also passed. The
installed archive is retained for ordinary app use; synthetic global test archives
are cleaned. Native resource hashes and visible attribution were verified; map use
recorded zero remote fetch/XHR requests. Test-only frame scheduling is explicitly
excluded from any production timing claims.

Use `KOKIN_NETWORK=deny KOKIN_WORLD_OFFLINE=1 KOKIN_INSTALL_WORLD=1
KOKIN_WORLD_REUSE=1` for the installed-world-only acceptance path. It performs no
metadata or download requests and passed actual vector rendering/export.
