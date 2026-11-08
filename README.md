# Kokin-OSINT

A local-first desktop investigation workspace, where every entity, claim,
connection, location, event, image, and AI observation traces back to the
evidence that supports it.

> **Status: pre-alpha (v0.0.1), core-workflow implementation in progress.**
> The desktop now exposes evidence import/extraction, public static collection,
> contextual analysis, search, activity, named graph boards, cited reports and export/restore. The eleven-step desktop
> workflow repeat passes offline on Mac; the personal-use release gate remains open.

## Current features

| Area | Available controls |
|---|---|
| Cases | Encrypted creation/open/recovery/close and recovery-key acknowledgement |
| Evidence | Native file import, deduplication, explicit HTML extraction, paginated observations and text-only source bytes with verified citations |
| Collection | Offline by default; opt-in public static HTTP(S), disclosure, broker address/redirect checks, response caps and bounded timeouts |
| Analysis | Evidence-grounded entities/identifiers/relationships, all seven confidence dimensions, assessments and merge preview/reject/split |
| Workbench | Evidence, Entities, Search, Activity, Graph, Export, Sources, AI and Maps; shared detail/back history, paging, search filters and coverage on every result set |
| Graph | Multiple starting records, bounded direction/kind expansion, saved encrypted boards, camera/layout/pins, selection, evidence navigation and list alternative |
| Onboarding | Skippable introduction and encrypted real-case checklist; import and extraction remain separate |
| Reports | Encrypted versioned drafts, cited findings, seven-dimensional assessments, graph illustrations, escaped Markdown/self-contained HTML export; plaintext disclosure |
| Packages | Export with explicit omissions, verification and restore to a new folder; no package recovery key or digital signature |
| Media | Located PDF text, EXIF/orientation, encrypted previews/hashes and optional English OCR in an isolated native worker |
| Faces | Optional offline image/PDF/video face comparisons, suggested groups, reviewed person links, combined names/topics and local SearXNG discovery; [setup and limits](docs/increments/37-face-search.md) |
| Sources | Disclosed DNS/RDAP, crt.sh, Wayback discovery and separate archived-page capture; raw responses enter Evidence first |
| Local AI | Optional Ollama setup/import, cited suggestions and human review; pinned bge-small int8 semantic/hybrid search with encrypted vectors; lexical remains default |
| Maps | Local PMTiles, explicit coarse-world installation, source-grounded coordinates, uncertainty, saved camera and cited map illustrations |
| Operations | Background jobs, operation status/cancellation before atomic commit, serialized writes and case-generation checks |

HTML/JSON/PDF and supported images have explicit extraction paths; scanned pages can use opt-in English OCR. Unsupported formats remain imported evidence with limitations. Collected markup is displayed as text.
Recording exists as an explicit developer transport; arbitrary recordings still
need secret review before publication. Case-folder entry remains typed; when creating
a case, leaving it blank creates a unique folder under the OS application-data
directory's `cases` folder. The actual path is shown with the recovery key and in
case details. Opening an existing case still requires its folder path.

## Roadmap and verification

[The authoritative roadmap](docs/ROADMAP.md) distinguishes implemented, backend
only, planned and proposed capabilities. Graph exploration is available in the Graph workspace and entity
details. Media, public sources, optional AI/semantic search and offline maps now have
desktop controls. Ollama is separately installed and remains optional; Qwen inference
and hardware recommendations need that runtime plus explicit model installation and
real benchmarks. The pinned embedding resources passed real native retrieval tests.
See [AI delivery](docs/increments/35-local-ai-and-semantic.md) and
[map delivery](docs/increments/36-offline-maps.md). PR #1 is not adopted.

The eleven-step workflow now passes through the desktop twice offline on Mac,
including exact persisted identity/lineage comparison after reopen and restore.
Windows, manual VoiceOver and visible production timing remain deferred. Experimental
Mac verification and exact fault/benchmark scope are recorded in the increment reports;
no new release is published by this implementation. See [testing strategy](docs/testing/strategy.md)
and [graph/desktop report](docs/increments/30-graph-and-desktop-acceptance.md).
Builds remain unsigned/unnotarized; no public-release readiness is claimed.

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
- Not a claim that network collection is invisible. The live collection
  workflow discloses which connector contacted which service, when, and
  what it transmitted.

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

For the current Apple Silicon media/AI bundle, also stage the reviewed native engines
and notices with `scripts/prepare_media_mac.py`, `scripts/prepare_ai_mac.py`, `scripts/prepare_faces_mac.py` and
`scripts/bundle_frontend_notices.py`. These are deliberate developer resource
actions; installed pinned assets can be verified with `--offline`. Build the media
workers with `cargo build -p kokin-media --features engine --bins` and relocate their vetted libraries using `scripts/bundle_media_mac.py`, then
build Tauri with `--config src-tauri/media-packaging.conf.json` (or the equivalent
relative path from `src-tauri/`). See [native review](docs/native/media-license-review.md) and [face/video review](docs/native/face-and-video-license-review.md).
Models, OCR language data and map archives remain separate opt-in app resources.

## Layout

```
crates/
  kokin-store/      encrypted case storage, schema, audit chain
  kokin-keys/       key hierarchy (Argon2id, case master key, KEK wrapping)
  kokin-blob/       content-addressed encrypted blob store
  kokin-net/        the ONLY socket-capable crate — egress broker
  kokin-ingest/     file and fixture-backed URL ingestion, provenance
  kokin-extract/    parsers producing observations
  kokin-graph/      evidence-grounded analysis and entity resolution
  kokin-search/     FTS5 search
  kokin-export/     case packaging and integrity verification
  kokin-ai/         cited local suggestions, review, hardware and encrypted semantic retrieval
  kokin-vector-adapter/ isolated typed static sqlite-vec initialization
  kokin-media/      contained PDF/image/OCR worker
  kokin-connectors/ closed public-source manifests and plans
  kokin-maps/       bounded local PMTiles and coarse-world extraction
src-tauri/          typed desktop commands and case/global operation orchestration
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
  in CI; URL ingestion is exercised against synthetic replay fixtures. Live
  recording and connector implementations are still planned.
- **No copyleft dependencies.** `cargo-deny` enforces a permissive-only
  allowlist.
- **Case databases are genuinely encrypted.** A byte-level test asserts
  `case.db` contains no plaintext and lacks the SQLite magic header — catching a
  build that silently linked plain SQLite.
- **The case header carries no plaintext key material.** A test asserts neither the
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

### Guided workspace compatibility

Boards, report revisions and case guidance use append-only schema migration 8.
Older builds that support schema 7 cannot open an upgraded case. Existing package
snapshots preserve these records without a container-format change. Reports are
plaintext exports; packages remain the restorable encrypted format.

See [guided investigation implementation](docs/increments/ui-guided-investigation.md)
for verification, performance scope and outstanding release gates.
