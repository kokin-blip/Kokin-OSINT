# Increment 23 — the citation nobody had checked

An observation stores a `value` and a `locator_json`. Twenty-two increments
built the machinery around that pair and **nothing had ever checked that the
second supports the first.**

That is not a latent bug so much as a missing claim. The extractor writes both
in the same breath from the same match, so its own citations hold by
construction — but nothing said so, nothing enforced it, and the panel that
displays them was about to put them side by side and invite the reader to
assume a correspondence no code asserted.

This increment renders evidence, resolves every citation against the bytes it
names, and settles **A-031** — which has been `partial` since increment 16.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-search/src/coverage.rs` | `artifact_coverage`, and one shared `state_of` — see below. |
| `src-tauri/src/views.rs` | Fifteen view types for evidence, lineage and quoting. |
| `src-tauri/src/read.rs` | `artifacts`, `artifact_observations`, `observation`, and the quote resolver. |
| `src-tauri/src/lib.rs` | Three commands. |
| `ui/src/lib/bindings.ts` | The TypeScript mirror, four command wrappers. |
| `ui/src/lib/Evidence.svelte` | New. Artifact list, evidence table, coverage gaps. |
| `ui/src/lib/Observation.svelte` | New. The quote verdict and the lineage panel. |
| `ui/src/lib/DocumentPanel.svelte` | New. Five states, text only. |
| `src-tauri/tests/read.rs` | Twelve tests, and a fixture that now holds a duplicate arrival. |
| `src-tauri/tests/bindings.rs` | Two tests, and thirteen types moved out of the deferred list. |
| `src-tauri/tests/demo_case.rs` | New, `#[ignore]`d. A case on disk, for looking at. |

No new dependency, on either side.

## A-031: the rendering path, settled against the design

The original design named a sandboxed iframe for rendering captured HTML.
`tauri.conf.json` has set `frame-src 'none'` since increment 1. One had to move,
and **the design is the one that moved** (D-037).

The safety argument is the obvious one and it is not the strongest. The
strongest is evidential: **a browser's rendering of a page can differ
arbitrarily from the bytes the case hashed** — `display: none`, generated
content, script that rewrites the DOM before anyone looks. The content hash
covers the bytes. It does not cover the picture. An analyst reading a rendering
is reading something the case cannot vouch for, and the whole product is built
on being able to vouch for things.

So documents render as text. The safe choice and the honest choice turned out
to be the same choice, which is the only reason this was easy.

**Sanitising was rejected** rather than deferred. A sanitiser is a denylist
against a parser differential, a class bypassed repeatedly against exactly this
corpus — and it still renders markup, so the phishing overlay in the fixture
below survives it untouched.

The enforcing control is that the primitives are absent, not that the CSP is
strict:

- `no_part_of_the_interface_can_render_a_document_as_markup` scans every `.ts`
  and `.svelte` file under `ui/src` for `{@html}`, `innerHTML`, `outerHTML`,
  `insertAdjacentHTML`, `document.write`, `createContextualFragment`,
  `DOMParser`, `<iframe`, `<object` and `<embed`.
- `the_content_security_policy_still_forbids_every_execution_route` pins the
  four directives behind it, so relaxing one is a decision with a failing test
  attached rather than a diff nobody reads.

**The CSP is deliberately not relied on alone.** `script-src 'self'` stops
injected script and inline handlers. Nothing in a CSP stops rendered markup
positioning an overlay over the real interface and collecting what is typed into
it. A-031 moves to `mitigated`.

## A-037: a citation that does not support its claim

New attack row, and the one this increment exists for.

The failure needs no attacker — a wrong locator arrives from an extractor bug,
an imported package, or a later transform whose offsets mean something else —
and it is invisible, because the panel renders identically whether the citation
holds or not. An attacker who can get one row into a case gets the same effect
on purpose: **a claim the interface presents as quoted from a document the
document does not make.**

`read::quote` resolves the locator against the artifact's own bytes and returns
the verdict as the payload's tag (D-038): `exact`, `contains`, `differs`,
`out_of_range`, `too_large`, `unknown_scheme`, `unavailable`.

Three decisions inside that are worth naming:

- **`differs` is rendered, not hidden.** A citation that does not hold is shown
  as unsupported, in the one tone in that panel that survives being read in
  monochrome.
- **`too_large` declines rather than truncating.** A shortened quote can be
  confirmed and never contradicted, because the value might be in the part that
  was dropped. A verdict sound in one direction only is worse than no verdict.
- **An unknown scheme keeps its raw JSON and is not interpreted.** A
  page-relative offset read as a document-relative one would quote confidently
  from the wrong place — so `{"kind":"pdf_page_box","page":3,"start":0,"end":6}`
  resolves to nothing at all, despite having a `start` and an `end` this build
  could have used.

The bytes stream through the AEAD and the content hash on the way to the
window, so a quote is never shown from a document that failed authentication.
That check is load-bearing rather than incidental: **the window is already full
when the AEAD rejects**, which the A/B below demonstrates.

## Lineage is a tree, because deduplication makes it one

The same document reaching a case from two places is an investigative fact. The
store keeps one blob and every arrival, so `also_collected` names the others —
a panel showing only the artifact in front of the analyst would state a single
origin for evidence the case knows arrived twice.

Lineage is walked along the `derivation` edges rather than the convenient
foreign keys. `observation.artifact_id` and `capture.source_id` are
denormalisations of the same facts; the edges are what the provenance model
promises, and reading the shortcut would leave the promise untested by anything.

## One rule, one definition

`incomplete_artifacts` mapped a `succeeded` run to `Partial`. That is correct
**only** because its `WHERE` clause has already filtered to rows that are
incomplete — reasoning invisible at the point the mapping is written, and
exactly how it gets copied somewhere the filter does not apply and starts
reporting fully-read documents as partly read.

Rather than copy it, `state_of(status, error_code, has_gap)` is now the one
place a run status becomes a coverage state, and `has_gap` is a parameter rather
than an assumption. This is the third increment in a row to find the same shape
— a claim with no single definition drifting while every copy keeps looking
right.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    332 passed, 0 failed, 3 ignored     (318 before; +14, and demo_case ignored)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check                        advisories ok, bans ok, licenses ok, sources ok
npm run check (svelte-check)            111 files, 0 errors, 0 warnings
python scripts/lint_crossrefs.py        OK: 600 refs across 101 files
python scripts/lint_sql_ordering.py     OK
python scripts/lint_research.py         OK
```

`lint_crossrefs.py` earned its keep again: it failed on five dangling `A-037`
and `D-037` citations written into Rust before the rows existed.

### A/B on each new control

Each break was applied alone, the suite run, and the file restored.

| Break | Result |
| --- | --- |
| `quote` returns the observation's own `value` as the quote — the flattering implementation that renders it twice and agrees with itself | `a_citation_that_does_not_support_its_claim_says_so` **and** `the_quote_carries_the_document_around_it_and_not_just_the_value` failed. Nothing else. |
| A failed AEAD no longer withholds the quote | `a_quote_is_not_shown_from_a_document_that_failed_authentication` failed — **and it failed by showing a quote**, because the window is already full when authentication rejects. |
| `also_collected` returns an empty list | `a_document_that_arrived_twice_names_both_arrivals` failed. |
| `{@html content.text}` in `DocumentPanel.svelte` | `no_part_of_the_interface_can_render_a_document_as_markup` failed, naming the file and the primitive. |
| `frame-src 'self'` in `tauri.conf.json` | `the_content_security_policy_still_forbids_every_execution_route` failed. |

### Two things the tests found on their own

**`observation` is append-only, enforced by a trigger** — *"rerun the extractor,
do not edit what it saw"*. The first version of the forged-citation test tried
to `UPDATE` a locator and was refused. The test now inserts a row instead, which
is both the only way and the more honest one: the check exists for rows
something *else* wrote, so the test has to be that something else.

**The fixture page yields two observations, not three**, and the `transform_name`
is `extract.html`, not `html_extract`. Both were assumptions in a first draft
that the suite rejected immediately.

## Manual verification — the application, driven

Built `--release` and driven with synthetic mouse and keyboard against a real
encrypted case. Eight screenshots, each read.

`demo_case.rs` builds that case: `#[ignore]`d, in the workspace, for the reason
D-036 gave for the scale probe. Nothing in the interface can collect anything
until increment 26, so without it there is nothing to look at.

