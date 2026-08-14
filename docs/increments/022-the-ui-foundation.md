# Increment 22 — somebody looks at it

Twenty-one increments built a back end and a command layer with **no caller**.
Every claim since increment 14 has been about the shape of a payload, and no
human had seen one.

This increment renders four screens and drives them, on Windows, against a real
encrypted case. The most important thing it produced is not code — it is that
the boundary has now been exercised end to end by something other than a test.

## Files changed

| File | What |
| --- | --- |
| `ui/src/lib/bindings.ts` | New. The whole boundary in TypeScript: error codes, view types, six command wrappers. |
| `ui/src/lib/session.svelte.ts` | New. One case or none, mirroring the Rust session. |
| `ui/src/lib/CaseGate.svelte` | New. Create, open, unlock by recovery key. |
| `ui/src/lib/RecoveryKey.svelte` | New. The screen that must not be skippable. |
| `ui/src/lib/ErrorNotice.svelte` | New. Failures presented by code, never by prose. |
| `ui/src/App.svelte` | The shell. Was 108 lines calling one command. |
| `src-tauri/tests/bindings.rs` | New. Four tests holding the two sides of the boundary together. |

No new dependency, on either side.

## Nothing makes the two sides of the boundary agree

Tauri passes JSON. The Rust compiler never sees `bindings.ts` and `tsc` never
sees `views.rs`, so a field added on one side and forgotten on the other
produces no error anywhere — just a value the interface silently never reads.

The failure has a shape worth naming: **it is not a crash, it is a panel that
renders correctly and omits something.** `has_recovery_key` is the case in
point. If it stopped being mirrored, the case screen would go on looking right
while no longer telling anyone that a forgotten passphrase is permanent.

So `bindings.rs` holds four checks:

- **Every field of a bound view type appears in the TypeScript.**
- **Every view type is either bound or explicitly listed as deferred**, with the
  increment that will bind it. An explicit list rather than a filter, so a *new*
  view type fails until somebody classifies it. Twenty-six types are on that
  list today and each later increment deletes lines from it.
- **Every `ErrorCode` variant appears in the TypeScript union.** `ErrorNotice`
  holds a `Record<ErrorCode, Presentation>`, so TypeScript already refuses a
  missing key — but only against a hand-written union, and this checks the union
  against the Rust enum, which is the link no compiler covers.
- **Only `bindings.ts` calls `invoke`**, the mirror of
  `serialize_is_derived_only_where_the_boundary_is_declared` on the Rust side.

## The recovery-key screen

`create_case` returns the key once. Nothing stores it in readable form and no
command returns it again (D-026). If this screen can be walked past, the case's
recovery path is destroyed and nothing says so until a passphrase is forgotten,
possibly months later.

So: no Escape, no click-outside, no close button. The acknowledgement is a
checkbox **and** retyping one group of the key back. A button alone is one
reflexive click from the same outcome as no screen at all, and copying is not
recording — a clipboard does not survive a reboot, and the day this key is needed
is not today.

One group rather than all eight, because a wall of retyping gets defeated by
pasting. The group asked for is the second from the end: the first is what a
screenshot taken before scrolling captures, and the last is what a truncated copy
most often keeps.

**None of this verifies the key was saved, and it does not pretend to.** It
raises the cost of skipping above the cost of complying, which is the most a
screen can do.

## Failures are read by code and shown by prose

`ErrorNotice` maps each of the nineteen `ErrorCode` values to a title, a remedy,
and a **tone**. Tone is the load-bearing part: several of these codes are the
product working exactly as designed — a machine refused a decision only a person
may make, a case declining to be swapped out from under a rendered panel — and
rendering those in red teaches an analyst that the safety rails are faults.

