# Testing strategy

The spec requires that live-source tests be clearly separated from
deterministic suites. This document states how, and what CI enforces today.

## The rule

**The entire default test suite runs with no network access.** CI sets
`KOKIN_NETWORK=deny`. A test that needs a live service is not a slow test — it
is a different category of test, and it does not run by default.

This is not only a speed or flakiness concern. A test suite that silently
reaches the network cannot prove the product's central privacy claim, because
it cannot distinguish "offline mode works" from "offline mode happens to not
have been exercised".

## Categories

| Category | Runs by default | Network | Where |
|---|---|---|---|
| Unit | yes | none | alongside the code |
| Connector contract | yes | recorded fixtures via `ReplayHttp` | `tests/` per connector |
| Parser regression | yes | none | `tests/fixtures/` corpora |
| Hostile-content | yes | none | `tests/fixtures/hostile/` |
| Migration | yes | none | `kokin-store` |
| Acceptance (11-step workflow) | yes | fixtures only | `tests/` |
| **Live-source** | **no** | real | `#[ignore]`, run explicitly |

Under replay, a fixture cache miss is a **test failure, never a live call**.
That single rule is what stops the deterministic suite from quietly degrading
into a live one.

## What CI enforces now

Implemented in `.github/workflows/ci.yml` as of increment 1:

1. **No network crates outside `kokin-net`.** A `grep` for `reqwest`, `hyper`,
   `ureq`, `std::net`, and `tokio::net` across `crates/` and `src-tauri/`,
   excluding `kokin-net`. Verified against a planted violation — the lint was
   confirmed to actually catch one, rather than passing because it matches
   nothing.
2. **Fixture secret hygiene.** A `grep` over `tests/fixtures/` for cookie,
   authorization, bearer, API-key, and password patterns. Fails the build on a
   hit, so a sanitiser miss cannot be committed.
3. **Licence and advisory checks** via `cargo-deny`, with a permissive-only
   allowlist.
4. **Build and test on both platforms**, `windows-latest` and `macos-14`.

## Storage tests specifically

`kokin-store` carries three tests that exist because of a specific failure mode
rather than for coverage:

- round-trip through close and reopen,
- a *named* wrong-passphrase error, distinguishable from corruption,
- **a byte-level assertion that the case file contains no plaintext and lacks
  the `SQLite format 3\0` magic header.**

The third one matters most. A build that linked plain SQLite instead of
SQLCipher would pass every functional test while writing unencrypted evidence
to disk. Only a byte-level check catches that.

## Not yet implemented

Named here so their absence is deliberate rather than forgotten. Each arrives
with the increment that makes it meaningful:

- Graph integrity, confidence-scoring, and AI-grounding tests
- Database migration tests (increment 3)
- Import/export round-trip tests (increment 11)
- Crash-recovery and disk-full tests
- Performance tests against small/medium/large case fixtures (PoC P2)
- Windows and macOS packaging tests beyond "the bundle builds"
