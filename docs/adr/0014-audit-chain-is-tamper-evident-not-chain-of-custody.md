# ADR-0014: The audit chain is tamper-evident, not tamper-proof, and is not chain of custody

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** the mechanism is easy to extend; the *claim* is what this
  ADR pins down.

## Context

The spec is explicit: *do not describe this as forensic chain of custody unless
the implemented controls actually support that claim.*

They do not, and no purely local design can. This ADR exists mainly to stop a
future contributor — or a future marketing page — from quietly upgrading the
claim, because the mechanism looks stronger than it is.

## Decision

`audit_event` forms a hash chain:

```
event_hash = BLAKE3(prev_event_hash || canonical_cbor(payload))
```

Hashing uses deterministic CBOR (`ciborium`, sorted canonical keys) so the same
logical payload always hashes identically. Human-readable export manifests use
RFC 8785 JCS JSON, which serves a different purpose and is not the hash input.

## What this actually gives you

**It detects modification of the log by anyone who cannot recompute the chain.**
That covers file corruption, a partial write, and an unsophisticated edit.

## What it does not give you

**Anyone holding the case passphrase can rewrite the entire chain from genesis
and produce a perfectly valid log.** The case owner is inside the trust
boundary, and the chain offers no protection against the case owner. There is no
external anchor, no third-party attestation, and no independent timestamp.

That is the difference between tamper-*evident* within a trust boundary and
tamper-*proof*, and it is the difference between this and chain of custody.

**This disclaimer belongs in the UI, not only in this file.** A user who has to
read the ADRs to learn the limits of the guarantee has already been misled.

## Deferred

RFC-3161 external timestamping would let a third party attest that a hash
existed at a time, which is the missing anchor. It requires network access to a
timestamping authority — acceptable as an explicit, user-initiated,
broker-mediated action, but not compatible with offline-first defaults.
Deferred to Phase 3.

## Open question for the owner

**D-010: may Kokin output ever be used in a legal proceeding?** If yes, the
"not chain of custody" language needs review by a lawyer, not by an architect.
Recorded in `docs/decision-log.csv` as needing owner input, and deliberately not
answered here.

## Verification

Implemented in `crates/kokin-store/src/audit.rs`. Nine tests, including:

- modifying an event breaks the chain, reported at the modified event
- deleting an event breaks the chain, reported at the event after the hole, and
  distinguished from a modification so the diagnosis says which happened
- **`anyone_who_can_write_can_forge_a_valid_chain`** — deletes the whole log,
  rebuilds it, and asserts it verifies as `Intact`. This test exists so the
  limitation above is demonstrably true rather than merely asserted, and so
  anyone tempted to upgrade the claim has to delete a test that contradicts them.

The lifecycle is actually recorded: `case.created`, `case.opened` (carrying
**which key unlocked it**, since an unexpected recovery-key unlock is the event
worth noticing), and `case.passphrase_rotated`.

## Consequences

- Verification is cheap and local; export `verify` walks the chain.
- Crypto-shredding a blob makes that blob's integrity permanently unverifiable,
  so an export containing it verifies as `PARTIAL`, never `OK`
  (`docs/limitations/deletion.md`). A verifier that reported `OK` after a shred
  would be lying, which is worse than reporting a gap.
- Recorded as R-011.
