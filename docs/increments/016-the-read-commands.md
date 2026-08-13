# Increment 16 — the read commands

Increment 15 gave the application a case lifecycle. It could open a case and
close it; it could not show anything inside one. `Session::with_case` existed
and had a single test proving it returned `NoCaseOpen`.

This increment gives it three readers — `search`, `artifact_document`, `entity`
— plus `coverage` for a view that has not searched yet. All four are one-line
commands over `read.rs`, which knows nothing about Tauri.

The interesting part of this increment is not the plumbing. It is that this is
the layer where three specific lies become easy to tell, and all three are lies
of *omission* that no crate underneath can prevent.

## Files changed

| File | What |
| --- | --- |
| `src-tauri/src/views.rs` | New. Every type that crosses the boundary, in either direction. |
| `src-tauri/src/read.rs` | New. `search`, `coverage`, `artifact_document`, `entity`. |
| `src-tauri/src/session.rs` | View definitions moved out; it is now the lock and nothing else. |
| `src-tauri/src/error.rs` | `NotFound`, `SearchIndexCorrupt`; `From` for search, graph and blob errors. |
| `src-tauri/src/lib.rs` | Four more commands, each one line. |
| `src-tauri/tests/read.rs` | New. 15 tests against a real case on disk. |
| `scripts/lint_sql_ordering.py` | Scans `src-tauri/` as well as `crates/`. |
| `docs/threat-model/attack-catalog.csv` | A-031, A-032, A-033. |
| `docs/decision-log.csv` | D-029, D-030, D-031. |

No new external dependency. `src-tauri` now depends on `kokin-search`,
`kokin-graph` and `kokin-blob`, all already in the workspace.

## A result list cannot be rendered without its caveat

Coverage was measured in increment 9 and has been correct ever since. It was
also a separate function, which means the interface could render forty results
from a case that is 60% read and never mention the other 40% — and it would,
because that is what happens to a caveat somebody has to remember to fetch.

`SearchView` carries `hits`, `total` and `coverage`, and `coverage` is not an
`Option` (D-029). There is no command that returns hits without it. Computing it
per search costs a scan of lineage, which is small next to the documents it
describes and touches no full-text index.

The statement it makes is deliberately weaker than the one an analyst wants.
"Your search found three hits and two unread documents might have held a fourth"
cannot be produced by anything, ever: deciding whether an unread document matches
a query requires reading it. So the honest claim is case-scoped — *this case
holds documents no search of it can reach* — and it has to travel with every
result list rather than only the empty ones. A search returning nothing is
self-evidently unsatisfying; a full page is what stops the questioning.

## Five answers about a document, not three

`blob_access` (increment 14) returns three: readable, shredded, unrecorded. It
stops there on purpose — the remaining distinctions need I/O, and increment 14
had no caller that did any.

This is the caller that does. Once the read is attempted, "the key is in the
database" splits into three outcomes that mean entirely different things:

- **Readable** — the bytes are here and every one of them authenticated.
- **Lost** — the key is here and the ciphertext is not. Nobody decided this.
- **Damaged** — the ciphertext is here and failed the AEAD or the content hash.
  Something modified the case directory. This is the only state in the enum that
  is an accusation.

Collapsing `Lost` into `Shredded` is the tempting simplification, and it is the
worst available outcome (A-032). It tells the analyst that somebody destroyed
this document on purpose, when what actually happened is that the case lost it
and nobody noticed. The case then carries a confident, wrong account of its own
evidence, and — because deliberate destruction is a normal, expected state —
nobody goes looking.

None of the five is an error. A shredded document rendered through
`CommandError` is a document rendered in red, which is how a retention policy
working correctly comes to look like a fault. Only a *missing artifact row* is an
error here, because that is a stale id the interface can act on.

### The excerpt is verified over the whole document

A document crosses IPC as a JSON string and a case may legitimately hold a
two-gigabyte artifact, so the payload is capped at 2 MiB with the remainder
reported as `truncated_bytes`. The cap is on what is *returned*, not on what is
*checked*: `read_to` streams the whole blob through the AEAD and the content
hasher, and the sink keeps the head and counts the tail.

