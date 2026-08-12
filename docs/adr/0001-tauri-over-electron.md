# ADR-0001: Tauri 2 for the desktop shell, not Electron

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard. The shell choice determines the IPC model, the
  packaging pipeline, and the language the core is written in. Reversing this
  after the core crates exist means rewriting the core.

## Context

Kokin-OSINT is a local-first desktop investigation workspace for Windows and
macOS. The application must have low idle resource use, strong isolation
between untrusted collected content and the application core, straightforward
packaging for non-technical users, and a small enough install that it can be
distributed without friction.

The owner has shipped desktop applications before and has been bitten
specifically by bundle size: a previous project's PyInstaller `--onefile` build
re-extracted 1.45 GB on every launch and took roughly two minutes to reach a
window. Install size is therefore treated as a first-class constraint, not a
nice-to-have.

## Alternatives considered

**Electron.** Mature, enormous ecosystem, well-understood packaging and
auto-update (electron-builder, Squirrel). Every renderer is a full Chromium.
Baseline install is roughly 150 MB before any application code, and idle memory
runs to several hundred MB. Its main-process code is Node, which would make
JavaScript the natural home for the evidence and crypto layers — a poor fit for
the integrity guarantees this product is built around.

**Tauri 2.** Uses the operating system's webview (WebView2 on Windows, WKWebView
on macOS) instead of bundling a browser engine. Baseline install is roughly
15 MB. The core is Rust, which gives us memory safety in exactly the layer that
parses hostile input, and a capability/permission system in the shell itself.
Cost: webview behaviour differs between platforms, so rendering must be tested
on both; and the ecosystem is younger than Electron's.

**Native per-platform (WinUI + SwiftUI).** Best fidelity and performance,
roughly double the UI work, and no shared graph/map/timeline implementation.
Rejected as untenable for a solo developer.

## Decision

Tauri 2.

The deciding factors, in order:

1. **Bundle size** — 15 MB against 150 MB, against a stated constraint the owner
   has already been burned by.
2. **Rust in the parsing layer** — every byte this application ingests is
   untrusted. Memory-safe parsing of hostile HTML, images, archives, and
   documents is a core security property, not an implementation detail.
3. **Capability model** — Tauri's permission system aligns directly with the
   connector trust tiers (ADR-0011); Electron would require building that from
   scratch.
4. **Rust core is reusable** — the `kokin-*` crates are testable and shippable
   without a webview at all, which is what makes the deterministic offline test
   suite possible.

The accepted cost is cross-platform webview variance. This is mitigated by the
CI matrix building and testing on both platforms from the first commit.

## Consequences

- The frontend cannot assume Chromium. No Chromium-only CSS or JS APIs.
- Rendering differences between WebView2 and WKWebView must be caught by CI and
  by manual verification, since the owner has no Mac (see ADR-0002 consequences
  and `docs/limitations/platforms.md`).
- Auto-update uses Tauri's updater, which requires a signing key; deferred until
  distribution is in scope (owner decision: personal use first).
- The Python OSINT ecosystem is not directly importable. See ADR-0002.
