# Increment 9 — search

Reference-workflow step 6. L0, L1 and L2 all held rows after increment 8, and
none of them could be found without already knowing an id. This increment makes
a case answerable.

The index is a **projection**, never a source of truth. Every row in it is
derived by a trigger from a row in L1 or L2, and `rebuild()` reproduces it from
those rows. That is what licenses the index to be lossy, denormalised and tuned
for retrieval: nothing is knowable only from it.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-store/src/migrations.rs` | Migration 5: `search_document`, the `search_index` FTS5 table, 14 triggers, and a backfill. One new test. |
| `crates/kokin-store/src/golden_schema.txt` | Re-blessed. |
| `crates/kokin-store/src/lib.rs` | `PRAGMA temp_store = MEMORY` on every case connection, plus the test that pins it. |
| `crates/kokin-search/src/query.rs` | New. User text → FTS5 expression, and the reason that is not a one-liner. |
| `crates/kokin-search/src/lib.rs` | New. `search`, `count`, `facet_counts`, `rebuild`, `check`, `dangling_documents`. |
| `crates/kokin-search/tests/search.rs` | New. 14 integration tests over the real pipeline, plus an ignored scale measurement. |
| `crates/kokin-search/Cargo.toml`, `README.md` | Populated from the stub. |
| `docs/threat-model/attack-catalog.csv` | A-020 (query injection), A-021 (temp-file spill). |
| `docs/decision-log.csv` | D-015 (index shape), D-016 (a document owns its text). |

No new third-party dependency. `kokin-search` uses `rusqlite` and `thiserror`,
both already in the tree.

## The four ideas

**1. A document carries only the text its own row owns.**

The tempting alternative was to index a relationship by the display names of the
entities it joins, because `Acme director` is what an analyst would actually
type. It is rejected (D-016): borrowing another row's text makes this document's
correctness depend on a row it does not control, so renaming an entity would
leave a name searchable that the case no longer holds. Stale names in an
investigation tool are worse than absent ones — an absent result prompts another
search, a stale one ends the question. The cost is stated plainly in the
limitations below.

**2. Superseded observations are flagged, not removed.**

An extractor rerun withdraws its earlier observations (ADR-0006). Deleting their
documents would erase the ability to ask what the case used to say. They are
carried with `superseded = 1`, hidden by default, and **labelled** when asked
for. The failure this prevents is specific and nasty: without it, a rerun
doubles every result list, and the duplicates read as corroboration — two
independent sightings of one fact.

**3. User text never reaches FTS5 as syntax.**

Every chunk is emitted as a double-quoted phrase, so `NOT`, `OR`, `NEAR`, `*`,
`:` and parentheses become literal text for the tokeniser to discard. Structure
comes only from what the query builder decides. This is A-020, and the injection
that matters is not a crash — it is a typed `title :` quietly narrowing the
search so a shorter result list looks like a finding about the world.

**4. The index can always be rebuilt, and can be asked whether it is right.**

`rebuild()` re-derives it; `check()` runs FTS5's own integrity check;
`dangling_documents()` finds any document pointing at a row that no longer
exists. A hit that opens nothing is a result an analyst cannot check, so the
projection has to be auditable against the rows it claims to describe.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    189 passed, 0 failed, 1 ignored

    48 net · 33 store · 17 extract-hostile · 16 graph-l2 · 14 blob · 14 search-integration
    · 12 ingest · 11 search-query · 10 extract · 9 keys · 5 graph-scales

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check              advisories ok, bans ok, licenses ok, sources ok
scripts/lint_crossrefs.py     OK: 205 internal references across 32 files
scripts/lint_research.py      OK: 24 sources; 18 claim rows
```

The one ignored test is the scale measurement below, which takes ~45 s in
release and would make the suite useless as a fast check.

## Measured: does it hold at 100,000 observations?

ADR-0003 commits to "cases with at least 100,000 observations remain
searchable" and p95 under 200 ms. Measured on **AMD Ryzen 5 3600XT, Windows 11,
release build, real SQLCipher-encrypted case**:

