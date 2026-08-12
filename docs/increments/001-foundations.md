# Increment 1 — foundations, and proof that encrypted storage builds on both platforms

Date: 2026-08-12 · Branch: `feat/phase0-foundations` · Gates 1–3 closed

## Files changed

Initial commit plus three follow-ups (`6b7bd67`, `9d8ddd1`, `aa5fbe3`,
`d3fe79c`, `d7d4702`). Roughly 95 files. The ones that carry behaviour:

| File | What it does |
|---|---|
| `crates/kokin-store/src/lib.rs` | The only real code. Opens a SQLCipher case, applies the key, forces a read to surface a wrong passphrase, pins cipher parameters. |
| `src-tauri/src/lib.rs` | Thin command layer; exposes `storage_encryption_status()`. |
| `ui/src/App.svelte` | Displays the linked cipher backend. |
| `.github/workflows/ci.yml` | Build matrix, privacy lints, licence gate, research and cross-reference lints. |
| `deny.toml` | Permissive-only licence allowlist; bans `neo4rs`; five documented advisory suppressions. |
| `scripts/lint_research.py` | Fails the build on an unattributed research claim. |
| `scripts/lint_crossrefs.py` | Fails the build on a doc reference that does not resolve. |

Eight further crates are stubs with a doc comment and nothing else.

## Behaviour added

Creating and reopening an encrypted case; a named error for a wrong passphrase;
surfacing the linked cipher backend in the UI so a mis-built binary is visible
to a user, not only to CI. That is the whole of the runtime behaviour, and it is
deliberate — the increment's purpose was risk reduction, not features.

## Tests executed and results

Local, Windows, 2026-08-12:

```
cargo test --workspace --all-targets     3 passed, 0 failed
cargo fmt --all -- --check               clean
cargo clippy --workspace --all-targets   0 warnings (with -D warnings)
cargo deny check bans licenses sources advisories
                                         advisories ok, bans ok, licenses ok, sources ok
python scripts/lint_research.py          24 sources, 18 claim rows
python scripts/lint_crossrefs.py         100 references across 20 files
npm run check (svelte-check)             0 errors, 0 warnings
```

The three store tests are: encrypted case survives close and reopen; wrong
passphrase is rejected *and named as such*; **the case file contains no
plaintext and lacks the `SQLite format 3\0` header.** The third is the one that
matters — it is the only check that catches a build which silently linked plain
SQLite, a failure every functional test would pass.

**CI run `31600285791`: all four jobs green on both platforms**, producing
installable artifacts — `kokin-osint-x86_64-pc-windows-msvc` (3.75 MB NSIS) and
`kokin-osint-aarch64-apple-darwin` (4.81 MB). This is the increment's exit
criterion: the macOS build works without a Mac, and the Windows installer
builds.

Getting there took four runs. The first three had their Windows leg cancelled
by `cancel-in-progress` when I pushed again mid-build, which also meant
`rust-cache` never reached its save step, so every Windows run paid the full
cold OpenSSL compile. The fourth completed and failed for a real reason — see
`docs/increments/002-key-hierarchy.md`, which fixed it.

### Checks verified by negative control

A check that has never failed is not evidence. Three were deliberately broken
and confirmed to fail:

| Check | Planted fault | Caught |
|---|---|---|
| Privacy lint | `use reqwest::Client;` outside `kokin-net` | yes |
| Licence gate | removed `MIT` from the allowlist | yes |
| Cross-reference lint | repointed a valid decision reference at a nonexistent id | yes |

The research lint needed no planted fault — it caught a real shifted column in
the competitor matrix on first run, which would otherwise have silently
mislabelled Hunchly's capabilities.

## Performance measurements

Not the point of this increment, and no scale claims are made. For context:
cold Windows build (vendored OpenSSL from source) ~10m39s locally; the three
store tests run in 0.61s; the frontend bundle is 33.56 kB.

## Security and privacy implications

- Case files are encrypted with SQLCipher, page-level, **including the FTS
  index** — the design property that makes ADR-0004 worth the build complexity.
- Cipher parameters are pinned (`cipher_page_size=4096`, `kdf_iter=256000`) so a
  future SQLCipher upgrade cannot silently change the on-disk format.
- `unsafe_code = "forbid"` workspace-wide.
- Network egress is architecturally constrained to `kokin-net` and enforced by a
  CI lint, before any connector exists to violate it.
- **No secrets, case data, models, or large artifacts are committed**; the
  `.gitignore` covers `*.kokincase`, `/models/`, `*.onnx`, `*.gguf`,
  `*.traineddata`, `*.pmtiles`, and key material.
- Five unmaintained-crate advisories are suppressed. They are transitive through
  Tauri with no upgrade available, they are static Unicode tables with no I/O,
  and the suppression is by advisory ID so any *other* unmaintained crate still
  fails the build (R-013).

## Known limitations

1. **Pushing during a run cancels it.** `cancel-in-progress` is correct for PR
   churn but means the slowest leg never finishes if a commit lands mid-build,
   and the cache is never saved. The working practice is to hold pushes while a
   Windows build is running; if that proves too restrictive, exempt the build
   job from cancellation.
2. macOS artifacts are ad-hoc signed only; Gatekeeper will block them
   (R-002, accepted).
3. Eight of nine crates are stubs. The architecture is documented, not built.
4. The threat model is 3 controls active against 12 planned. Stated in the
   threat model itself so it cannot be read as implemented.
5. Building on Windows requires Strawberry Perl or another native Windows perl;
   cygwin perls are rejected by OpenSSL's `Configure` (R-005, ADR-0004).
6. **PoC P8 is blocked** on D-012 — WhatsMyName is CC BY-SA 4.0, so porting its
   dataset into our connector format is an adaptation of a ShareAlike work.

## Manual verification

```
git clone <repo> && cd Kokin-OSINT
cargo test -p kokin-store            # 3 tests must pass
cargo deny check bans licenses sources advisories
python scripts/lint_research.py && python scripts/lint_crossrefs.py
cd ui && npm ci && npm run check && cd ..
node ui/node_modules/@tauri-apps/cli/tauri.js build   # Strawberry Perl must be first on PATH
```

Then launch the built app: the window must state which cipher backend is linked.
If it does not say SQLCipher, the build is wrong regardless of passing tests.

## Next recommended increment

**Increment 2 — `kokin-keys`:** Argon2id parameters, the case-master-key / KEK /
recovery-key hierarchy, OS keyring behind a feature flag, and known-answer test
vectors. No UI.

It is the right next step because increment 1 hardcodes a passphrase straight
into SQLCipher's `PRAGMA key`, which is fine for proving the round-trip and
wrong for a product: it gives no key rotation, no recovery path, and no
separation between the passphrase and the key that actually encrypts the data.

That blocker is now cleared: run `31600285791` is green on both platforms.
