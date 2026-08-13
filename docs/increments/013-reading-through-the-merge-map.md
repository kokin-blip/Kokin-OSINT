# Increment 13 — reading through the merge map

Increment 12 built entity resolution and closed by admitting that nothing read
it: merging two entities changed no query's answer. This makes the merge mean
something. It also closes 12's limitation 4 (no audit event), which was folded
in here as planned rather than deferred.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-search/src/lib.rs` | `Hit::canonical_id`, `SearchOptions::resolve_merged`, and `one_row_per` — the collapse every read in the crate now goes through. |
| `crates/kokin-search/tests/search.rs` | 5 new tests. |
| `crates/kokin-graph/src/resolution.rs` | `evidence_for_cluster`, `ClusterEvidence`, audit events on `merge`/`reject`/`split`. |
| `crates/kokin-graph/src/lib.rs` | `evidence_for` ordering fixed; the assessment audit payload built rather than interpolated. |
| `crates/kokin-graph/tests/resolution.rs` | 3 new tests. |
| `crates/kokin-store/src/audit.rs` | Payloads validated and canonicalised before hashing. 2 new tests. |
| `scripts/lint_sql_ordering.py` | New. |
| `.github/workflows/ci.yml` | Runs it. |
| `docs/threat-model/attack-catalog.csv` | A-025, A-026 added, both mitigated. |
| `docs/decision-log.csv` | D-022 (the lint), D-023 (two evidence reads, not one). |

No new dependency, and no migration: everything here is a read.

## Two rows for one person is not a display bug

It is a claim. Two entities matching a search separately, each carrying its own
evidence, read as two independent accounts of the same fact — and two accounts
agreeing is the strongest thing an investigation can find. A-025 files it beside
A-021, the re-extraction that doubled every result: same false corroboration,
reached from the analytical layer instead of the lineage one.

So the collapse is not decoration on `search`. `count` and `facet_counts` go
through the same `one_row_per` helper, because a total that disagrees with the
list under it is worse than no total and both are read in one glance.
`the_count_and_the_facets_agree_with_the_list` asserts that in both modes, and
then asserts the two modes actually differ — without that last line the whole
test passes vacuously against a collapse that does nothing.

`resolve_merged = true` is the default deliberately. The other setting is for
auditing a merge, and asking for it is the specialised request.

## Collapsing before paging, not after

The obvious implementation deduplicates the returned page in Rust. That silently
shrinks pages: ask for fifty, get forty-seven, because three were aliases of each
other — and the caller has no way to tell a short page from the last one.
`paging_is_not_shrunk_by_collapsing` walks the whole result set one row at a time
and fails on the first short page.

This is also where D-020 finally earns its cost. The collapse is
`LEFT JOIN er_merge_map ... AND active = 1`, one indexed lookup per hit. A merge
map that allowed chains would put a recursive CTE on the hot path of every query
in the crate. Nothing had joined through that invariant until now.

## The evidence of a person, and the evidence of a row

Merging moves no `evidence_link`, so a UI showing the survivor with
`evidence_for` displays the survivor's material and quietly omits the absorbed
record's — the case's strongest evidence about that person disappearing at the
exact moment they were identified.

`evidence_for_cluster` gathers the cluster and keeps `entity_id` on every result
(D-023). Flattening that away would discard the answer to "which of these two
records did that come from", which is the question a merge makes hard to ask and
the question ADR-0008 exists to keep answerable. `evidence_for` was left alone:
resolving it silently would remove the row-level read an analyst auditing the
merge itself needs.

## The audit chain hashed strings it had never parsed

`append` took `payload_json: &str` and hashed it as bytes. Its doc comment
claimed the payload was re-encoded canonically. It was not, and two things
followed from that.

A payload built by `format!` from a merge rationale — free analyst text, quotes
and braces and newlines included — could store something that is not JSON at
all. The chain would hash it, verify it forever, and count it as an event, while
the payload answered no question it was written to answer. **The record passes
every integrity check this product performs and is unreadable** (A-026). And two
payloads differing only in key order hashed differently, so "the same event"
was not a stable notion.

`append` now parses, refuses anything that is not a JSON object, and re-encodes
before hashing. Callers build payloads with `serde_json`. The one interpolated
payload left in the codebase — `assessment.recorded` — was converted at the same
time; it was safe by accident, because the values it interpolated came from a
validated scale, and safe by accident is the kind of thing that stops being true
when someone adds a field.

`a_rationale_full_of_json_does_not_corrupt_the_chain` puts quotes, braces, a
backslash escape and a newline in a merge rationale and reads it back.

## The third time is a lint

`evidence_for` ordered by `created_utc, id` and its doc comment promised
insertion order. It is the same defect as A-023 and D-021, in a third crate,
and this time it was not found by a failing test — increment 12's write-up said
"worth a lint if it happens a third time", and grepping for the pattern found it
in ninety seconds.

`scripts/lint_sql_ordering.py` fails the build on `ORDER BY <col>_utc` followed
by an id tie-break (D-022). `audit_event.id` is allowlisted by name because it
is an `INTEGER PRIMARY KEY` — a rowid alias, and the one id column in this schema
that means what an ordering wants it to mean.

Why a lint and not review: review is what was in place for the first two, and
both were found by intermittent test failures instead. The wrong version reads
as obviously correct, which is exactly why it kept being written.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    232 passed, 0 failed, 1 ignored

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
python scripts/lint_sql_ordering.py    OK across 28 files
python scripts/lint_crossrefs.py       OK: 251 references across 36 files
```

