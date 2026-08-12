# Kokin-OSINT

A local-first desktop investigation workspace, where every entity, claim,
connection, location, event, image, and AI observation traces back to the
evidence that supports it.

> **Status: pre-alpha skeleton (v0.0.1).** No collection, storage, or analysis
> features are implemented yet. The repository currently contains the
> architecture decisions, the risk register, and a proven encrypted-storage
> round-trip. See `docs/adr/` for what has been decided and why.

## What this is

An offline-capable desktop application for open-source research, built around a
strict separation between *what was collected* and *what an analyst concluded
from it*. Raw evidence is append-only and never silently modified. Every derived
object — an extracted field, an entity, a relationship, an AI suggestion —
carries a lineage chain back to its inputs, the transformation that produced it,
that transformation's version, and when it ran.

## What this is not

- Not a tool for bypassing access controls, testing credentials, defeating
  CAPTCHAs, or accessing private platform APIs.
- Not chain of custody. The audit log is tamper-*evident* within the
  application's trust boundary; anyone holding the case key can rewrite it. See
  `docs/limitations/`.
- Not a claim that network collection is invisible. The application shows which
  connector contacted which service, when, and what it transmitted.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | stable (1.85+) | `rustup` |
| Node.js | 22+ | for the frontend build |
| **Perl** | **native Windows perl** | **Windows only.** Required to build vendored OpenSSL for SQLCipher. |

### The perl requirement, specifically

On Windows you need **Strawberry Perl** or ActivePerl:

```powershell
winget install StrawberryPerl.StrawberryPerl
```

Cygwin-flavoured perls **do not work** and fail with confusing errors:

- **msys64 perl** → `This perl implementation doesn't produce Windows like paths`
- **Git-for-Windows perl** → `Can't locate Locale/Maketext/Simple.pm`

OpenSSL's `Configure` explicitly rejects them for MSVC targets. macOS and Linux
system perl work as-is. See ADR-0004.

**Installing Strawberry Perl is not enough — it must be first on `PATH`.**
`C:\Strawberry\perl\bin` has to come before any msys or Git perl. Two ways this
bites, both observed on this project:

- In Git Bash, `which perl` reports `/usr/bin/perl` (msys). Building from Git
  Bash therefore fails even with Strawberry installed.
- A GitHub Actions step with `shell: bash` on a Windows runner has the same
  problem, which is why the CI bundle step deliberately uses the default shell.

Check with `perl -V:osname` — it must print `osname='MSWin32'`. Anything else is
the wrong perl.

## Build

Run these from the **repository root**, not from `ui/`. The Tauri CLI resolves
`src-tauri/tauri.conf.json` by searching downward from the working directory.

```bash
cargo test --workspace                  # includes the encrypted-storage tests
cd ui && npm ci && cd ..                # frontend dependencies

node ui/node_modules/@tauri-apps/cli/tauri.js dev     # run the app
node ui/node_modules/@tauri-apps/cli/tauri.js build   # produce an installer
```

Invoked through `node` rather than `./ui/node_modules/.bin/tauri` because that
shim is a `/bin/sh` script, which on Windows pulls in the wrong perl (above).

The first build compiles OpenSSL and SQLCipher from source and takes roughly
ten to fifteen minutes. Subsequent builds are cached.

## Layout

```
crates/
  kokin-store/      encrypted case storage, schema, audit chain
  kokin-keys/       key hierarchy (Argon2id, case master key, KEK wrapping)
  kokin-blob/       content-addressed encrypted blob store
  kokin-net/        the ONLY socket-capable crate — egress broker
  kokin-extract/    parsers producing observations
  kokin-search/     FTS5 search
  kokin-export/     case packaging and integrity verification
  kokin-ai/         hardware detection, local inference
src-tauri/          desktop shell (thin — no business logic)
ui/                 Svelte frontend
docs/adr/           architecture decision records
docs/               threat model, data model, limitations, benchmarks
research/           source registry, competitor matrix, tool coverage
poc/                proof-of-concept probes, excluded from release builds
```

## Design invariants

These are enforced by CI, not by convention:

- **No network access outside `kokin-net`.** Any other crate opening a socket
  would bypass the SSRF guard, rate limiter, byte caps, per-connector
  attribution, and offline mode at once.
- **The whole test suite passes with no network.** `KOKIN_NETWORK=deny` is set
  in CI; connectors are exercised against recorded, sanitised fixtures.
- **No copyleft dependencies.** `cargo-deny` enforces a permissive-only
  allowlist.
- **Case databases are genuinely encrypted.** A byte-level test asserts
  `case.db` contains no plaintext and lacks the SQLite magic header — catching a
  build that silently linked plain SQLite.
- **The case header carries no key material.** A test asserts neither the
  passphrase nor the recovery key appears in the plaintext `header.json`.

## What a case looks like on disk

A case is a bundle directory, not a single file (ADR-0016):

```
MyCase.kokincase/
  header.json    plaintext - salt, KDF profile, and the case master key
                 wrapped separately under the passphrase and the recovery key
  case.db        SQLCipher, keyed by the case master key
  blobs/         content-addressed encrypted evidence
```

`header.json` is plaintext by design and contains no secrets — the salt is not
secret and the wrapped keys are AEAD ciphertext, the same shape as a LUKS or
`age` header. It does mean the *existence* of a case and its parameters are
visible to anyone with filesystem access; the contents are not.

Because the database is keyed by the case master key rather than the
passphrase, changing a passphrase rewrites only `header.json` and re-encrypts
nothing.

## Licence

Source-available. See `LICENSE`.
