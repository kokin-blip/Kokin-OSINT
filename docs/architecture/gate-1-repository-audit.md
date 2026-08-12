# Gate 1: Repository audit and risk inventory

- **Date:** 2026-08-12
- **Gate status:** passed

The working method requires auditing the existing repository before
recommending or changing any architecture. This document records the result.

## Finding: the repository was empty

`kokin-blip/Kokin-OSINT` was created 2026-08-12T08:42:22Z and had received no
pushes. Verified through the GitHub API rather than assumed:

| Check | Result |
|---|---|
| `repos/kokin-blip/Kokin-OSINT` | `size: 0`, `language: null`, `default_branch: main` (declared but nonexistent) |
| `git/trees/HEAD?recursive=1` | HTTP 409 — `Git Repository is empty` |
| `repos/.../commits` | HTTP 409 — `Git Repository is empty` |
| `repos/.../branches` | `[]` |
| Releases, issues | none |

No local checkout existed anywhere on the development machine. Verified by
recursive scan of every `.git/config` under the user profile for the string
`Kokin-OSINT` (0 matches), plus directory scans of all plausible source
locations and every fixed drive.

The user profile directory is itself a git repository, but for an unrelated
project (`kokin-swiss-downloader`). Its working tree was clean, with no
untracked or staged OSINT-related work, and its `.gitignore` excludes
everything outside one unrelated subdirectory.

## Consequences for the audit checklist

Every item the working method asks for — existing data models, current tests
and build process, technical debt, reusable components, components to replace,
uncommitted or user-authored work to preserve — is **not applicable**. There
was nothing to preserve and nothing to overwrite.

The only pre-existing project artifacts were two planning documents in the
owner's Downloads folder. They are inputs to this work, not code.

## What was audited instead

Since there was no codebase, the substantive audit was of the development
environment, because it constrains the architecture:

| Component | Finding | Architectural consequence |
|---|---|---|
| Rust 1.97.1 + cargo | present | Tauri and a Rust core are viable immediately (ADR-0001) |
| Node 24.18.0 / npm 11.16 | present | Frontend tooling ready |
| `cargo-tauri` CLI | **absent** | Use the npm `@tauri-apps/cli` instead of compiling the Rust CLI |
| Native Windows perl | **absent** | Blocks vendored OpenSSL, hence SQLCipher. Resolved by installing Strawberry Perl; recorded as risk R-005 and in ADR-0004 |
| Ollama | present | Local inference available without bundling a runtime (ADR-0012) |
| Python (conda + venv) | present | Available for `external_tool` connectors, deliberately not bundled (ADR-0002) |
| ExifTool 13.29 | present, unpacked in Downloads | Usable as an optional external extractor; **not** bundled, for licence reasons (ADR-0002) |
| msys64 | present | Supplies build utilities, but its perl is unusable for OpenSSL |
| GPU: RTX 2060 SUPER | 8 GB VRAM | **WMI misreports this as 4.29 GB** — hardware detection must use NVML (PoC P5) |
| `gh` 2.96, authenticated | present | Repository operations available |

## Risk inventory

Twelve risks recorded in `docs/risk-register.csv`, of which four are rated
high or critical impact:

- **R-001** macOS is a target with no Mac available to test on.
- **R-004** a build could silently link plain SQLite, producing unencrypted
  cases while all functional tests pass.
- **R-006** a network call outside `kokin-net` would bypass every egress control
  at once.
- **R-008** collected content reaching a local model could carry injection
  instructions.

R-004 and R-006 are already mitigated by tests and CI lints in this increment.
R-001 is mitigated by the macOS CI leg existing from the first commit. R-008 is
open and gated behind PoC P6.
