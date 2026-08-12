# ADR-0009: sqlite-vec behind a trait, with a stated exit condition

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** easy by construction — that is what the trait is for.

## Context

Semantic search over observations needs vector similarity. The natural fit is an
extension that lives inside the case database, so vectors are covered by
SQLCipher page encryption like everything else and do not become a plaintext
index sitting beside an encrypted one (the same argument as ADR-0004).

`sqlite-vec` is that extension. Verified 2026-08-12 (S-015): latest release is
**v0.1.9**, published 2026-03-31, Apache-2.0. It is pre-1.0 and has been for
some time.

## Decision

Use `sqlite-vec`, behind a `VectorIndex` trait, with the exit condition written
down now rather than discovered later.

**Exit condition.** Move to `usearch` (or an equivalent embedded index) if any
of these hold:

1. Recall or latency fails the PoC P2 target at 100k vectors of 384 dimensions.
2. A correctness bug appears that upstream does not fix within one release
   cycle.
3. The project goes twelve months without a release, indicating abandonment.

The migration cost is bounded because nothing outside the trait knows vectors
are stored in SQLite. The cost that is *not* bounded is losing encryption
coverage: an external index file would need its own envelope encryption, which
is why `usearch` is the fallback and not the default.

## Consequences

- Vector search is not on the critical path for the Phase 1 vertical slice. If
  this dependency fails, the slice still ships.
- Pre-1.0 means the on-disk format may change across versions. Vector indexes
  are treated as **derived data, rebuildable from observations**, never as a
  source of truth — so a format break costs a reindex, not a case.
- Recorded as R-012 in the risk register alongside `ort` (ADR-0012), which has
  the same shape of problem.
