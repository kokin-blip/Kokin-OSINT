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

## Build

```bash
cargo test --workspace     # includes the encrypted-storage round-trip
cd ui && npm ci && npm run build
cd ui && npx tauri dev     # run the app
```

The first build compiles OpenSSL and SQLCipher from source and takes several
minutes. Subsequent builds are cached.

## Layout

```
crates/
  kokin-store/      encrypted case storage, schema, audit chain
  kokin-crypto/     key hierarchy
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
- **Case files are genuinely encrypted.** A byte-level test asserts the file on
  disk contains no plaintext and lacks the SQLite magic header — catching a
  build that silently linked plain SQLite.

## Licence

Source-available. See `LICENSE`.
