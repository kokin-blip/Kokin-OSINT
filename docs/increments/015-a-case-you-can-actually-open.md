# Increment 15 — a case you can actually open

Increment 14 made reopening a case correct. `src-tauri` still exposed one
command — `storage_encryption_status` — so nothing outside the test suite had
ever opened one. This increment gives the application a case lifecycle:
create, open, recover, close, and know which case is current.

It also fixes something increment 14 listed as a limitation and increment 15
could not proceed without: a `RecoveryKey` had no textual form, so every case
created through the application would have had an unusable recovery path,
discovered on the day it was needed.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-keys/src/phrase.rs` | New. `to_phrase` / `from_phrase`. 8 tests. |
| `crates/kokin-keys/src/lib.rs` | Exports them; `KeyError::MistypedRecoveryKey`. |
| `src-tauri/src/error.rs` | New. `CommandError { code, message }`, `ErrorCode`. |
| `src-tauri/src/session.rs` | New. `Session`, `CaseView`, `NewCaseView`, `SessionView`. |
| `src-tauri/src/lib.rs` | Six commands, each one line. Managed `Session` state. |
| `src-tauri/tests/session.rs` | New. 9 tests, no webview. |
| `docs/threat-model/attack-catalog.csv` | A-029, A-030. |
| `docs/decision-log.csv` | D-026, D-027, D-028. |

No new external dependency: `blake3` and `rusqlite` were already in the tree.

## Writing a key down

Thirty-two random bytes need a form a person copies onto paper and types back a
year later. Crockford base32, eight groups of seven, `I`/`L`/`O`/`U` omitted and
the read-alike substitutions applied on input, plus a 24-bit BLAKE3 checksum
(D-026).

The checksum is not integrity — the AEAD tag on the wrapped key is what makes a
wrong key fail closed. It exists so that *a typo is reported as a typo*, before
Argon2 runs, instead of surfacing as `WrongKey` two seconds later and leaving
the user unable to tell whether they mistyped or their case is damaged. Those
are different problems with different remedies, and
`a_mistyped_recovery_key_is_told_apart_from_a_wrong_one` pins both directions:
one character changed gives `MistypedRecoveryKey`, a valid key for a different
case gives `WrongPassphrase`.

**Twenty-four bits, not twenty, and the reason is a bug my own test found.**
With a 20-bit checksum the encoding needs 56 characters to carry 276 bits, which
leaves four bits nothing reads. Four checksum bits shared a character with the
tail of the key and went uncompared, so flipping them produced a *different
written form for the same key* — a non-canonical encoding, and a class of
single-character typo that parses successfully and tells the user they typed it
correctly. `one_wrong_character_is_caught` caught it because it asserts the
whole population — every position, every substitution, 1,736 typos — rather than
sampling. At 24 bits, 256 + 24 = 280 = 56 × 5 exactly, there are no bits the
decoder ignores, and `the_encoding_divides_evenly_into_characters` guards that.

## A code, and a message

Every command returned `Result<String, String>`, so the only thing an interface
could branch on was the text of an error. A wrong-passphrase dialog that stops
appearing because someone improved a message is a bug that survives review,
because both changes look correct in isolation.

`CommandError { code, message }` (D-028) splits the part a machine reads from
the part a person reads. `ErrorCode` is closed, and `From<StoreError>` maps
**variant by variant** rather than calling `to_string()` — so a variant added to
`StoreError` later cannot arrive at the webview by inheritance, carrying whatever
it happens to hold. `Internal` is documented as a bug, not a bucket.

## One case at a time

Opening a second case while one is open is refused, not performed (D-027).

The convenient version swaps the case. Nothing errors — both cases are the
user's, both are valid — and every panel already on screen keeps describing the
first one while commands act on the second. The analyst files an observation, or
a merge decision, into the wrong investigation, and the audit chain faithfully
records them doing it (A-030). There is no integrity failure to detect
afterwards; there is just a case they can no longer trust, and by symmetry
another one.

## What crosses the boundary

The webview is the least trusted part of this process. `OpenCase` holds the case
master key, and blob filenames are keyed with it, so a leaked CMK does not merely
decrypt a case — it locates it (A-029).

The guarantee is structural: the domain crates take no `serde` dependency, so
`OpenCase` and `CaseHeader` **cannot** be serialised. The surest way to keep a
type from gaining `Serialize` is for the crate defining it not to know what
`Serialize` is. View types live in `src-tauri`, in one file, and
`the_views_carry_exactly_these_fields_and_no_key_material` asserts `CaseView`'s
exact field set — so adding a field is a decision someone makes against a test
that names the boundary, rather than a convenience that ships because it
compiled.

One deliberate exception: `NewCaseView.recovery_key`, returned once by
`create_case` and by nothing else. There is no command that shows it again;
that would mean holding it for the session or re-deriving it, and neither is
permitted. An interface that fails to put it in front of the user has destroyed
the recovery path, which is a thing the UI must make impossible to skip past.

## Commands are one line each

Every `#[tauri::command]` is a one-line call onto a `Session` method. `Session`
knows nothing about Tauri, so the whole lifecycle — including every refusal — is
tested by ordinary `cargo test` with no window on screen. What those 9 tests do
not cover is exactly the one line of marshalling per command, which is the
quantity of untested code this arrangement buys.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    253 passed, 0 failed, 1 ignored     (236 before; +8 phrase, +9 session)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
python scripts/lint_sql_ordering.py    OK across 31 files
python scripts/lint_crossrefs.py       OK: 274 references across 38 files
python scripts/lint_research.py        OK: 24 sources, 18 claim rows
```

## Manual verification

**A/B 1 — refusing the second open.** Removed both `if guard.is_some()` guards
from `create` and `open`, which is the swap-silently implementation. Result:
exactly `opening_a_second_case_is_refused_and_the_first_stays_open` failed; the
other 8 passed, so the control is scoped to what it claims.

**A/B 2 — the checksum.** Made the comparison in `from_phrase` unreachable.
Result: `one_wrong_character_is_caught` failed and nothing else in `kokin-keys`
did — 16 of 17 still passed. Notably `a_key_survives_being_written_down_and_typed_back`
still passed, which is the point: a round-trip test cannot see this defect at
all, and would have shipped it.

**A/B 3 — the field set.** Added a `blobs_dir` field to `CaseView` and populated
it. Result: `the_views_carry_exactly_these_fields_and_no_key_material` failed
with the two lists side by side. This is the A/B that matters most here, because
that test is the only thing standing between "someone needs one more field" and
a boundary that grows without anyone deciding it should.

## Known limitations

1. **Nothing reads a case yet.** `Session::with_case` exists and has one test;
   no command uses it. Search, ingest, the graph and coverage are all still
   unreachable from the interface. That is increment 16.
2. **The UI is still `App.svelte` and nothing else.** These commands have no
   caller. The lifecycle is correct and untested end to end, in the sense that
   no human has clicked it.
3. **The passphrase crosses IPC as a `String`.** Unavoidable — the user types it
   into the webview — but it is a plain `String` with no zeroization on either
   side of the boundary, and Rust cannot zeroize what JavaScript allocated.
   Worth stating rather than implying the layer is airtight.
4. **No timeout, no auto-lock.** A case stays open with the key in memory until
   something calls `close_case`. An idle lock is real work — it has to decide
   what happens to in-flight jobs — and belongs after there are jobs.
5. **A poisoned mutex is unrecoverable without a reopen.** A command that
   panics while holding an open case leaves the session refusing everything with
   `Internal`. Correct in that the layer genuinely cannot tell whether anything
   is damaged, and coarse.
6. **`create_case` cannot decline a recovery key.** The header supports
   `cmk_wrapped_by_recovery: None` and `CaseView` reports it, but no command
   produces such a case. Deliberate: the option to make a forgotten passphrase
   permanent should be a loud UI decision, not a parameter that defaults.

## Next recommended increment

**Increment 16 — the read commands.** `search`, returning results and coverage in
one payload so an interface cannot render a result list without its caveat;
`artifact_document`, going through `blob_access` so the three answers reach the
UI as three answers; and the entity views that read through the merge map.

This is where `Session::with_case` stops being a lonely helper, and where the
DTO discipline gets its real test — a `Hit` has a `relevance`, a `canonical_id`
and seven confidence dimensions that must never be added up, and the view layer
is exactly where a composite score would get invented for the convenience of
sorting.
