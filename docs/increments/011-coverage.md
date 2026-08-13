# Increment 11 — what the case has not read

Increment 10 gave documents a prose budget and, past it, truncated. The
truncation was counted, returned to the caller, and written to the audit log —
and then went nowhere anybody looks. A case could be only partly searchable and
answer a search in exactly the words it uses for a case that holds nothing.

That is the failure this product exists to prevent, arrived at from the inside.
This increment makes it visible.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-store/src/migrations.rs` | Migration 6: `run_subject`, `coverage_gap`, four append-only triggers, a backfill. One new test. |
| `crates/kokin-store/src/golden_schema.txt` | Re-blessed. |
| `crates/kokin-extract/src/lib.rs` | Records what each run was about and what it knows it missed. Two gap-kind constants. |
| `crates/kokin-search/src/coverage.rs` | New. `coverage`, `incomplete_artifacts`, `gaps_for_artifact`. |
| `crates/kokin-search/src/lib.rs` | The module, and the crate note that an empty result list is not a finding. |
| `crates/kokin-search/tests/coverage.rs` | New. 10 integration tests over the real pipeline. |
| `docs/threat-model/attack-catalog.csv` | A-022 closed; A-023 added. |
| `docs/decision-log.csv` | D-018 (`run_subject`), D-019 (gaps scoped to the latest run). |

No new dependency.

## Two tables, because two different things were missing

**`run_subject` — what a run was about.** `derivation` records what a run
*produced*, which cannot answer this question: a run that produced nothing
writes no edge, so a refused PDF, a blown limit and an empty document all look
identical to an artifact nobody has opened. One of those is finished work and
the other is a job still to do (D-018). It is written before anything can fail
and outside the transaction `record` opens, so it survives every path out of
extraction — including the failing ones, which are the ones it exists for.

**`coverage_gap` — what a run that *succeeded* knows it did not cover.** Runs
that failed outright already say so in `transform_run.status` and `error_code`;
duplicating that here would create two places to disagree about one run. The gap
this table is for is the quiet one: the run reports success, the observations
look complete, and part of the document was never read.

Both are append-only, on the same grounds as `derivation`. `coverage_gap` also
carries a `CHECK ((magnitude IS NULL) = (unit IS NULL))`, because a magnitude
with no unit gets read as whatever the reader expects and the reader expects the
flattering number. `NULL` magnitude stays available for the genuine case — a
parser that stopped early cannot say how much was left — and that is different
from zero.

## The bug this increment caused, and how it was found

The first run of the new suite was 9 for 9. The second was 8 for 9, on
unchanged code.

`coverage` picks the latest run per artifact with
`ORDER BY started_utc DESC, id DESC`. `started_utc` is a second-resolution ISO
string and a re-extraction opens both runs inside one second, so the tie-break
decides — and run ids are `blake3` of 16 random bytes, with no relation to time.
The ordering was a coin flip.

The test flapped. What it means in a case is worse: a coverage report answering
from a superseded reading, telling an analyst that a fully re-read document is
still truncated, or that a truncated re-read is complete. It is wrong in both
directions and looks authoritative in both. Filed as A-023.

The fix orders by `rowid`, SQLite's insertion order, which is genuinely
monotonic in the order runs opened. A `VACUUM` may renumber it, but only by
copying rows in existing rowid order, so the ordering — the only thing read here
— survives.

`a_rerun_in_the_same_second_still_supersedes_the_reading_before_it` pins it
deterministically. It is the one test in the file with hand-written rows,
because the case under test is ids sorting *opposite* to insertion order, and
real ids are random: the pipeline cannot be asked for that on demand. It gives
it roughly half the time, which is exactly how much use a coin-flip test is.

## Coverage is case-scoped, and cannot be query-scoped

The report an analyst actually wants is *"your search found three hits, and two
documents nobody has read might have held a fourth."* Nothing can produce it.
Deciding whether an unread document matches a query means reading it, at which
point it is not unread.

So the honest statement is the weaker one — *this case holds documents that no
search of it can reach* — and it has to be attached to every result list, not
only to searches that come back empty. A search returning forty hits from a case
that is 60% read is the more misleading of the two, because nobody interrogates
a full page of results.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    210 passed, 0 failed, 1 ignored

    48 net · 34 store · 24 extract-hostile · 16 graph-l2 · 16 search-integration
    · 14 blob · 12 ingest · 11 extract · 11 search-query · 10 search-coverage
    · 9 keys · 5 graph-scales

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check              advisories ok, bans ok, licenses ok, sources ok
scripts/lint_crossrefs.py     OK
scripts/lint_research.py      OK: 24 sources; 18 claim rows
```

