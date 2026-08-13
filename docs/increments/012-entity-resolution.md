# Increment 12 — entity resolution as a reversible projection

ADR-0008 was accepted in phase 0 and nothing had implemented it. This does.

The conventional merge picks a survivor, repoints the foreign keys and deletes
the absorbed row. That destroys the evidence which argued *against* the merge
along with everything else — and the evidence against is exactly what an analyst
needs six weeks later when they doubt themselves. Nothing here rewrites an
entity. Identity is *read* through `er_merge_map`; split is deactivating a row;
undo is a further decision that leaves the original visible.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-store/src/migrations.rs` | Migration 7: `er_candidate`, `er_decision`, `er_merge_map`, six triggers, the ADR-0008 `CHECK`. Four new tests. |
| `crates/kokin-store/src/golden_schema.txt` | Re-blessed. |
| `crates/kokin-graph/src/resolution.rs` | New. `propose`, `open_candidates`, `merge`, `reject`, `split`, `canonical_id`, `cluster`, `history`. |
| `crates/kokin-graph/src/lib.rs` | Four new error variants. |
| `crates/kokin-graph/tests/resolution.rs` | New. 7 integration tests over the real L2 API. |
| `docs/threat-model/attack-catalog.csv` | A-007 closed; A-024 added and **accepted, not mitigated**. |
| `docs/decision-log.csv` | D-020 (depth-1 merge map), D-021 (which entity survives). |

No new dependency.

## A machine may argue for a merge; only a person may make one

```sql
CHECK (action <> 'merge'
       OR (actor NOT LIKE 'rule:%' AND actor NOT LIKE 'ai:%'))
```

This is the one control in the increment that would lose its meaning in
application code. It holds for every write path, including ones that never call
`kokin-graph`, and `automated_actors_may_propose_a_merge_but_never_decide_one`
proves it by going straight to SQL. `LIKE` is ASCII-case-insensitive in SQLite,
so `Rule:` and `AI:` are caught too.

Rejecting is deliberately left open to automation. The asymmetry is the design:
deciding two people are *different* destroys nothing and can be reconsidered,
while a wrong merge dissolves the distinction between two real people.

**A-024 is filed as accepted, not mitigated.** A `CHECK` constraint reads the
actor string; automation writing `actor = 'user:local'` is a lie rather than a
bypass and no schema can detect it. The audit chain makes it attributable after
the fact. The real control is the convention that anything proposing merges
writes `er_candidate` rows under its true prefix — and a convention is what that
is.

## No similarity score, anywhere

The obvious column is a confidence or similarity number on `er_candidate`. There
isn't one. ADR-0007 admits no composite score in this product, and a number here
would be read as one the moment it was rendered — `0.94` beside "corroborated by
two sources" invites an arithmetic nobody defined. What a rule observed goes in
`rationale`, in words, where it can be disagreed with.

## The merge map is exactly one level deep

Chains make resolution a recursive walk, and a recursive walk over rows an
analyst can toggle is a cycle waiting to happen: `A -> B` plus `B -> A` is two
ordinary-looking merges and an infinite loop in every query that reads identity.
Requiring a canonical to be a root *and* an absorbed entity to be a leaf makes
depth 1 structural, so `canonical_id` is one indexed lookup and a cycle is
unrepresentable rather than merely unlikely (D-020).

Three triggers, because there are three ways in: insert with an absorbed
canonical, insert with a canonical of your own, and reactivate a split row after
the world has moved. The third is the one worth naming — split, merge elsewhere,
then unsplit walks straight around the first two.

The cost lands in the API: joining two clusters is deactivate-then-insert across
every member, inside one decision and one transaction.

## The bug this increment caused, and it is the same bug as last time

Two tests failed intermittently. `older_first` picked the surviving entity by
`created_utc` and then by id — and `created_utc` is a second-resolution ISO
string while entity ids are random hex, so for entities created by one
extraction inside one second **the survivor was a coin flip**.

This is A-023's root cause exactly, one increment later, in a different crate,
found the same way. It is milder — either cluster is correct, so this is
reproducibility rather than correctness — but the same case rebuilt from the
same merges would display a different name, and an analyst should not have to
look to find out which one won.

Fixed by tie-breaking on `rowid`, SQLite's insertion order, and pinned by
`the_older_entity_survives_even_when_both_were_created_in_one_second`, which
forces ids that sort *opposite* to insertion order because real ones are random
and produce that arrangement only half the time.

**Two occurrences is a pattern, not a coincidence.** Every ordering in this
codebase that reads `*_utc` needs a monotonic tie-break, and the reflex of
reaching for the id column is wrong here specifically because ids carry no time.
Worth a lint if it happens a third time.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    221 passed, 0 failed, 1 ignored

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check              advisories ok, bans ok, licenses ok, sources ok
scripts/lint_crossrefs.py     OK: 236 internal references across 35 files
```

