# ADR-0005: Relational adjacency in SQLite, with petgraph for algorithms

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** moderate. The graph API sits behind a trait, so the
  algorithm backend can be swapped. The storage decision is harder to reverse.

## Context

Kokin is a link-analysis tool. The reflex for link analysis is a graph database,
and that reflex is where this decision goes wrong if it is not made deliberately.

The workload has two very different halves. Storage must hold 100k+ observations
and 300k+ edges durably, encrypted, in one file. Traversal must answer bounded
questions — neighbours of this entity, paths up to three hops — fast enough to
feel interactive, over a subgraph that is small because the render budget is
~2k nodes (ADR-0003 and the plan's scale contract).

## Alternatives considered

**Neo4j Community.** The obvious choice, and disqualifying on licence alone:
Neo4j Community is GPLv3 and Neo4j's server offerings are AGPLv3. Kokin ships
source-available (owner decision, 2026-08-12), so linking or bundling either is
not available to us. It is also a server process, which contradicts the
single-encrypted-file case model. `deny.toml` bans `neo4rs` by name so this
cannot be reintroduced by reflex in six months.

**KuzuDB.** Embedded, permissively licensed, genuinely good at multi-hop
traversal. Rejected for Phase 1 because it means a *second* storage engine
alongside SQLite, with its own file, its own encryption story (it has none that
composes with SQLCipher), and its own crash-consistency semantics. That is a
large amount of new failure surface to buy performance we have not yet shown we
need. Recorded as the exit path if PoC P2 fails.

**Relational adjacency in the case database.** Edges are rows. Traversal is a
recursive CTE. Algorithms that need a real graph (centrality, community
detection, layout) load the bounded subgraph into `petgraph` in memory.

## Decision

Relational adjacency in SQLite, with `petgraph` for in-memory algorithms over
bounded subgraphs.

The property that decides it: **edges live inside the encrypted case file**,
under the same transaction as the evidence they rest on. A relationship and the
`evidence_link` rows that justify it commit or roll back together. With a
separate graph store, that invariant becomes a distributed-transaction problem,
and the central promise of this product — that no analytical claim exists
without its evidence — becomes best-effort.

## Verification

PoC P2 measures p95 latency for a 3-hop recursive CTE at 100k observations and
300k edges. Kill criterion: interactive p95 above 200 ms sends us to KuzuDB
through the trait.

## Consequences

- Unbounded traversal is not offered, and that is deliberate: expansion is
  bounded and user-driven, which is also the correct UX for an investigation
  graph. "Expand everything" on a real case produces an unreadable hairball.
- Recursive CTEs are harder to read than Cypher. Queries live in one module
  with the traversal shape documented rather than being scattered.
- No graph-native indexes. If a workload appears that genuinely needs them, P2's
  numbers are the evidence for revisiting, not intuition.