Tests worth naming:

- `prose_past_the_budget_is_visible_at_the_point_of_search` — the increment in
  one test. The case answers `ZK-9930` with silence, and separately admits the
  silence is about itself.
- `a_document_read_and_found_empty_is_not_a_document_nobody_read` — the
  distinction `run_subject` exists for, asserted against a real extraction that
  really produces nothing.
- `a_document_the_extractor_refused_is_reported_not_forgotten` — a real PDF
  through a real refusal, and the error code survives to the report so
  "we cannot read this format" stays distinguishable from "this file is broken".
- `a_rerun_that_reads_the_whole_document_clears_the_gap` — and the superseded
  run's gap is still on record afterwards.
- `a_rerun_that_still_falls_short_does_not_report_the_gap_twice` — counting gaps
  instead of documents would make a case look worse every time it was
  re-examined, which trains people to stop re-examining.

## Manual verification

Three A/B controls, each confirming exactly the expected tests failed.

**A/B 1 — the backfill.** Neutered migration 6's `INSERT ... SELECT FROM
derivation` with `WHERE 0`. Result: exactly one of 34 store tests failed
(`upgrading_a_case_recovers_which_artifacts_a_run_had_read`), and the whole
`kokin-search` coverage suite passed. The same finding as increment 9's A/B 1,
and it holds for the same reason: a fresh case backfills nothing, so no test
starting from `migrate()` can catch a missing backfill. One test stands between
an upgraded case and a coverage report that says every document it collected
before the upgrade has never been read.

**A/B 2 — the tie-break.** Restored `ORDER BY ... id DESC`. Result:
`a_rerun_in_the_same_second_still_supersedes_the_reading_before_it` failed
deterministically and `a_rerun_that_reads_the_whole_document_clears_the_gap`
failed on that run's coin flip. The other 8 passed, which is right — none of
them opens two runs over one artifact.

**A/B 3 — recording gaps.** Made `record_coverage_gaps` write nothing. Result:
exactly the 4 gap-dependent tests failed. The 6 that passed include
`a_rerun_in_the_same_second_...`, which writes its own gap row by hand and so
cannot observe the extractor's silence — a useful check that the failure set is
precise rather than total.

## Known limitations

1. **Coverage is about artifacts, not about sources.** A site that was never
   crawled produces no artifact, so nothing here counts it. "We hold nothing
   from this domain" is a collection-plan question and needs the collection plan
   to exist first.
2. **`Complete` means no gap was reported, not that the document was
   understood.** Everything here rests on transforms recording their own
   shortfalls honestly. A transform that silently drops what it cannot handle
   produces a case that reports itself fully covered — the one failure mode this
   increment cannot detect from the outside.
3. **Upgraded cases under-report attempts** until re-extraction, because runs
   that produced nothing left no trace to recover. Pinned by a test so the error
   direction cannot silently reverse: it reports work as still to do, never as
   already done.
4. **`kokin-ingest` writes no `run_subject` rows.** Its runs are about captures
   and blobs, and nothing yet asks about their coverage, but that means this
   table describes extraction only and should not be read as a general
   "what did this run touch" index.
5. **No UI renders any of this.** There is no UI to render search either, so
   this is not a regression — but A-022 is closed on the strength of the figure
   being available and tested at the library boundary, not on anyone having seen
   it.
6. **`incomplete_artifacts` takes a limit and no cursor.** A case with thousands
   of unread documents shows the first page and no way to walk the rest.
7. **Gap kinds are extractor-defined strings** with no registry. Two transforms
   could invent different codes for the same shortfall, and nothing would
   notice.

## Next recommended increment

**Increment 12 — the `er_*` tables (A-007, ADR-0008).** The reason to defer
entity resolution twice was that a case needed enough entities for merging to be
a real question. Increments 9 through 11 built the machinery that produces them
in bulk and can now say how much of a case they were drawn from, which is
exactly the context a merge decision needs: merging two entities on the strength
of a case that is 60% read is a different act from merging them on a case that
is fully read, and until this increment there was no way to tell those apart.

The alternative candidate is a text transform for PDFs, which limitation 2 of
increment 10 and the `Failed` state added here both point at. It is the more
obviously useful feature and the less structurally important one: the `er_*`
tables are a schema commitment that gets harder to make the more L2 rows exist,
and PDF extraction is a transform that can be added at any time without moving
anything already built.
