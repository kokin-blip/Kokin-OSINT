# Increment 10 — the words on the page

Increment 9's largest stated limitation was that search covered what a case had
**recorded about** its documents — titles, links, addresses, entity names — and
never what the documents actually say. This closes it.

No schema change was needed. Page prose is emitted as ordinary `page.text`
observations with byte-range locators, so migration 5's existing trigger indexes
it and `kokin-search` did not change at all. That is the payoff of increment 9's
"a document carries only the text its own row owns": a new *kind* of row becomes
searchable without touching the search layer.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-extract/src/html.rs` | `KIND_TEXT`, two new limits, prose accumulation and chunking, whitespace collapsing, truncation accounting. |
| `crates/kokin-extract/src/lib.rs` | `text_truncated_bytes` through `Extracted` and the audit payload; `params_hash` covers the new limits; the compile-time guard test. |
| `crates/kokin-extract/tests/hostile_corpus.rs` | 7 new tests. |
| `crates/kokin-search/tests/search.rs` | 2 new tests; the fixture page gained prose and a script. |
| `docs/threat-model/attack-catalog.csv` | A-022. |
| `docs/decision-log.csv` | D-017. |

## What the parser had to be told

Four things were settled by probing `lol_html` rather than by reasoning, and
three of them contradicted the obvious guess:

- **`text!("body")` finds nothing in a document with no `<body>` tag**, and
  saved fragments routinely have none. The handler selects on `*`.
- **`*` does not double-count.** Text inside `<div><span>` fires once, not once
  per matching ancestor.
- **`TextType` separates prose cleanly.** Script bodies arrive as `ScriptData`,
  stylesheets as `RawText`, `<title>` as `RCData`. Filtering to `Data` excludes
  all three, so no selector gymnastics are needed to keep JavaScript out of the
  index.
- **Text arrives per node, split by inline markup.** `Acme <b>Holdings</b> Ltd`
  is three nodes.

That last one forced a choice. Concatenating nodes with no separator turns
`<p>A</p><p>B</p>` into `AB` — a word the page does not contain. Always
inserting a space costs the opposite error: `<b>Hold</b><i>ings</i>` becomes
`Hold ings`. Separate blocks are common and mid-word formatting is rare, so the
space wins, and both cases have a test.

## The bug this increment caused, and what it cost

Adding prose extraction broke `a_500_mb_document_is_read_in_bounded_memory` —
a document that had always been readable started failing with
`TooManyObservations`. At 1 KiB chunks, half a gigabyte of paragraphs is about
512,000 observations against a 10,000 cap.

This is not a test that needed updating. The module's own documentation says
*"evidence that is refused for being large is evidence an investigation does not
get"*, and the change had quietly made large documents refusable on the strength
of their wordiness alone.

The fix is a separate prose budget (D-017), and the interesting part is why it
**truncates** where the observation cap **refuses**. The two failures are not
alike:

- Too many discrete facts → refuse. A truncated fact list is indistinguishable
  from a complete one, and an analyst has no way to know the page held more.
- Too much prose → truncate, count, report. Prose is a search aid, not a fact
  list, and refusing the document over it would also discard every link, title
  and address in it.

Truncation is never silent. `text_truncated_bytes` reaches the caller and the
`evidence.extracted` audit payload, so a case can be asked which of its
documents are only partly searchable. **It does not yet reach the analyst at
search time, which is where they actually are** — filed as A-022, `partial`.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    199 passed, 0 failed, 1 ignored

    48 net · 33 store · 24 extract-hostile · 16 graph-l2 · 16 search-integration
    · 14 blob · 12 ingest · 11 extract · 11 search-query · 9 keys · 5 graph-scales

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check              advisories ok, bans ok, licenses ok, sources ok
scripts/lint_crossrefs.py     OK: 220 internal references across 33 files
scripts/lint_research.py      OK: 24 sources; 18 claim rows
```

New tests worth naming:

- `script_and_style_bodies_are_not_indexed_as_prose` — otherwise every page
  carrying a tracker matches `function`, and a case answers searches with its
  own machinery.
- `a_document_that_is_all_prose_is_read_not_refused` — 5.4 MB of pure text.
- `prose_past_the_budget_is_reported_rather_than_dropped_in_silence`.
- `a_document_with_no_body_tag_still_yields_its_prose`.
- `the_words_on_the_page_are_findable` — the end-to-end payoff, including that a
  prose hit routes back to a real byte range.

## A compile-time guard, not a runtime one

`params_hash` enumerates the limit fields by hand. A new limit added to `Limits`
and forgotten there would make two configurations that produce *different*
observations share a hash — so a rerun under new limits would look like a repeat
of the old run, supersede nothing, and leave both readings live in the case.

`every_limit_changes_the_params_hash` destructures `Limits` exhaustively, so
adding a field **stops this file compiling** until someone looks at it. The
assertions then check each field actually reaches the hash. A/B: removing
`max_text_bytes` from the hash fails the test with
`max_text_bytes does not reach params_hash, so a rerun that changes it would
supersede nothing`.

## Known limitations

1. **A phrase straddling a chunk boundary is not findable as a phrase.**
   4 KiB chunks make it rare; overlapping windows would fix it at roughly double
   the index size.
2. **Prose past 4 MiB per document is not searchable** (A-022). Counted and
   audited, but invisible at the point of search.
3. **Mid-word inline markup splits a word** — `<b>Hold</b><i>ings</i>` indexes
   as `Hold ings`. The deliberate other side of not running blocks together.
4. **Boilerplate is indexed like anything else.** Site navigation and footers
   repeat across every page of a crawl, inflating the index and giving common
   nav words high document frequency. BM25 discounts them, but they are still
   stored.
5. **Still HTML only.** PDFs, images and office documents contribute no text;
   they are the next formats worth a transform.
6. **Existing cases need a re-extraction** to gain prose. The limits changed, so
   `params_hash` changed, so a rerun supersedes correctly rather than
   duplicating — but nothing triggers that rerun automatically yet.
7. **`text_truncated_bytes` counts decoded, whitespace-collapsed bytes**, not
   source bytes, so it understates how much of the original file was skipped.

## Next recommended increment

**Increment 11 — surfacing incompleteness at the point of search.** Limitations
2 and 6 are the same problem seen twice: the case knows it is only partly
searchable and the analyst does not. A result list that says "3 documents in
this case hold text that was not indexed" and "4 artifacts have never been
extracted" converts a silent gap into a visible one, which is the difference
this product keeps claiming to make.

It is a better next step than the `er_*` tables (A-007, ADR-0008) because it
closes a gap this increment opened, and because entity resolution still needs
enough entities in a case for merging to be a real question.
