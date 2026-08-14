# Increment 20 — the scale probe, and three queries I wrote badly

R-003 has been open since gate 3: ADR-0003 chose SQLite with FTS5, ADR-0005
chose relational adjacency over a graph database, and neither had been measured
against the spec's target of *"cases with at least 100,000 observations remain
searchable"* at a p95 of 200 ms.

It is now measured. **Both ADRs stand**, and the three slowest numbers in the
run were my queries rather than the storage engine.

## Files changed

| File | What |
| --- | --- |
| `src-tauri/tests/scale.rs` | New. The probe, `#[ignore]`d, run deliberately with `--release`. |
| `docs/benchmarks/p2-scale.md` | New. The measurements and what they mean — the first file in an empty directory. |
| `docs/risk-register.csv` | R-003 → `mitigated` with the measurement cited; **R-014** opened. |
| `docs/decision-log.csv` | D-036: why the probe is a test and not a `poc/` project. |

No dependency changes.

## The case under test

100,000 observations, 20,000 entities, 300,000 relationships, 200 artifacts.
Seeded in ~31 s. **438 MiB on disk**, which is a figure nothing had before.

Loaded through the real schema so every trigger fired — an `observation` insert
fires into `search_document`, which fires into `search_index`. A bulk load around
the side would have measured an FTS index that the product's own write path never
built, which is the one thing this probe must not do.

Two schema strictnesses caught the seed before it could produce numbers:
`transform_run.transform_name` rather than `transform`, and — more interestingly
— `nothing in L2 exists without evidence: write the evidence_link before the
entity`. The invariant is enforced at **insert order**, so an L2 row cannot exist
for even one statement without its grounding. All 320,000 seeded rows had to
write evidence first.

## The graph: ADR-0005 holds, and the obvious query is a trap

Three forms of the same 3-hop expansion span **1,235×**.

| Form | p95 |
|---|---|
| `reach.id IN (r.from_entity, r.to_entity)` | 84,398 ms |
| Two indexed union branches | 548 ms |
| **Bounded per hop, in the host** | **68 ms** |

`IN (from_entity, to_entity)` reads naturally and expresses an undirected edge in
a single branch. It also uses **neither index** — SQLite has `relationship_from`
and `relationship_to` separately, and an `IN` across two columns is a lookup on
neither, so every recursion step full-scans 300,000 rows.

Rewriting it as the union of two directed traversals makes each branch an index
lookup: **152× faster**, and still 2.8× over budget.

The rest is not indexing. Both CTE forms compute the **entire 3-hop closure and
then `LIMIT` it**, so from a hub the limit describes what is returned, not what
was computed — exactly the *"uncontrolled graph explosion"* the spec names as a
thing to prevent. Walking hop by hop and stopping at the render budget costs
68 ms, and returns 196 nodes rather than 1,603. That difference is the feature,
not a caveat.

Worth stating plainly: **a single recursive CTE also cannot be cancelled or
rendered progressively**, both of which the spec requires. The form that is fast
is the form the product needed anyway.

## Search: FTS5 is fine, alias collapsing is not

FTS5 answers a common term over 100k observations in **117 ms**. The default path
costs **291 ms**, and the 174 ms difference is `resolve_merged`.

It is **not** the `LEFT JOIN` on `er_merge_map` that the module comment
identifies as the expense — that is one indexed lookup per hit. It is the window
function in `one_row_per`:

```sql
ROW_NUMBER() OVER (PARTITION BY subject_kind, resolved_id ORDER BY relevance, subject_id)
```

which must materialise and sort **all ~40,000 matched rows** before `LIMIT 50`
can apply. With `resolve_merged: false` SQLite streams and stops at 50.

This is not a careless query. Collapsing before limiting is deliberate and argued
in the code — doing it afterwards silently shrinks pages, so a caller asking for
fifty gets forty-seven when three are aliases. The cost was simply never measured
at this size.

**The case under test had zero merges.** Paying 174 ms to deduplicate against an
empty map is the specific waste, and it names the remedy: skip the window when
the case holds no active merge, which preserves semantics exactly because there
is nothing to collapse.

