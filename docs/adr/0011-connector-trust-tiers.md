# ADR-0011: Five connector trust tiers with capability ceilings

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** moderate. Tiers can be added; removing one strands
  connectors written against it.

## Context

Connectors are the extension point, and therefore the attack surface. A single
"plugin" notion forces one trust level for code the user wrote, code a stranger
published, and a subprocess Kokin cannot supervise at all. Those are not the
same risk and must not get the same permissions.

## Decision

Five tiers, each with a capability ceiling the broker (ADR-0010) enforces.

| Tier | What it is | Ceiling |
|---|---|---|
| `builtin_supported` | Rust, in-tree, maintained by us | Full manifest-declared capabilities |
| `builtin_experimental` | In-tree, not yet stable | Same, flagged in the UI as unstable |
| `third_party_local` | Third-party WASM component (**Phase 2**, wasmtime WASI-P2) | Sandboxed; no ambient filesystem or network |
| `external_tool` | A subprocess such as ExifTool or sherlock | **`network.observable = false`**; blocked entirely in offline mode |
| `import_only` | A file another tool produced | No execution at all |

`external_tool` is the honest tier. Kokin cannot see or restrict what a
subprocess does with a socket. Rather than implying supervision it does not
have, the manifest declares the limitation, the UI states it plainly, and
offline mode refuses the whole tier. Claiming otherwise would be the kind of
overstated assurance this project's threat model explicitly rejects.

## Phase 1 scope

Phase 1 ships `builtin_*` and **declarative** connectors only: a manifest plus
request templates plus CSS/JSONPath extraction rules, with **no code**, and
therefore inherently sandboxed. That covers roughly 70% of realistic OSINT
connectors, including a WhatsMyName-style username sweep — subject to D-012,
since that dataset is CC BY-SA 4.0 (S-009).

Connector signing, revocation, and quarantine are deferred to Phase 2 on the
owner's "personal use first" decision. The manifest already carries the fields
they will need, so this is deferral, not a rewrite.

## Manifest

`connector.toml`, validated against `connectors/schema/connector.schema.json`,
declaring: network destinations and methods, **`transmitted_fields`** (precisely
what leaves the machine), auth and credential scope, required browser state, I/O
types, concurrency and rate limits, retries, pagination, cancellation, retention,
error taxonomy, fixture provenance, version compatibility, health, and
`last_verified`.

## Consequences

- Writing a connector requires filling in a long manifest. Deliberate: every
  field is something a user would otherwise have to take on trust.
- `transmitted_fields` is a claim we cannot fully verify for `external_tool`,
  and the UI must not present it as if we can.
