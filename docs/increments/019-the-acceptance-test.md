# Increment 19 — the acceptance test, and the list it tests against

Eighteen increments have been reported as delivering numbered steps of a
"reference workflow". This increment writes that workflow down for the first
time, and then tests it end to end.

## Files changed

| File | What |
| --- | --- |
| `docs/testing/reference-workflow.md` | New. The eleven steps, numbered once, authoritative. |
| `docs/testing/acceptance-plan.csv` | New. One row per step: what must be demonstrated, what drives it, its test, its status. |
| `src-tauri/tests/acceptance.rs` | New. One test walking all eleven steps, twice, offline. |
| `tests/fixtures/acceptance/tungsten-holdings.html` | New. The page step 2 collects. |
| `src-tauri/Cargo.toml` | `kokin-net`, `kokin-export` and `blake3` as **dev**-dependencies. |
| `docs/increments/008-l2.md`, `009-search.md` | Corrected: search is step 8, not step 6. |

No new runtime dependency. `src-tauri`'s shipped dependency set is unchanged —
`kokin-net` appears only under `[dev-dependencies]`, so nothing in the built
application can reach it and the CI import lint is unaffected.

## The numbering had already drifted

The eleven steps were cited in ADR-0012, `docs/testing/strategy.md` and eight
increment reports. They were defined nowhere.

Most reports agreed with the master plan: case creation at 1, ingest at 2–3,
extraction at 4, the analytical layer at 5, close-and-reopen at 9, replay at 11.
**Increments 8 and 9 both called search step 6.** Step 6 is showing a
relationship with its evidence; search is step 8, together with the activity
history. Both reports now carry a correction pointing at the new file.

The correction is trivial. The mechanism that produced it is not, and it is the
third instance of one shape in three increments: **a claim with no single
definition drifts, and every copy of it goes on looking right.** Increment 16
found it in a lint scoped to the wrong directory, increment 17 in nine citations
of decision-log rows that did not exist, and here in a number repeated from
memory across eight documents.

## What the test drives, and what it cannot

Where a Tauri command exists, the test calls it rather than the crate beneath.
The 313 tests in this workspace are otherwise free to reach past the boundary
the interface will stand behind, and a workflow that passes only when driven
from inside the workspace is not the workflow a user performs.

Two steps have no command and are driven directly. Both are pre-existing gaps,
recorded rather than smoothed over:

- **Step 2** — `ingest_url` is not exposed, because `src-tauri` takes no
  `kokin-net` dependency and there is no live transport to hand it (increment
  17, limitation 2). Increment 26.
- **Step 10** — `kokin-export` has no command layer at all (increment 18,
  limitation 4).

## Step 11 is not a twelfth thing to do

`walk_the_workflow` runs once, then again into a fresh directory, and the two
summaries must be equal. Summaries deliberately exclude ids, which are random
per run; what must match is the shape of what the case ended up holding.

Under `ReplayHttp` a cache miss is an error rather than a request, so a pass is
what establishes that steps 2–10 never touched the network. It also establishes
determinism, which nothing previously checked.

## Two dimensions for "accept", because the scale says so

Step 7 was first written against `relationship_confidence`, which failed with
`'probable' is not a value of scale relationship_confidence version 1` — the
error taxonomy working exactly as designed, naming the scale and its version.

Reading the scales settled it better than guessing did. "Accept" is
`review_status = accepted`, whose definition **requires an `audit_event` naming
the actor** and which is `machine_assignable: false`. So the test records both
dimensions — how strong the evidence is, and separately whether a person signed
off — and asserts the audit chain names `user:local`. Storing the value is not
the step; being able to say who accepted it is.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    313 passed, 0 failed, 1 ignored     (312 before; +1 acceptance)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check                        advisories ok, bans ok, licenses ok, sources ok
python scripts/lint_crossrefs.py        OK: 508 references across 94 files
python scripts/lint_sql_ordering.py     OK across 48 files
python scripts/lint_research.py         OK: 24 sources, 18 claim rows
```

One test rather than eleven, deliberately: the steps share a case and the value
is in the sequence. Eleven independent tests would each rebuild the state and
would not establish that the workflow runs.

## Manual verification

**A/B 1 — the offline guarantee. First attempt was a bad A/B and is worth
recording as one.** Changing the `URL` constant left the test passing, because
`install_fixture` reads the same constant — both sides of the comparison moved
together, so the break was not a break. The real A/B stores the fixture under a
different request than the walk makes.

Result: `FixtureMissing { method: "GET", url: "https://tungsten.example/contact",
key: "b588dff8…" }`, naming what it looked for and the key it looked under. **A
cache miss is a test failure, never a live call.**

**A/B 2 — a relationship with no reachable evidence. The state could not be
created.** Deleting `evidence_link` rows for the relationship before step 6
produced not a failed assertion but a `ConstraintViolation`:

```
this is the last evidence for a row that still exists: delete the row first
```

That is a schema trigger, not a convention. Worth being precise about what it
means for the test: **step 6's assertion cannot fail through data**, only
through a bug in `evidence_for`. The guarantee behind *"a graph edge without
accessible supporting evidence should never be presented as a verified fact"* is
enforced by the database, and this test does not carry it. It checks that the
read works, which is a smaller and honest claim.

## Known limitations

1. **The fixture is synthetic, not recorded.** Nothing in this workspace can
   record one — `HttpCapability` has a single implementation and it is the
   replaying one. So step 11 proves the replay path and the offline guarantee,
   and does **not** prove that a recording path produces something replayable.
   `RecordingHttp` arrives with `LiveHttp` in increment 26, and the caveat in
   `docs/testing/reference-workflow.md` comes out then and not before.
2. **The workflow is one page, one relationship, two entities.** It proves the
   path connects; it proves nothing about scale. That is PoC P2 and R-003,
   which is the next increment.
3. **Steps 2 and 10 bypass the command layer**, so the acceptance test does not
   cover the boundary for the two ends of the workflow.
4. **No UI is involved at any point.** Every acceptance criterion in the spec
   that begins "a new user can…" remains unevaluated, and will until Milestone B.
5. **`capture_completeness` is asserted non-empty and not asserted correct.**
   Phase 1 is HTTP fetch of static HTML, so the value should be
   `static_html_only`; pinning the exact value belongs with the live transport
   that can produce anything else.
6. **The test takes 33 seconds**, most of it Argon2id running four times — two
   case creations and two export re-wraps. Correct, and worth watching if the
   acceptance suite grows.

## Next recommended increment

**Increment 20 — PoC P2, the scale probe.** `poc/p2-scale/`, outside the
workspace so proof-of-concept code cannot reach a release build. Synthesise 100k
observations, 300k edges, 100k vectors; measure p95 for the FTS5 query, the
3-hop recursive CTE, and `coverage()` — which increment 16 flagged as recomputed
on every search and which nothing has ever measured.

**R-003 is the one open risk that invalidates a core ADR.** If SQLite plus FTS5
plus SQLCipher cannot hold interactive latency at that size, ADR-0003 is wrong,
and it is far cheaper to learn that now than after a UI is built on top of it.
