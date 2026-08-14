# Increment 21 — a fix that works, until the first merge

R-014 came out of P2: search cost 291 ms against a 200 ms budget, and 174 ms of
that was collapsing aliases on a case that had no merges at all.

This increment skips the collapse when there is nothing to collapse. It works —
**291 ms → 117 ms** — and it stops working the moment a case merges anything,
which is measured here rather than discovered later.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-search/src/lib.rs` | `collapse_aliases`; `one_row_per` takes a decision rather than the options. |
| `crates/kokin-search/tests/search.rs` | `skipping_the_collapse_changes_nothing_until_something_is_merged`. |
| `src-tauri/tests/scale.rs` | A measurement after one merge. |
| `docs/benchmarks/p2-scale.md` | Before, after, and after-one-merge. |
| `docs/risk-register.csv` | R-014 → `partially mitigated`, re-scoped. |

No dependency changes.

## The skip is provable, not empirical

`search_document` carries `UNIQUE (subject_kind, subject_id)` — one document per
subject. With no active `er_merge_map` row, `resolved_id` is
`COALESCE(NULL, d.subject_id)` for every matched row, so each
`PARTITION BY subject_kind, resolved_id` holds exactly one row, `ROW_NUMBER()` is
1 everywhere, and `WHERE alias_rank = 1` discards nothing.

**The unique index is the proof.** That matters more than the speedup: "skip the
expensive thing when it looks unnecessary" is where correctness usually breaks
quietly, and this is a case where the schema settles it rather than an argument
about the data.

The probe itself is free — `er_merge_map_absorbed` is a partial index on
`WHERE active = 1`, so the existence check stops at the first row.

## What it bought

| Query | before | after | after **one** merge |
|---|---|---|---|
| FTS5 common term | 291 ms | **117 ms** | **291 ms** |
| FTS5 two terms | 196 ms | **95 ms** | — |
| FTS5 typeahead | 295 ms | **121 ms** | — |
| `resolve_merged: false` | 118 ms | 118 ms | — |

The default path and the explicitly-unresolved path now measure within 1 ms of
each other, because they now execute the same plan. That is the equivalence
argument confirmed by measurement rather than asserted.

## And what it did not

**One merge, between two of twenty thousand entities, puts it straight back to
291 ms.**

This was worth measuring rather than assuming in either direction, and the result
is the unflattering one. The window sorts every *matched* row, and that work has
nothing to do with how many merges exist: two rows needing deduplication out of
forty thousand matches cost exactly what forty thousand would.

So the fix buys latency for a case up to its first merge and nothing afterwards.
**Merging records is the thing this product exists to support.** Every case
leaves the population this helps, permanently, the first time an analyst does the
central thing.

Stating it that way rather than "2.5× faster search" is the point. The headline
is true and the qualification is the part that predicts what a user experiences.

**R-014 stays open, re-scoped.** The sharper diagnosis: collapsing is applied
globally when it is needed for a vanishing subset of rows. A row whose
`subject_id` never appears in `er_merge_map` cannot have an alias and needs no
partition — so an indexed anti-join could send only genuinely mergeable rows
through the window and let the rest stream. **Untested**, and a design sketch
rather than a plan until it is measured.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    314 passed, 0 failed, 2 ignored     (313 before; +1)

cargo test -p kokin-osint --test scale --release -- --ignored
    1 passed, in 249s

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
python scripts/lint_crossrefs.py        OK: 576 references across 98 files
python scripts/lint_sql_ordering.py     OK across 49 files
python scripts/lint_research.py         OK: 24 sources, 18 claim rows
```

## Manual verification

**A/B — the guard.** Forced `collapse_aliases` to return `false` unconditionally.
Result: **four** tests failed —
`a_merged_entity_is_one_result_not_two`,
`splitting_puts_the_second_result_back`,
`merging_two_entities_does_not_merge_what_they_hold` and
`the_count_and_the_facets_agree_with_the_list`.

That is the reassuring outcome. The merged path was already covered by tests
written in increment 13, so the 314 passing after the change means something —
had the suite been silent, the fix would have been unverifiable and the speedup
would have been the only evidence for it, which is no evidence at all.

**The new test covers what the existing ones cannot.**
`skipping_the_collapse_changes_nothing_until_something_is_merged` checks the same
case on both sides of its first merge: resolving and not resolving must agree
before, and the collapse must resume after. The second half is the load-bearing
one — a guard stuck permanently off satisfies every assertion before the merge,
and "we silently stopped collapsing aliases" is invisible to a case that has none.

## Known limitations

1. **The merged path is unchanged**, and it is the path every case ends up on.
2. **One merge was measured, not many.** The claim that cost is independent of
   merge count follows from the window sorting matched rows, and only the
   one-merge point is measured. A case with a thousand merges was not tested.
3. **The existence probe runs on every search, `count` and `facet_counts`** —
   three per query page. It is an index probe against a partial index, and it did
   not show up in the measurements, but it is not free and it is per-call rather
   than cached for the session.
4. **`collapse_aliases` takes a connection**, so `one_row_per` is no longer a
   pure string builder in practice — the decision is made by its callers. That is
   the right place for it and it does mean three call sites must remember to make
   it. Nothing enforces that a fourth would.
5. **The 350 ms ceiling on the merged path is a regression guard, not a target.**
   It exists so the merged case cannot get worse unnoticed; it is not an
   acceptance of 291 ms.

## Next recommended increment

**Increment 22 — the UI foundation**, and Milestone B with it: the typed binding
layer over `invoke`, the app shell, error handling that branches on `ErrorCode`
rather than on message prose (D-028), and the case lifecycle — create with the
one-time recovery-key display that must be impossible to skip past (D-026), open,
unlock by recovery key, close.

Twenty commands have had no caller for six increments. This is the first one
whose output a person can look at, and the first that can begin to close A-031.