Tests worth naming:

- `merging_changes_no_entity_and_destroys_no_evidence` — snapshots every entity
  row before and after and asserts they are identical, then checks the absorbed
  entity's identifier still belongs to *it* rather than to the survivor. This is
  ADR-0008's whole claim in one assertion.
- `automated_actors_may_propose_a_merge_but_never_decide_one` — goes around
  `kokin-graph` entirely, which is the only way to test that the guarantee is in
  the database.
- `the_merge_map_cannot_be_made_to_chain_or_to_cycle` — all four ways in.
- `merging_two_clusters_moves_every_member` — names the *absorbed* members
  rather than the canonicals, because that is what an analyst clicking a result
  list would actually do.

## Manual verification

**A/B 1 — the ADR-0008 `CHECK`.** Replaced it with `1 = 1`. Result:
`automated_actors_may_propose_a_merge_but_never_decide_one` failed, and so did
the golden-schema test — the right pair, one behavioural and one structural.
36 of 38 store tests still passed.

**A/B 2 — the leaf-side depth trigger.** Neutered its `EXISTS` with `AND 0`.
Result: exactly `the_merge_map_cannot_be_made_to_chain_or_to_cycle` and the
golden-schema test failed.

**A/B 3 — absorbing the losing canonical.** Removed
`members.push(absorbed_canonical)` from `merge`. Result: 5 of 7 resolution tests
failed. **This is a coarse control and worth saying so** — without that line a
plain two-entity merge writes no merge-map row at all, because a lone canonical
has an empty cluster, so almost everything fails. It confirms the line is
load-bearing and little else.

## Known limitations

1. **Nothing reads the merge map yet.** `search`, `evidence_for`,
   `assessments_for` and `coverage` all still work in raw entity ids, so merging
   two entities does not yet make a search return one result instead of two.
   This is the largest gap in the increment and the reason for the next one.
2. **`hypothesis_group_id` is a column nothing writes.** ADR-0008 requires
   alternate hypotheses to coexist; the schema supports it and the API has no
   way to express it. Left rather than invented, because designing it without a
   UI to argue with would be designing it twice.
3. **No entity-resolution *rules*.** Everything must be proposed by hand.
   The obvious first rule — two entities sharing a normalised identifier — is a
   query away and deliberately not written yet: rules generate proposal volume,
   and there is no queue UI to absorb it.
4. **`merge` writes no audit event.** Every other decision-shaped act in this
   codebase does. A-024's fallback control is the audit chain, so this must be
   closed before entity resolution is used in anger.
5. **`open_candidates` treats any decision on a pair as closing it**, including
   an old `reject` that a later `propose` disagrees with. A rule finding new
   evidence for a rejected pair cannot currently reopen the question.
6. **Cluster merges are O(members) rows.** Fine at investigation scale, and it
   means a large cluster's history grows quadratically if it is repeatedly split
   and re-merged.

## Next recommended increment

**Increment 13 — reading through the merge map.** Limitation 1 makes this
increment inert: resolution that nothing resolves *by* is bookkeeping. Search
should return one hit for a merged entity, `evidence_for` should gather the
cluster's evidence, and `coverage` is unaffected. It is also where D-020's
promise gets tested for real — the depth-1 invariant exists precisely so this
join is cheap, and nothing has yet joined through it.

Limitation 4 (the audit event) is small enough to fold into that increment
rather than wait, and should be.
