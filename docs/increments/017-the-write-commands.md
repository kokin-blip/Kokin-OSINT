# Increment 17 — the write commands

Increment 16 left the application able to read a case it could not fill. Ingest,
extract, entity creation, merges and assessments were all reachable from the test
suite and from no command.

This increment adds nine: `ingest_file`, `extract`, `create_entity`,
`add_identifier`, `relate`, `assess`, `merge_entities`, `reject_merge` and
`split_entity`. Like the reads, each is one line over a module that knows nothing
about Tauri.

The interesting part is that this is the first layer where the caller is
*untrusted*. Every read command could be wrong; a write command can be a lie that
the case then records faithfully and forever.

## Files changed

| File | What |
| --- | --- |
| `src-tauri/src/write.rs` | New. The nine writes, `ACTOR`, and the shared translation of evidence, subjects, factors and rationale. |
| `src-tauri/src/views.rs` | Eleven request and view types, every request `deny_unknown_fields`. |
| `src-tauri/src/error.rs` | Nine codes; `From` for `IngestError` and `ExtractError`; the `GraphError` refusals mapped properly at last. |
| `src-tauri/src/lib.rs` | Nine more commands, each one line. |
| `src-tauri/Cargo.toml` | `kokin-ingest` and `kokin-extract` move from dev-dependencies to dependencies. No `kokin-net`. |
| `src-tauri/tests/write.rs` | New. 21 tests against a real case on disk. |
| `scripts/lint_crossrefs.py` | Scans Rust sources and `scripts/`, not only Markdown. |
| `docs/decision-log.csv` | D-032, D-033. |
| `docs/threat-model/attack-catalog.csv` | A-034. |

No new external dependency.

## The actor is not an input

`kokin_graph` refuses `rule:` and `ai:` actors the decisions only a person may
make (ADR-0008), and migration 7 refuses them again in a `CHECK` constraint. The
module note in `kokin_graph::resolution` already said where that ends: neither
can catch automation writing `actor = "user:local"`.

This is the layer where that stops being hypothetical, because a request field is
written by the webview — the least trusted thing in the process, and the one that
renders hostile HTML (A-031). Had `actor` been a parameter, every guarantee above
would have reduced to a naming convention the caller chooses to honour, and the
audit chain would record a person where a machine acted: **intact and false**,
which is worse than absent.

So `ACTOR` is a constant, no request type carries an `actor` field, and every
write request is `deny_unknown_fields` (D-032). The second half is not decoration:
without it a caller sends `actor` anyway, serde ignores it, and the request that
tried to lie is indistinguishable from an honest one.

The corollary is that **`MachineMayNotAct` is unreachable from this crate**. It is
mapped anyway — the mapping has to be right for the rules engine and the model
that will call `kokin_graph` in-process under their own names, and routing a
refusal through `Storage` is how a deliberate boundary comes to look like a
database fault.

The honest cost, recorded rather than glossed: `user:local` is not a person. Two
analysts sharing a machine are indistinguishable in the audit chain. That is
coarse; accepting the name from the webview would make every actor unverifiable
instead of merely coarse.

## The refusals are the product working

Increment 16 mapped nine `GraphError` variants to `Storage` because no command
could reach them, and said the write commands would need them mapped properly.
They do. `MachineMayNotAssign`, `MachineMayNotMerge`, `SelfMerge`,
`AlreadyMerged` and `NotAbsorbed` are not failures — the first two are ADR-0008
holding, and the last three are the case having moved on since the interface last
looked.

`AlreadyMerged` and `NotMerged` are separate codes for a reason that only shows up
in the interface: the first has an action attached — open the entity they resolve
to — and the second has none. Folding them together throws away the one thing the
UI could do about it.

Similarly `ExtractionFailed` versus `Storage`. Every variant under
`ExtractionFailed` leaves the artifact, its bytes and its provenance exactly as
they were; one derivation did not happen. `a_document_this_extractor_cannot_read_is_not_a_damaged_case`
pins that, because the case is fine and telling the analyst otherwise is its own
kind of data loss.

