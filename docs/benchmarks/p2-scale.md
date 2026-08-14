# P2 — the storage layer at 100,000 observations

**Date:** 2026-08-14 · **Risk:** R-003 · **ADRs:** 0003, 0005, 0009
**Probe:** `src-tauri/tests/scale.rs` (`#[ignore]`d; run deliberately)

```text
KOKIN_NETWORK=deny cargo test -p kokin-osint --test scale --release -- --ignored --nocapture
```

## Why

ADR-0003 chose SQLite with FTS5 over a server database. ADR-0005 chose
relational adjacency over a graph database. R-003 has been open since gate 3:
neither choice had been measured against the spec's stated target — *"cases with
at least 100,000 observations remain searchable"* — and the interactive budget
is **p95 ≤ 200 ms**.

## The case under test

| | |
|---|---|
| Observations | 100,000 |
| Entities | 20,000 |
| Relationships | 300,000 |
| Artifacts | 200 |
| Seed time | ~30 s |
| **On disk** | **438 MiB** |

Loaded through the real schema, so every trigger fired: an `observation` insert
fires into `search_document`, which fires into `search_index`. A bulk load around
the side would have measured an FTS index the product's own write path never
built.

**The topology is deliberately hostile.** Every tenth relationship is wired into
50 hub entities out of 20,000. Real investigation graphs have hubs; probably not
that concentrated. The expansion numbers are therefore a *pessimistic* bound,
which is the right direction for a risk probe, and "hub expansion is slow" must
not be read as "all expansion is slow".

## Results

| Query | p50 | p95 | Verdict |
|---|---|---|---|
| 3-hop expansion, `IN (from, to)` | 86,316 ms | 86,316 ms | counter-example |
| 3-hop expansion, indexed CTE | 567 ms | 569 ms | counter-example |
| **3-hop expansion, bounded per hop** | **69 ms** | **70 ms** | **within budget** |
| FTS5 common term (default options) | 291 ms | 292 ms | over budget — R-014 |
| FTS5 two terms | 197 ms | 197 ms | marginal |
| **FTS5 common term, `resolve_merged: false`** | **118 ms** | **118 ms** | **within budget** |
| FTS5 typeahead prefix | 295 ms | 296 ms | over budget — R-014 |
| `coverage()` | 0.2 ms | 0.2 ms | within budget |

## What the numbers mean

### The graph: ADR-0005 holds, and the obvious query is a trap

Three forms of the same 3-hop expansion span **1,235×**.

**`reach.id IN (r.from_entity, r.to_entity)`** reads naturally and expresses an
undirected edge in one branch. It also uses neither index: SQLite has
`relationship_from` and `relationship_to` as separate indexes, and an `IN` across
two columns is a lookup on neither. Every recursion step full-scans 300,000 rows.
**86 seconds.**

**Two union branches** — `JOIN reach ON r.from_entity = reach.id` and again on
`r.to_entity` — express the same set as the union of two directed traversals, and
each is an index lookup. **569 ms, a 152× improvement**, and still 2.8× over
budget.

The remaining cost is not indexing. Both CTE forms compute the **entire 3-hop
closure and then `LIMIT` it**, so from a hub the limit describes what is returned
and not what was computed. That is precisely the *"uncontrolled graph explosion"*
the spec names as a thing to prevent.

**Walking hop by hop in the host**, stopping the moment the render budget is
reached and capping how many neighbours any one node may contribute, costs
**70 ms**. It returns 196 nodes rather than 1,603 — that is the point, not a
caveat: it is a *bounded* expansion, and the bound is the product feature.

> **ADR-0005 is not in question. The query shape is.** A single recursive CTE
> also cannot be cancelled or rendered progressively, both of which the spec
> requires — so the form that is fast is the same form the product needed anyway.

### Full-text search: FTS5 is fine, alias collapsing is not

FTS5 itself answers a common term over 100k observations in **118 ms**.

The default path costs **292 ms**, and the 174 ms difference is `resolve_merged`.
It is *not* the `LEFT JOIN` on `er_merge_map` that the module comment anticipates
as the cost — that is one indexed lookup per hit. It is the window function:

```sql
ROW_NUMBER() OVER (PARTITION BY subject_kind, resolved_id ORDER BY relevance, subject_id)
```

which must materialise and sort **all ~40,000 matched rows** before `LIMIT 50`
can apply. With `resolve_merged: false` SQLite streams and stops at 50.

**This is not a careless query.** Collapsing before limiting is deliberate and
argued in the code: doing it afterwards silently shrinks pages — ask for fifty,
get forty-seven, because three were aliases. The cost was simply never measured
at this size.

**The case under test had zero merges.** Paying 174 ms to deduplicate against an
empty map is the specific waste, and it suggests the remedy: skip the window
entirely when the case holds no active merge, which preserves semantics exactly
because there is nothing to collapse. Recorded as **R-014**, not fixed here —
measuring and fixing in one increment is how a benchmark comes to justify the
change it was used to design.

### `coverage()`: the worry was unfounded

Increment 16 made coverage non-optional on every result list (D-029) and flagged
it as *"recomputed on every search and never measured… per-keystroke work if
typeahead is ever wired to this"*. It is **0.2 ms**, roughly 1,400× cheaper than
the search it accompanies. Closed.

## Verdict on R-003

**Partially resolved, and the storage choice survives.**

- **ADR-0003 (SQLite + FTS5) holds.** FTS5 answers in 118 ms at target scale. The
  overage is a query shape in `kokin-search`, not the engine.
- **ADR-0005 (relational adjacency) holds**, conditional on bounded expansion —
  which the spec independently requires.
- **ADR-0009 (`sqlite-vec`) is untouched.** Vector KNN was **not measured because
  there is nothing to measure**: `sqlite-vec` is not in this workspace — no
  dependency, no lock entry, no embedding anywhere. That third of R-003 stays
  open until embeddings land and closes against ADR-0009's own exit condition.

Neither kill criterion is met. No migration to KuzuDB or `usearch` is warranted
on this evidence.

## What this does not establish

1. **One machine, one run.** A Ryzen 5 with an NVMe SSD. A weaker disk changes
   every number here, and the spec's audience includes investigators on modest
   hardware.
2. **Read-only.** Nothing was measured under concurrent writes, which is what a
   collection job does while an analyst searches.
3. **438 MiB for 100k observations** is recorded and not yet judged. Whether that
   is acceptable depends on an install-size and case-size expectation that
   `docs/decision-log.csv` still lists as an open owner question.
4. **Cold cache is untested.** Each measurement discards one untimed pass first,
   so these are warm figures. The first search after opening a case will be worse.
5. **The hub topology is synthetic.** See above.

## Follow-up

| Item | Where |
|---|---|
| Skip alias collapsing when the case has no active merges | R-014 |
| Bounded per-hop expansion as the graph view's only traversal | increment 30 |
| Vector KNN at 100k | ADR-0009, when embeddings exist |
| Cold-cache and concurrent-write figures | a later run of this probe |
