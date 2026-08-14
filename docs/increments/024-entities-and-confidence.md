# Increment 24 — seven judgements, and no eighth

ADR-0007 has seven confidence dimensions and no total. Every crate below this
one honours that. **The view layer is where a total gets invented anyway**,
because a total is what sorts a table and colours a badge — and once one exists
in a rendering it is indistinguishable from a measurement (A-033).

This increment renders the confidence block, and pays off the debt both previous
increments closed by naming: **component tests**.

## Files changed

| File | What |
| --- | --- |
| `src-tauri/src/views.rs` | `EntityRowView`. |
| `src-tauri/src/read.rs` | `entities`, canonical rows only, counted across the cluster. |
| `src-tauri/src/lib.rs` | One command. |
| `ui/src/lib/Entity.svelte` | New. The confidence block, identifiers, evidence, decisions. |
| `ui/src/lib/Entities.svelte` | New. The list, and the link down to the bytes. |
| `ui/src/lib/bindings.ts` | Seven entity view types, two commands. |
| `ui/vitest.config.ts` | New. |
| `ui/src/lib/RecoveryKey.test.ts` | New. 8 tests. |
| `ui/src/lib/DocumentPanel.test.ts` | New. 6 tests. |
| `ui/src/lib/Entity.test.ts` | New. 12 tests. |
| `.github/workflows/ci.yml` | `npm test`, after the type check. |
| `src-tauri/tests/read.rs` | One test: the list shows people, not rows. |
| `src-tauri/tests/demo_case.rs` | Two entities, judged differently, then merged. |

Two new dev dependencies: **vitest** and **jsdom** (D-039). Neither ships.

## The debt, paid

Increments 22 and 23 both closed with the same sentence in the same words:
*every UI claim rested on screenshots I read, which verifies a build and guards
nothing.* Increment 22 named the specific failure: **a change that made the
recovery-key gate skippable would pass everything in CI.**

That is now false. `RecoveryKey.test.ts` mounts the real component and asserts
the gate holds on one condition, that Escape does nothing, that no button reads
*close*, *skip*, *later* or *dismiss*, that the challenge is group 7 of 8, and
that a *different group of the same key* is refused — which a naive
"is this substring in the key" check would have passed.

**Why component tests and not an end-to-end driver** is recorded in D-039. The
short version: Playwright drives the real webview and is the wrong tool for a
rendering rule, because it cannot *construct* the fixtures these tests need. "A
merged cluster whose rows disagree on one dimension" is several write commands
away in a real case and one object literal away here.

**The honest limit**: jsdom is a second HTML implementation, so these verify
against jsdom's parser rather than WebView2 or WKWebView. The markup
prohibition (A-031) therefore stays enforced by the source scan from increment
23 and is **not** delegated to these tests.

## What the confidence block must not do

**All seven, always** (D-031). A sparse list renders as absence, and absence
reads as *no concern* — the opposite of what an unassessed dimension means.

**`unassessed` is not `insufficient_information`.** Nobody looked, versus
somebody looked and could not tell. The second is a judgement with an author, a
timestamp and recorded factors; ADR-0007 made it first-class precisely so that
"we do not know" is a statement rather than a blank. The panel gives it a
*Somebody looked* badge and its own styling.

**A disagreement is attributed, not reconciled.** Where merged rows were judged
differently, both readings appear with the row each hangs on. Picking a winner
is a composite score wearing a different hat.

**Nothing counts up.** The payload gives the component nothing to add — values
are keys on named ordinal scales and the only number is the scale version. The
test asserts the *rendered text* carries no percentage, no `n/7`, no "overall",
no "score".

The one number in the list view, `assessed_dimensions`, is worded as
`2 dimensions judged` rather than as a fraction or a bar, because it counts
whether anyone *looked*, never what they concluded. **"7 of 7" would describe a
thoroughly examined entity that may be thoroughly doubtful.**

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    333 passed, 0 failed, 3 ignored     (332 before; +1)