## Manual verification

**A/B 1 — the collapse.** Forced `one_row_per` to take its pass-through branch.
Result: exactly `a_merged_entity_is_one_result_not_two`,
`merging_two_entities_does_not_merge_what_they_hold`,
`splitting_puts_the_second_result_back` and
`the_count_and_the_facets_agree_with_the_list` failed. 18 of 22 search tests
still passed, so the control is scoped to what it claims and does not disturb
the rest of the crate.

**A/B 2 — the merge audit event.** Compiled it out with `#[cfg(any())]`. Result:
`every_resolution_decision_reaches_the_audit_chain` and
`a_rationale_full_of_json_does_not_corrupt_the_chain` failed, and nothing else
in the crate did.

**A/B 3 — payload validation.** Made `canonical_payload` return its input.
Result: exactly the two new audit tests failed; 38 of 40 store tests passed.

**A/B 4 — the lint.** Reintroduced both real historical bugs — `created_utc, id`
in `evidence_for` and `started_utc DESC, r.id DESC` in `coverage` — and ran the
lint. It reported both with correct line numbers and exited 1, including the
`coverage` one, which is spread across four lines inside a window function. This
is the A/B that matters most here, because a lint that cannot catch the bug it
was written for is worse than none: it looks like coverage.

## Known limitations

1. **A cluster spanning facets is counted under its representative's facet.**
   Merging a `person` row with an `organisation` row files the result under
   whichever alias ranked best. Real ambiguity, and picking the row the list
   already shows is the least surprising of the available wrong-ish answers —
   but it means facet counts are not a partition of the underlying rows.
2. **`assessments_for` still works in raw entity ids.** Assessments recorded
   against an absorbed entity are invisible from the survivor. Smaller than the
   evidence gap because assessments are per-subject by design and merging two
   subjects' assessments raises a question ADR-0007 has no answer for: two
   different values on one dimension do not combine, and there is no composite
   to fall back on. Deliberately left until there is a UI to argue with.
3. **The merge map is joined, never cached.** Fine at investigation scale and it
   is one indexed lookup, but the p95 figure in `docs/increments/009-search.md`
   was measured before this join existed and has not been re-measured.
4. **`open_candidates` still treats any decision as closing a pair** —
   increment 12's limitation 5, untouched.
5. **The ordering lint is textual.** It reads `ORDER BY`, not SQL. A clause built
   from a runtime string, or one whose terms are assembled by `format!`, is
   invisible to it. The three known occurrences were all literals, so this
   catches the pattern as it has actually appeared, which is not the same as
   catching the pattern.

## Next recommended increment

**Increment 14 — the first entity-resolution rule.** Everything can now be
proposed, decided, read through and audited, and every proposal must still be
made by hand (increment 12's limitation 3). The obvious first rule — two entities
sharing a normalised identifier — is a query away, and it is the one that turns
`er_candidate` into something with volume in it, which is what will show whether
`open_candidates` and its no-reopening limitation survive contact with real use.

The prerequisite is a queue an analyst can work through, so this is the point at
which the UI stops being deferrable.