**Recorded as R-014 and deliberately not fixed here.** Measuring and fixing in
one increment is how a benchmark comes to justify the change it was used to
design.

## `coverage()`: the worry was unfounded

Increment 16 made coverage non-optional on every result list (D-029) and flagged
it as *"recomputed on every search… per-keystroke work if typeahead is ever wired
to this"*. It is **0.2 ms**, roughly 1,400× cheaper than the search beside it.

## Three measurement kinds, so that two sentences stay different

Two of the slowest numbers are queries this product must never issue, and
asserting on them would fail the probe for demonstrating what it was written to
demonstrate. So each measurement is a `Contract` (must hold the budget), a
`CounterExample` (recorded, never asserted), or a `TrackedGap` (over budget, with
a named risk and a ceiling from measurement, so a regression fails even before
the fix lands).

The distinction is the point. *"This query is slow"* and *"this query must not be
slow"* are different statements, and moving a query between those categories is a
decision with a paper trail rather than a threshold edit.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    313 passed, 0 failed, 2 ignored     (313 before; the probe is the second ignored test)

cargo test -p kokin-osint --test scale --release -- --ignored
    1 passed, in 247s

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check                        advisories ok, bans ok, licenses ok, sources ok
python scripts/lint_crossrefs.py        OK: 554 references across 97 files
python scripts/lint_sql_ordering.py     OK across 49 files
python scripts/lint_research.py         OK: 24 sources, 18 claim rows
```

## Manual verification

The probe is its own A/B, three times over: the naive CTE, the indexed CTE and
the bounded walk are the same expansion measured against each other, and the
FTS pair differs only in `resolve_merged`. Each isolates one variable, which is
why the diagnosis names a window function rather than "search is slow".

**Two flaws in my own harness, both found by reading its output rather than by a
test.** The first version ran 30 fixed iterations, so a query taking 84 seconds
was sampled 30 times: 43 of the run's 45 minutes went to proving one already
damning number to four significant figures. Sampling is now capped by wall clock
as well as count, and the run is 247 seconds.

The second was a label. A `TrackedGap` that came in under budget printed
`OK (R-014 closed?)`, which reads as a risk being resolved when all it means is
that this particular query does not trigger the gap. It now says
`not triggered here`. A benchmark's output is a claim like any other.

**One reporting error I made to the user during this increment, recorded because
the report is the deliverable:** I read `exit code 0` from a shell pipeline and
announced that every query was within budget. The exit code was `sed`'s; the test
had failed. Numbers from a pipeline are not a verdict.

## Known limitations

1. **One machine, one run.** A Ryzen 5 with an NVMe SSD. A weaker disk moves
   every figure here, and the spec's audience includes investigators on modest
   hardware.
2. **Read-only.** Nothing was measured under concurrent writes, which is what a
   collection job does while an analyst searches.
3. **The hub topology is synthetic and hostile** — every tenth relationship into
   50 hubs out of 20,000. The expansion figures are a pessimistic bound, which is
   right for a risk probe, and "hub expansion is slow" must not be read as "all
   expansion is slow".
4. **Warm cache only.** Each measurement discards one untimed pass, so the first
   search after opening a case is worse than anything reported here.
5. **Vector KNN was not measured because there is nothing to measure.**
   `sqlite-vec` is named in R-003 and ADR-0009 and is not in this workspace — no
   dependency, no lock entry, no embedding. That third of R-003 stays open
   against ADR-0009's own exit condition.
6. **438 MiB for 100k observations is recorded, not judged.** Whether it is
   acceptable depends on an install-size and case-size expectation that the
   decision log still lists as an open owner question.
7. **The bounded walk is in the probe, not in the product.** Nothing ships it
   yet; increment 30 does.

## Next recommended increment

**Increment 21 — R-014, then the UI foundation.** The search fix is now
specified precisely enough to be small: skip the alias-collapsing window when
`er_merge_map` holds no active row, with a test that a case *with* merges still
collapses and a case without still pages correctly.

Then Milestone B begins in earnest — the typed binding layer, the app shell, and
the case lifecycle, which is the first increment in this project's history whose
output a person can look at.