**One of its three documents is written the way the pages this product collects
are written** — inline script exfiltrating `document.cookie`, an `onerror`
beacon, and a fixed-position full-screen form asking for the case passphrase.
That is the only way to show A-031 mitigated rather than asserted.

In order:

1. **The artifact list renders** — `never read` / 0 observations for the page
   nothing has opened, `read` / 4 for the extracted one. The word, not a bar.
2. **The evidence table** lists `email.address`, `link.href`, `page.text` and
   `page.title`. Note `page.text` contains *"Session expired Case passphrase
   Unlock"* — the extractor correctly reads the attack's own overlay text as
   page content.
3. **The quote verdict reads `FOUND IN THE DOCUMENT`**, and the excerpt shows
   the real page source with `<a href="mailto:press@tungsten.example">`
   highlighted, grey context either side — including the `<script>` tag,
   **displayed as text**.
4. **The lineage panel** names all four steps: collected (path, time, connector,
   job, completeness, normalised locator, ingest run and build), stored (media
   type and content hash), read (`extract.html 1`, build `kokin-extract 0.0.1`,
   succeeded), recorded (`html_byte_range · a[href] · bytes 117–157`, and the
   raw locator JSON beneath it).
5. **The whole hostile page renders as source** in a `<pre>`. No script ran, no
   beacon fired, no overlay appeared. The interface is intact behind it.
6. **A shredded document's observation** reads *"THE BYTES ARE NOT AVAILABLE TO
   CHECK — The document was deliberately destroyed. What it said cannot be
   confirmed again by anyone"*, in the neutral palette.
7. **Its document panel** reads `DESTROYED — The bytes were shredded`, also
   neutral, with the size, type and hash still above it. Increment 16's finding
   rendered rather than argued.
8. Its lineage survives in full. The observations outlived their source, which
   is the entire reason crypto-shredding is not deletion.

## The increment numbers moved

The plan's milestone B assumed the UI foundation landed at 21. It landed at 22,
because 21 went on the R-014 fix. Corrected in `NOT_YET_BOUND` and in
`App.svelte` rather than left to drift — the same mistake increment 19 found in
the reference workflow, caught earlier this time.

Milestone B is now: **24** entities and confidence, **25** search and history,
**26** write flows.

## Known limitations

1. **Still no component tests.** Increment 22's largest gap is increment 23's
   largest gap. Every UI claim above rests on screenshots I read. A change that
   made `DocumentPanel` render the wrong state would pass everything in CI —
   though not, now, a change that made it render markup.
2. **`artifacts()` runs one pass per row.** Bounded by `MAX_ARTIFACTS` (200) and
   deliberate: deriving the extraction state inline would duplicate
   `kokin_search`'s latest-run window, which is the thing this increment
   otherwise spent effort deduplicating. It will need a batch query before a
   case holds thousands of documents.
3. **The demo case is a fixture, not a test.** It asserts almost nothing and
   says so.
4. **A quote resolves only `html_byte_range`.** Every other scheme is honest
   about being unresolvable, which is correct and is not the same as useful.
   EXIF and PDF locators arrive in increment 31.
5. **`differs` has never been seen in the wild**, only forged. No extractor in
   this product can currently produce one, which is what the test asserts over
   all of them — but it means the presentation of that state has been reviewed
   by nobody except me.
6. **A-034 is untouched.** The path still crosses IPC as a string; the native
   file dialog is increment 26.

## Next recommended increment

**Increment 24 — entities and confidence.** All seven dimensions rendered
always, with `unassessed` visibly distinct from `insufficient_information`
(D-031), and disagreement across a merged cluster shown attributed to its rows
rather than reconciled.

**No composite score anywhere.** A-033 is a view-layer attack and that is the
view layer: a total is what sorts a table and colours a badge, and once one
exists in a rendering it is indistinguishable from a measurement.

The evidence link from an entity back to the observation panel built here is
what makes that increment worth doing in this order.