npm test (vitest)                       26 passed, 3 files
cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check                        advisories ok, bans ok, licenses ok, sources ok
npm run check (svelte-check)            169 files, 0 errors, 0 warnings
python scripts/lint_crossrefs.py        OK: 621 refs across 103 files
python scripts/lint_sql_ordering.py     OK
python scripts/lint_research.py         OK
```

### A/B on each new control

| Break | Result |
| --- | --- |
| `ready = saved \|\| matches` — either condition opens the recovery gate | 3 failed: both "alone past" tests and *rejects a different group*. |
| An `I'll do this later` button added to the recovery gate | 1 failed: *offers no way out other than acknowledging*, and it is a **text** match, so renaming the button does not evade it. |
| `entity.confidence.filter((d) => d.values.length > 0)` — render only assessed dimensions | 4 failed, one of which I had not predicted (see below). |
| `Overall: {n} of 7 assessed` added above the block | 1 failed: *invents no total, no score and no percentage*. |

**The unpredicted failure is the useful one.** Filtering unassessed dimensions
also broke *survives a factors field that is not a JSON array of strings* —
because that test asserts seven dimensions still render after a malformed
factors field, which turns out to be a second, independent way of stating the
same rule. A test I wrote for robustness was also guarding the increment's
central claim, and I did not know that until the break.

### Two controls the tests found for me

**A rule may not assign `insufficient_information`.** The demo case tried, and
`kokin_graph` refused with `MachineMayNotAssign`. That is exactly right and I
had forgotten it: *"we could not establish this"* is a conclusion a person
reaches, not one a matcher reports. The scale's `machine_assignable` flag is
per value, not per scale.

**Evidence rows read as duplicates after a merge.** Driving the real
application showed two identical `SUPPORTS observation a7c31bf9…` lines — correct
(one link per merged row) and unreadable. `EvidenceView.entity_id` existed and I
was not rendering it. Now each says which record cites it, the same way the
confidence values do. **A screenshot found this and no test would have**, which
is the argument for continuing to do both.

## Manual verification — the application, driven

Six screenshots. The demo case now holds two entities grounded in the hostile
page, judged differently, then merged.

1. **The entity list shows one row, not two** — the merge collapsed them —
   tagged `1 RECORD MERGED IN`, with `2 dimensions judged`.
2. **All seven dimensions render**, five of them reading *"Nobody has judged this
   dimension. That is not the same as no concern."*
3. **Identifier match** carries `rule:shared_identifier` with an `AUTOMATED`
   badge beside the human assessments.
4. **Source reliability shows `DISAGREEMENT`** with both readings — *Usually
   reliable* and *Mixed* — each attributed to its own entity id and carrying its
   own recorded factor.
5. **Identifiers, evidence and the merge rationale** all render, the last
   reading *"the address on the contact page is the press office's own"*.
6. **The chain closes.** Clicking an entity's evidence link opens the
   observation, which reads `QUOTED EXACTLY` with `Tungsten Holdings - contact`
   highlighted at bytes 20–47 of the real page source, above its full lineage.

That last one is the three-layer model navigable end to end for the first time:
a claim about a person, opened down to the bytes of the document that says so,
with the citation resolved against those bytes rather than asserted.

## Known limitations

1. **`Entities.svelte` and `Evidence.svelte` do not know about each other.**
   Each opens its own copy of an observation panel. Harmless now, wrong once
   there is navigation, and the right fix is a router rather than shared state
   between two siblings.
2. **No component test covers `Entities.svelte` or `Evidence.svelte`.** They are
   data plumbing over `invoke`, and testing them means mocking the bindings —
   which asserts that the mock agrees with the mock. The rules worth guarding
   live in the presentational components, and those are covered.
3. **jsdom is not the shipping webview.** See above.
4. **Nothing can be written from the interface.** Every judgement on screen was
   put there by a test fixture. Accept/reject of suggestions, merge and split
   are increment 26 — the panel renders decisions and cannot make one.
5. **`entities()` runs several queries per row**, the same shape as
   `artifacts()`. Bounded by `MAX_ENTITIES` (500) and for the same reason: the
   cluster read belongs to `kokin_graph`.
6. **`assessed_dimensions` is one step from being a score.** It is worded
   carefully and nothing sorts by it. If a future increment adds sorting, that
   is the column that will be reached for first, and it should not be.

## Next recommended increment

**Increment 25 — search, coverage and activity history.** `coverage` travels
with every result list and is not dismissible (D-029): a full page of results
from a 60%-read case is the dangerous case, not the empty one.

Activity history over `audit_event` has to carry ADR-0014's disclaimer **in the
interface and not only in the docs**: tamper-evident, not chain of custody. That
is the same class of statement as `Shredded`, and it fails the same way — by
being quietly upgraded in a reader's mind into something stronger than it is.
