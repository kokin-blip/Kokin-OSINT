# ADR-0010: One egress broker; `kokin-net` is the only socket-capable crate

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard. Retrofitting a chokepoint after connectors exist
  means auditing every one of them.

## Context

A local-first investigation tool makes a specific promise: nothing leaves the
machine except what the user asked for, and the user can see what left. That
promise is only as strong as the *least* careful network call in the codebase.

Controls that matter — SSRF guarding, rate limiting, byte caps, per-connector
attribution, offline mode — are only enforceable at a chokepoint. Scattered
`reqwest` calls each reimplement some subset, and the one that forgets is the
one that matters.

## Decision

`kokin-net` is the only crate permitted to open a socket. Connectors never do;
they receive host-minted capability handles (`HttpCapability`, `DnsCapability`,
`FsReadCapability`, `ProcCapability`, `CancellationToken`) derived from their
manifest's declared permissions.

The broker enforces on **every** request:

- Scheme allowlist — rejects `file:`, `gopher:`, `data:`.
- Destination allowlist from the connector manifest.
- **SSRF guard applied to the resolved IP, re-resolved immediately before
  connect.** Guarding the hostname is not enough: DNS rebinding defeats it.
  Blocked ranges include loopback, RFC1918, `169.254.169.254`, CGNAT, `::1`,
  and `fc00::/7`.
- Redirects are never auto-followed cross-host; the chain is recorded on the
  capture.
- Streaming byte caps enforced during read, never after.
- Token-bucket rate limits, per `(connector, job, case)` attribution.
- Secret scrubbing on all log paths.

## Verification

Enforced as a build failure, not a code-review convention. The `privacy lints`
CI job fails on any use of `reqwest`, `hyper`, `ureq`, `std::net`, or
`tokio::net` outside `crates/kokin-net/`. `deny.toml` additionally bans
`openssl-probe` as a signal of system TLS discovery happening somewhere it
should not.

The lint was verified by negative control on 2026-08-12: a planted
`use reqwest::Client;` was caught, then removed. A grep that has never failed is
not a check.

PoC P4 asserts rejection of `127.0.0.1`, `[::1]`, `169.254.169.254`,
`100.64.0.1`, a DNS rebinding sequence, `file://`, and `gopher://`.
**Kill criterion: any bypass halts connector work until it is fixed.**

## Consequences

- Every network-capable feature must route through one crate's API. This is
  friction, and it is the point.
- The `external_tool` connector tier is the honest exception: Kokin cannot
  police a subprocess's sockets. Those manifests declare
  `network.observable = false`, the UI says so plainly, and **offline mode
  blocks the entire tier** rather than pretending to supervise it.