`case_already_open` gets an action rather than an apology (D-027, A-030), and the
tone words carry the meaning so a refusal and a fault are distinguishable in
monochrome and to a screen reader.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    318 passed, 0 failed, 2 ignored     (314 before; +4 bindings)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
npm run check (svelte-check)            108 files, 0 errors, 0 warnings
python scripts/lint_crossrefs.py        OK
python scripts/lint_sql_ordering.py     OK
python scripts/lint_research.py         OK
```

## Manual verification — the application, driven

Built `--release`, launched, and driven with synthetic mouse and keyboard against
the real binary. Six screenshots, each read rather than assumed.

**The first run failed**, and it is worth recording why: a plain `cargo build
--release` produces a binary still pointing at `devUrl`, so the window rendered
Edge's *"Hmmm… can't reach this page"*. The Tauri CLI is what swaps in the
bundled frontend. Running Vite on 5173 beside it was the quicker route to
looking at the thing; **the bundled-asset path is therefore still unverified
locally**, and CI's `tauri build` is the only evidence it works.

Then, in order:

1. **The gate renders**, and the footer reads *"cases are written with sqlcipher
   4.14.0 community"* — the UI calling through the typed bindings, over IPC, into
   Rust, querying the real cipher backend. The boundary works.
2. **`Create case` stays disabled** with the passphrase empty. `canSubmit` holds.
3. **The recovery key appears**: eight Crockford groups of seven, `Continue`
   disabled, asking for group 7 of 8.
4. **Ticking only the checkbox and clicking `Continue` does nothing.** One
   condition is not enough — the gate holds.
5. **Typing the group in lowercase** reads `Matches.` and enables the button, so
   the Crockford case-insensitivity is real rather than assumed.
6. **The case opens** at schema version 7, reporting its recovery key as
   available in a sentence rather than a boolean.
7. **`Close case` returns to the gate**, and with the file unlocked the first
   sixteen bytes of `case.db` are `.b4.$).6y...N3Bc` — ciphertext, not
   `SQLite format 3`.

### A/B on each new test

- Removed `has_recovery_key` from the TypeScript → only
  `the_typescript_bindings_cover_every_bound_view_field` failed, naming
  `CaseView.has_recovery_key`.
- Added a `StrayView` to `views.rs` → only
  `every_view_type_is_either_bound_or_knowingly_not` failed, naming it.
- Imported `invoke` into `ErrorNotice.svelte` → only
  `only_the_bindings_module_calls_invoke` failed, naming the file.

## Known limitations

1. **Paths are typed, not picked.** There is no native file dialog until
   increment 25, and a placeholder showing the shape of a case path is more use
   than a browse button that does not exist.
2. **The bundled-asset build path is unverified locally.** See above.
3. **`case_already_open` is unreachable from this interface**, because the gate
   is not rendered while a case is open. Its presentation is defensive, and the
   only thing that exercises it is a Rust test.
4. **The binding check is a text lint, not a type check.** It matches field
   *names*. A string mirrored as a number passes it and fails at runtime.
   Generating the bindings from the Rust types would catch that, and is the right
   answer if this grows past one file.
5. **No component tests.** Every UI claim above rests on screenshots I read, which
   is verification of a build rather than a regression guard. A change to
   `RecoveryKey.svelte` that made the gate skippable would pass everything in CI.
   That is the largest gap this increment leaves.
6. **A26 screens still to come** — evidence, entities, search, writes — and the
   deferred list in `bindings.rs` names all twenty-six types.
7. **Argon2id at 64 MiB is visibly slow on create**, which is correct and is
   reported to the user as deliberate rather than left to look like a hang.

## Next recommended increment

**Increment 23 — evidence and lineage.** The evidence table, observation detail
with the lineage panel, and `artifact_document`'s five states each presented
distinctly — `Shredded` in particular must not look like an error, which is
increment 16's finding rendered rather than argued.

That increment also has to settle **A-031**: `tauri.conf.json` sets
`frame-src 'none'`, and the original design named a sandboxed iframe as the way
to render captured HTML. One of the two has to move, and the attack row stays
`partial` until it does.