## A judgement with no stated reason is not recorded

`kokin_graph` accepts an empty rationale and defaults `contributing_factors_json`
to `[]`. This layer refuses both (D-033), and only this layer — the rules engine
will also call `kokin_graph`, and a machine-authored candidate legitimately has
factors rather than prose. Forcing a human rationale there would mean fabricating
one.

ADR-0007's "why" panel is the entire mechanism that forces reasoning to be written
down, and a merge whose rationale is blank cannot be reconsidered six weeks later
— which is the only reason ADR-0008 went to the trouble of making merges
reversible.

The factors list is serialised here rather than accepted as a string, so the
column always holds well-formed JSON of a known shape. A caller handing over raw
text would be writing prose into a field the why panel reads as structure.

## `ingest_url` does not exist, and the crate says so in its dependencies

`src-tauri` takes no `kokin-net` dependency. There is no live HTTP client in this
workspace — `kokin-net` has `ReplayHttp` and a policy engine and no transport — so
`IngestError::Http`, `NotSuccessful` and `Url` cannot arise from anything this
crate calls. They map to `Internal`, documented as unreachable, because if one
ever does fire the bug is here rather than in the user's request.

Collection from the internet is not yet possible from the application. That is
the largest single gap in the product and it is a dependency line, not a comment.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    289 passed, 0 failed, 1 ignored     (268 before; +21 write)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
cargo deny check                        advisories ok, bans ok, licenses ok, sources ok
python scripts/lint_sql_ordering.py     OK across 42 files
python scripts/lint_crossrefs.py        OK: 461 references across 85 files  (was 300 across 40)
python scripts/lint_research.py         OK: 24 sources, 18 claim rows
```

## Manual verification

Five A/B controls. Each was broken, the suite run, and the result compared
against what the control claims. **Two of the five did not behave as expected,
and both were defects rather than surprises.**

**A/B 1 — the actor.** Removed `deny_unknown_fields` from `NewEntityRequest`.
Result: only `a_request_cannot_name_its_own_actor` failed. It fails on
`create_entity` specifically, which is what it should do — the test walks all
seven request types and reports the one that let the actor through.

**A/B 2 — evidence required. This one failed to fail.** Disabling the empty check
in `grounding()` entirely broke **no test at all**; all 45 command-layer tests
passed.

The reason is worth stating because it is not a careless test. `kokin_graph`
refuses an ungrounded entity too, and `GraphError::Ungrounded` maps to this same
`EvidenceRequired` code — so an assertion on the code passes identically whether
or not the check in `grounding()` exists. The product behaviour was correct
throughout; the *layer* was defended by a suite that never exercised it, and a
later refactor removing the "redundant" check would have gone green.

What is only true of the command layer is what it can say. `kokin_graph` reports
"nothing in L2 exists without evidence: an entity was created with none" — a
statement about the schema. This layer knows the caller and can name the kinds
that would fill the gap. So the test now asserts the message offers the caller an
observation, and covers `add_identifier` and `relate` as well, since they reach
different `kokin_graph` functions and a check applied to one entry point and not
the others is the ordinary way this regresses. Re-broken afterwards: the
strengthened test fails, and only it.

Asserting on message prose is what D-028 rejects for *branching*, and rightly.
Here it is the only thing that distinguishes the two layers, and the alternative
is a test that cannot fail.

**A/B 3 — the stated reason.** Disabled both emptiness guards in `explanation()`
and `rationale()`. Result: only `a_judgement_with_no_stated_reason_is_refused`
failed.

**A/B 4 — the refusals.** Mapped the `GraphError` refusals back to `Storage`.
Result: **two** failed — `the_resolution_refusals_are_answers_rather_than_faults`
and `a_machine_is_refused_the_decision_only_a_person_may_make`. Expected: the
break covered three arms, and two tests name them. Not entanglement.

**A/B 5 — self-ingest.** Discarded the result of `refuse_the_case_itself`. Result:
only `a_case_cannot_ingest_itself` failed.

### The clippy finding was a hollow test, not a style complaint

`cargo clippy` rejected `for (table, id) in [("artifact", &f.artifact)]` — a loop
over a single element in `an_imported_file_arrives_whole_and_with_its_provenance`.
It was the remnant of a loop over several tables, and what it left behind was a
test whose name promised provenance and which counted one `artifact` row.

An ingest that wrote the bytes and skipped its provenance rows would have passed
it, and nothing else would have noticed, because every read path in this product
goes through the artifact. The test now walks the chain the command claims to
write: `source`, `capture`, `artifact` and `transform_run` rows all present; the
capture attributed to that source; both content hashes equal to the returned one;
and the L1 `derivation` edge recording `capture -> artifact` under that run.

Silencing the lint would have been one line and would have preserved the gap
exactly.

## The cross-reference lint was checking Markdown only

`scripts/lint_crossrefs.py` globbed `docs/**/*.md` and friends. That was right
when the arguments lived in the docs, and it stopped being right as this codebase
moved the argument for each control into the doc comment beside it.

By this increment, `write.rs`, `views.rs` and `error.rs` cited **D-032, D-033 and
A-034 nine times between them, and none of the three existed.** The lint reported
`OK: 300 internal references across 40 files` the entire time.

Extended to `crates/**/*.rs`, `src-tauri/**/*.rs` and `scripts/*.py`. Verified by
running it before the rows were written and confirming it named all nine with file
and line, then after. It now checks 461 references across 85 files — and no other
dangling reference existed anywhere in the workspace, which is the one piece of
good news in this section.

This is the second time this exact shape has been fixed. Increment 16 extended
`lint_sql_ordering.py` from `crates/` to `src-tauri/` for the same reason and
wrote down the same lesson: *a guard scoped to where a problem has occurred stops
covering where it will, and it keeps reporting OK the whole time.* Having now paid
for it twice, the general form is worth stating: **when code moves to a new
directory, the lints do not follow it, and every one of them keeps passing.**

## Known limitations

1. **A-034 is open.** `ingest_file` reads any path the webview names.
   `refuse_the_case_itself` is not the control — it only stops a case swallowing
   its own storage. The closing control is a native file picker, which needs a UI.
   Recorded as `partial`.
2. **No collection from the internet.** See above; `ingest_url` needs a transport
   in `kokin-net` that does not exist.
3. **Still no caller.** Nineteen commands, and the UI is `App.svelte` and nothing
   else. Every claim in this document and the last is about the shape of a
   payload, and no human has looked at one. Deferred deliberately to finish the
   backend first, not overlooked.
4. **`MachineMayNotAct` is unreachable from this crate** by construction, so its
   mapping is asserted by a test that constructs the error rather than by one that
   provokes it.
5. **`user:local` is not a person.** Two analysts on one machine are one actor.
6. **An identifier's evidence is stored correctly and displayed nowhere.**
   `EntityView.evidence` is entity-scoped, so `add_identifier`'s grounding round
   trips into the case and out of no view. `evidence_may_argue_against_the_thing_it_is_attached_to`
   checks it against the case directly, which is where the gap is visible as a
   fact rather than an omission. A gap in the reads.
7. **`extract` is the only transform the interface can run**, and it takes no
   options — the `Limits` are `default()`. A document past a limit reports what it
   dropped and cannot be re-run with a higher one.

## Next recommended increment

**Increment 18 — `kokin-export`.** The last stub between here and the gate-5
acceptance test: `package`, `verify` and `import` over a single transport file,
with an RFC 8785 JCS manifest.

`verify` must not need a passphrase — integrity should be checkable by someone who
cannot open the case — and it returns three verdicts rather than two, for the
reason A-032 already established here: a shredded blob and a lost one look
identical from a manifest, and reporting deliberate destruction where there was an
undetected failure is the worst available answer.