`an_excerpt_is_verified_against_the_whole_document` damages a byte three
megabytes in — a byte the excerpt never shows — and requires `Damaged`. An
implementation that stopped reading at the limit passes every other test in the
file and fails this one.

## Seven dimensions, whether or not anybody looked

`kokin_graph::assessments_for` returns the assessments that exist. Rendering
that list directly is the obvious implementation, and it produces a panel where
an unassessed dimension is *absent* — which reads as a dimension with nothing to
worry about. That is the opposite of what it means.

`EntityView.confidence` always has all seven, with `agreement` as `unassessed`,
`assessed` or `disagreed` (D-031). ADR-0007 makes `insufficient_information`
first-class precisely so "we have not established this" is a statement; a blank
is not that statement, and neither is a fabricated row with no actor and no
timestamp.

The dimensions come from the case's own `scale` table, not from
`kokin_graph::scales()`. A case is self-describing on purpose: reading the
build's copy would quietly make the binary the authority on what a case is
allowed to say, and an assessment made under version 1 of a scale would render
under version 2's labels.

**Disagreement is reported, not resolved.** A merged cluster can hold two rows
assessed differently — that is the normal case, since the assessments were made
before anyone decided the rows were one person. Both values are returned,
attributed to their rows. Picking a winner is a composite score with a smaller
denominator.

## The composite score has exactly one place it can be invented

Every crate below this one honours ADR-0007. This layer is where a total gets
added anyway, because a total is what sorts a table and colours a badge — and a
number in a payload is indistinguishable from a measurement by the time anyone
sees it (A-033).

`DimensionView` carries value *keys* on named ordinal scales. There is nothing
in the payload to average. `the_confidence_block_carries_nothing_that_could_be_added_up`
serialises the block, walks every value, and rejects any number whose path is not
`scale_version` — so the guarantee is structural rather than a convention that
survives until somebody needs to sort.

The same reasoning removed `relevance` from `HitView` (D-030). BM25 is negative,
lower-is-better, and corpus-dependent; the only honest thing it can do is order
the list, which it already did server-side. Passing it through invites a
percentage next to a search result, which reads as confidence — competing
directly with the seven dimensions that actually mean something.

## One file lists what crosses the boundary

Increment 15's guarantee was that the domain crates take no `serde` dependency,
so `OpenCase` *cannot* be serialised. That still holds, and it is still the real
control. But it only stays legible if the types that do serialise are somewhere a
person can read in one sitting, and this increment was about to add eleven more
of them.

So `views.rs` is now the whole boundary — inputs included — and
`serialize_is_derived_only_where_the_boundary_is_declared` reads this crate's
own source and fails if a `Serialize` or `Deserialize` derive appears anywhere
outside it and `error.rs`. `session.rs` is now the lock and nothing else.

## The ordering lint was looking at the wrong directory

`scripts/lint_sql_ordering.py` (D-022) globs `crates/**/*.rs`. That was correct
for as long as `src-tauri` had no SQL in it, and `read.rs` is the first file
outside `crates/` to write an `ORDER BY` — so the guard against the bug this
codebase has now written three times was, silently, not covering the newest
place it could be written.