```
100,000 observations | index built in 8.8 s
selective queries  p50 296 µs      p95 3.1 ms
broad queries      p50 235 ms      p95 247 ms
```

**Selective queries meet the budget with about 65× to spare.** Any query that
narrows the case at all — an address, an identifier, a name — is effectively
instant.

**Broad queries do not meet it: p95 247 ms against a 200 ms commitment.** This
is reported rather than tuned away. The cause is not tail latency (p50 is
235 ms, so every such query is slow) and not I/O: BM25 must score every matching
document, and in this corpus the broad terms match all 100,000.

Investigated before accepting:

| Variant | Broad-query cost (in-memory, unencrypted) |
| --- | --- |
| `COUNT(*)`, match only, no ranking | 4.3 ms |
| Rank + `LIMIT 50`, no join | 66.9 ms |
| **Current shape**: join + filter + `ORDER BY bm25()` + snippet | **81.0 ms** |
| Same but `ORDER BY rank` | 119.6 ms |
| Same but weights via `rank MATCH 'bm25(...)'` | 119.8 ms |
| Join + `ORDER BY rank`, no snippet | 117.8 ms |

Two things fell out of this. FTS5's `ORDER BY rank LIMIT` top-N optimisation
does **not** engage through the join, so `ORDER BY rank` is *slower* than
`ORDER BY bm25()` — the current shape is already the best of the six. And
`snippet()` is not the cost (compare rows 5 and 6); scoring is. A 32 MB page
cache was tried and moved p95 by 6 ms, so it was reverted rather than cargo-
culted: the cost is CPU, not decryption of cache misses.

The corpus is pathological in proportion — every document contains the broad
terms — but not in shape: the central subject of an investigation really does
appear in most of its documents. This is the right worst case to hold ourselves
to, and we currently miss it. The scale test asserts the 200 ms budget on
selective queries and guards broad queries at 500 ms as a *regression* ceiling,
with a comment saying exactly that, so the loosened number cannot later be
mistaken for a met commitment.

## Security and privacy implications

**A-020, query injection — mitigated.** Covered above; `no_single_character_can_escape_the_quoting`
sweeps 30 punctuation characters in 6 positions each against a real FTS5 parser.

**A-021, plaintext temp-file spill — mitigated, and it was latent.** SQLCipher
encrypts the database; it does not encrypt the temp files SQLite writes when a
sorter outgrows memory, and search is exactly the workload that spills. Nothing
was leaking — the bundled amalgamation is compiled `SQLITE_TEMP_STORE=2`, which
defaults to memory — but that guarantee rested entirely on a vendored
dependency's build flags, with nothing in this repository asserting it. A
libsqlite3-sys bump could have moved it to disk without a single test failing.
`PRAGMA temp_store = MEMORY` is now set explicitly and the test asserts both the
effective value and the compile-time default.

**Searches are deliberately not audited.** A record of every query an analyst
typed is a record of every suspicion they entertained and discarded. It is more
sensitive than the case, it cannot be crypto-shredded per subject, and it would
be the first thing read by anyone who seized the case. Saved searches, when they
arrive, must be an explicit act with an explicit deletion story.

**The FTS index inherits page-level encryption for free** (ADR-0004) because it
lives in the same database file. No separate protection is needed, and none
should be added that would move index content outside it.

**Relevance is not confidence.** `Hit::relevance` is BM25 and nothing else. A
page mentioning a name forty times outranks the registry filing that proves it.
It is named `relevance` rather than `score`, and the crate documentation says a
UI must not render it beside a confidence dimension where the two could read as
the same kind of quantity.

## Known limitations

1. **Artifact body text is not indexed.** Only observations and L2 rows are.
   The bytes of a captured page live in an encrypted blob outside the database
   and there is no text-extraction transform yet, so "search across the entire
   case" currently means "across everything the case has *recorded about* its
   documents", not their full text. This is the largest gap in the increment.
