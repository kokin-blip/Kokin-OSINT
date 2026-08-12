# ADR-0002: Rust core in-process; Python enters only as an external-tool connector

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** moderate. Adding a Python sidecar later is possible; the
  connector tier system is designed to accommodate it without a core rewrite.

## Context

The open-source OSINT ecosystem is overwhelmingly Python: sherlock, maigret,
holehe, theHarvester, and most of the Bellingcat toolkit's scriptable entries.
The obvious move is to bundle a Python runtime as a sidecar process and call
these tools directly.

Against that: bundling CPython plus dependencies costs 40–150 MB, undermining
ADR-0001's central justification. It also introduces a second packaging and
security-update surface, and — critically — a subprocess's network activity
cannot be policed by our egress broker (ADR-0010), which breaks the product's
central promise that every network request is attributable and that offline mode
actually means offline.

## Alternatives considered

**Bundled Python sidecar.** Immediate access to the whole ecosystem. Costs the
bundle-size win, adds a second supply chain, and produces network traffic the
application cannot see, attribute, rate-limit, or block.

**PyO3 embedded interpreter.** Same size cost, plus GIL contention and a much
harder packaging story across Windows and macOS. No isolation benefit — an
embedded interpreter shares our address space, which is strictly worse for
hostile-input handling.

**Rust-native reimplementation of the workflows, with Python tools available as
an explicitly-declared `external_tool` connector tier.** Costs implementation
effort per connector. Keeps the bundle small, keeps all first-party network
traffic inside the broker, and makes the boundary honest.

## Decision

The core is Rust, in-process. Python tools are reachable only through the
`external_tool` connector tier, whose manifest declares
`network.observable = false` and whose invocation is shown to the user. Offline
mode blocks that entire tier.

This is not a claim that reimplementation is free. It is a claim that the
alternative silently breaks a stated product guarantee, and that a guarantee we
cannot enforce is worse than a capability we do not ship.

**This decision has an empirical test.** PoC P8 implements twenty
WhatsMyName sites declaratively in Rust and measures the false-positive rate
against recorded fixtures. If a declarative Rust connector cannot match
sherlock's accuracy, the external-tool handoff becomes the primary path for
username discovery rather than the fallback, and this ADR is revised with that
evidence attached.

## Consequences

- Username discovery, DNS/RDAP, certificate transparency, and metadata
  extraction are implemented natively. Budget for this in the increment plan.
- Licence exposure drops sharply as a side effect: theHarvester declares no
  licence at all (all rights reserved) and holehe is GPL-3.0. Neither can be
  bundled. Under this ADR neither needs to be.
- ExifTool (GPL-1.0-or-later / Artistic-1.0-Perl) is likewise not bundled. Pure
  Rust EXIF handles the common cases; a user-installed ExifTool is detected and
  driven as an optional enhanced extractor.
- Users who want sherlock's full site coverage must install it themselves. This
  is a real capability gap and belongs in `docs/limitations/`.