Extended to `src-tauri/**/*.rs`: 31 files checked before, 40 now. Verified by
changing `ORDER BY namespace, value, rowid` in `read.rs` to
`ORDER BY created_utc, id` and confirming the lint reports it with a file and
line, then restoring it. Worth stating plainly because the failure mode is
generic: a guard scoped to where a problem *has* occurred stops covering where it
*will*, and it keeps reporting OK the whole time.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    268 passed, 0 failed, 1 ignored     (253 before; +15 read)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
python scripts/lint_sql_ordering.py    OK across 40 files  (was 31; see below)
python scripts/lint_crossrefs.py       OK: 284 references across 39 files
python scripts/lint_research.py        OK: 24 sources, 18 claim rows
```

## Manual verification

Five A/B controls, each run by breaking the behaviour and confirming exactly the
tests that name it fail.

**A/B 1 — coverage figures.** Replaced the computed coverage with zeros, which
serialises as a complete case. Result: only
`a_result_list_arrives_with_the_coverage_it_needs` failed. The test checks the
figures, not the presence of the field.

**A/B 2 — the blob store's verdict.** Discarded the result of `read_to` and
treated every read as success. Result: exactly three failed —
`a_modified_document_is_reported_as_damage`,
`a_document_whose_bytes_have_vanished_is_lost_not_shredded` and
`an_excerpt_is_verified_against_the_whole_document` — which are precisely the
three states that only exist because the verdict is read. The other twelve
passed, including the byte-for-byte round trip.

**A/B 3 — lost versus shredded.** Mapped `BlobError::NotFound` to `Shredded`,
which is the collapse A-032 is about. Result: only
`a_document_whose_bytes_have_vanished_is_lost_not_shredded` failed. Worth
noting how quiet that break is: the document still reports a state, the UI still
renders a sensible message, and nothing anywhere says the case lost a file.

**A/B 4 — the sparse dimension list.** Returned only the dimensions with an
assessment. Result: only
`every_dimension_is_present_whether_or_not_anyone_assessed_it` failed. The
disagreement test and the no-composite test both still passed — which is the
point. A sparse list looks entirely correct from every angle except the one that
asks what happened to the dimensions nobody mentioned.

**A/B 5 — the boundary file.** Added `#[derive(serde::Serialize)]` to a private
struct in `read.rs`. Result: only
`serialize_is_derived_only_where_the_boundary_is_declared` failed, naming the
file and the line.

## Known limitations

1. **Nothing writes yet.** Ingest, extract, entity creation, merges and
   assessments are all reachable from the test suite and from no command. The
   application can read a case it cannot fill. That is increment 17.
2. **The UI is still `App.svelte` and nothing else.** Ten commands now exist and
   none has a caller. Every claim in this document is about the shape of a
   payload, and no human has looked at one.
3. **A document is returned as text and the webview could still render it as
   markup** (A-031). The bytes are the evidence, and the evidence is frequently
   a hostile HTML page. The command hands over a JSON string documented as text;
   what actually enforces this is a CSP and a rendering path that cannot execute,
   and neither exists yet. Recorded as `partial`, deliberately.
4. **Coverage is recomputed on every search.** Two aggregate queries over
   lineage, which is nothing next to the FTS5 query beside it — but it is
   per-keystroke work if typeahead search is ever wired to this, and that is the
   moment to measure rather than now.
5. **`entity` issues a query per cluster member for assessments.** Fine at the
   cluster sizes this product will see (a merged person is two or three rows, not
   two hundred), and it will need one query with an `IN` clause if that ever
   stops being true.
6. **A relationship has no read.** `search` returns relationship hits that no
   command can open. Entities and documents were the two that unblock a UI;
   relationships need the graph view to have somewhere to put them.
7. **`DocumentContent::Damaged` folds two things together.** An I/O failure and a
   failed AEAD both arrive as `Damaged`, because inventing a sixth state meaning
   "we could not tell" is worse than reporting the stronger claim. A disk that is
   failing will therefore be reported as a case that was modified, which is the
   error in the safe direction but is still an error.

## Next recommended increment

**Increment 17 — the write commands.** `ingest`, `extract`, and the L2 writes:
creating an entity, adding an identifier, relating two of them, assessing a
dimension, and the merge/split pair.

This is where `CommandError`'s refusals stop being theoretical. `MachineMayNotMerge`
and `MachineMayNotAssign` are the product working, not failing, and they need
codes that say so rather than arriving as `Storage` — which is exactly why
`From<GraphError>` was written variant by variant here with most of its arms
still pointing at `Storage`. It is also where every write has to carry the
evidence that grounds it, because the schema will not accept one that does not,
and the command signatures are where that requirement either stays visible or
gets a convenient default.