2. **Broad queries exceed ADR-0003's 200 ms p95** — measured 247 ms at 100k.
   Either the ADR is revised with the two regimes separated, or the query path
   needs a genuine top-N path. Not resolved here.
3. **Relationships are not reachable by their endpoints' names.** The deliberate
   consequence of D-016. Reaching an edge means finding an entity and following
   it. A UI can paper over this by expanding hits into their edges; the index
   will not do it.
4. **Paging is `LIMIT`/`OFFSET`.** Fine at this scale, but a deep offset
   re-scores everything before it. A cursor over `(relevance, id)` is the fix
   when it matters.
5. **No `OR` between terms.** Multiple words are always conjunctive. `-x`
   excludes. Anything richer waits until there is a UI to express it.
6. **`facet_counts` scores the whole match set**, so it costs the same as a
   broad search. Calling it alongside `search()` roughly doubles the work.
7. **No stemming.** `director` does not match `directors`. Porter stemming is
   one tokenizer argument away but changes what every existing index means, so
   it belongs to a migration, not a flag.
8. **`search_document.subject_kind` is unconstrained**, so a bad direct write
   could create a document routing nowhere. `dangling_documents()` detects it;
   nothing prevents it. Same trade as `evidence_link.subject_kind` in migration
   4, for the same reason: SQLite cannot `ALTER` a `CHECK`.
9. **The scale test writes observations straight to SQL**, so it measures the
   read path only and says nothing about what building an index that size costs
   through the real extractor.

## Manual verification

Three A/B controls. Each one broke a claimed control and confirmed that exactly
the expected tests failed, and nothing else did.

**A/B 1 — the backfill.** Neutered the four backfill statements in migration 5
(`WHERE 0`). Result: `upgrading_a_populated_case_makes_its_existing_rows_searchable`
failed with `observation: 3 rows backfilled to 0 documents`, and **every other
test in the store suite still passed**. That is the finding, not a footnote: a
fresh case backfills nothing, so no test that starts from `migrate()` can ever
catch a missing backfill. This one test is the only thing standing between an
upgraded case and an index that silently omits everything collected before the
upgrade — a failure that presents as search working normally and returning
fewer results than the truth.

**A/B 2 — phrase quoting.** Replaced `phrase()` with `text.to_string()`. Result:
9 of 11 query tests failed. The two survivors were
`an_empty_query_matches_nothing_rather_than_everything` and
`a_query_of_only_exclusions_matches_nothing`, which do not depend on quoting —
correct, and a useful check that the failure set is precise rather than total.

**A/B 3 — the projection triggers.** Neutered three at once: supersession no
longer marked the index, entity renames no longer reindexed, entity deletes no
longer unindexed. Result: exactly `a_rerun_does_not_double_every_result`,
`renaming_an_entity_changes_what_finds_it` and
`deleting_an_l2_row_removes_it_from_the_index` failed. The other 11 integration
tests passed, including `the_index_agrees_with_the_case_after_a_workload` —
correct, since that test deletes nothing and so cannot observe a broken delete
path.

One test was strengthened before it was trusted. `hostile_input_never_becomes_syntax`
originally asserted on the *shape* of the generated string — that quotes
balanced after stripping the scaffolding. That is true by construction and says
nothing about whether FTS5 accepts the result. It was rewritten to execute every
hostile input against a real FTS5 table, and `no_single_character_can_escape_the_quoting`
was added to sweep punctuation systematically rather than relying on a list
someone thought of. Both were then confirmed to fail under A/B 2.

## Next recommended increment

**Increment 10 — artifact text extraction and indexing**, which closes
limitation 1 and is the difference between "search the case" as implemented and
as promised. It needs a text transform in `kokin-extract` emitting page text
into L1, at which point the existing trigger indexes it with no change to this
crate. It also creates the corpus needed to re-measure the broad-query regime
against realistic term distributions rather than the pathological one used here.

The alternative candidate remains the `er_*` tables (A-007, ADR-0008). Search
still argues for going first: entity resolution needs enough entities in a case
for merging to be a real question, and nothing yet creates them in bulk.
