# Testing strategy

Updated 2026-10-08. The default Rust and frontend suites are deterministic and
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
