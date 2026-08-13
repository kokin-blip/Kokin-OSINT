# Increment 14 — closing a case and opening it again

Fourteen increments in, no test had ever closed a case and opened it. Every one
of them worked inside a single process, on a `CaseMasterKey` the test generated
and then handed to both the write and the read. That is a fixture, not a case: a
real investigation is opened on Monday, closed, and opened again on Thursday,
and the only thing crossing the gap is a directory and something the analyst
remembers.

Drawing that line found a defect that had been in the codebase since increment 3
and could not be found any other way.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-store/src/lib.rs` | `OpenCase { conn, paths, cmk }`; `create_case`, `open_case` and `open_case_with_recovery_key` return it. Hand-written redacting `Debug`. |
| `crates/kokin-store/src/evidence.rs` | New. `blob_access` and `BlobAccess`. |
| `crates/kokin-store/Cargo.toml` | Depends on `kokin-blob` for `BlobRef`. |
| `crates/kokin-extract/src/lib.rs` | `load_readable_blob` now calls `blob_access` instead of carrying its own copy. |
| `crates/kokin-ingest/tests/reopen.rs` | New. 4 tests. |
| 20 call sites | Destructure `OpenCase`. Mechanical. |
| `docs/threat-model/attack-catalog.csv` | A-027, A-028, both mitigated. |
| `docs/decision-log.csv` | D-024 (`OpenCase`), D-025 (three answers, not two). |

No migration: the schema already held everything needed. This is entirely about
what the code was willing to hand back.

## A case that reopens perfectly and cannot show a document

`open_case` returned a connection and some paths, and dropped the case master
key on the floor. Blob *filenames* are keyed with that key —
`storage_name(hash, cmk) = blake3::keyed_hash(cmk, hash)`, so that a directory
listing does not leak which documents a case holds — which means a reopened case
could not locate its own evidence, never mind decrypt it.

What makes A-027 worth its row is how healthy the failure looks. The database
opens. Artifacts, hashes, lineage, observations, search, coverage, the graph: all
present, all correct. Nothing errors. The case is indistinguishable from a
working one right up to the moment someone asks to see the document behind a
conclusion, which is the moment a case has to hold up.

And no test could catch it, because every test was its own round trip. The bug
lived exactly in the gap the test suite defined as out of scope.

`OpenCase` makes the incomplete case unrepresentable (D-024): there is no way to
get a connection out of this crate without the key that completes it. The
`Debug` impl is written out rather than derived — `CaseMasterKey` already
redacts itself, so deriving would be safe today, but then the safety would
depend on a decision made in another crate, and `{:?}` on an open case is
precisely the call that ends up in a log line or a panic message.

## Destroyed on purpose is not the same as gone

Getting from a hash to bytes needs the wrapped per-blob key out of the `blob`
table, whose key columns are nullable on purpose: crypto-shredding sets them to
NULL and leaves the row, so the record that evidence was collected outlives the
evidence. That reassembly existed twice — once in `kokin-extract`, once copied
into a test — and the application layer still had no way to do it at all. Twice
is how the third copy gets the distinctions wrong.

`blob_access` returns three answers as **values**, not two values and an error
(D-025):

- `Readable(BlobRef)` — the key is here.
- `Shredded { shredded_utc }` — the row survives, the key does not. A normal
  state of a healthy case.
- `Unrecorded` — no row. Reached from an artifact, that is a broken case.

Making `Shredded` an error variant was the tempting shape, and wrong: an error
is what a UI renders in red, and shredding is a supported operation someone
performed deliberately. The inverse (A-028) is worse still — if the two collapse,
a case that has genuinely *lost* evidence presents as one whose owner destroyed
it on purpose, the analyst stops looking, and real data loss is filed as policy.

`Err` from `blob_access` now means a database failure and nothing else.

The fourth state — key present, ciphertext gone, e.g. a restore that missed
`blobs/` — is deliberately not reported here. Checking it needs the blob store
and the key, which turns a cheap database read into I/O, and this function would
be reporting on a file it had only stat-ed. The read itself already answers with
`BlobError::NotFound`, which is the better answer because something actually
opened the file.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    236 passed, 0 failed, 1 ignored     (232 before; the 4 new ones are reopen.rs)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
python scripts/lint_sql_ordering.py    OK across 30 files
python scripts/lint_crossrefs.py       OK: 265 references across 37 files
```

`reopen.rs` covers: the passphrase round trip through the database to the bytes;
the wrong passphrase failing at the key rather than at the blob; the recovery key
reaching the evidence the passphrase would have; and shredded evidence reading as
shredded, with an unrecorded hash as a third distinct answer, while its
provenance row survives.

## Manual verification

**A/B 1 — the key that crosses the gap.** Made `finish_open` return a freshly
generated `CaseMasterKey` instead of the one that unlocked the case, which is the
defect as it actually existed. Result: exactly
`a_case_can_still_read_its_evidence_after_being_closed_and_opened` and
`a_recovery_key_reaches_the_evidence_the_passphrase_would_have` failed, and both
failed *at the blob layer* — `contains` false, then `BlobError::NotFound` on a
keyed filename — after every database assertion above them had passed. That is
the shape of A-027 reproduced, not merely a red test: the case looked intact and
could not produce a document. `the_wrong_passphrase_never_gets_as_far_as_the_blobs`
and the shredding test still passed, so the control is scoped to what it claims.

**A/B 2 — the three answers.** Made `blob_access` return `Unrecorded` where it
returns `Shredded`. Result:
`evidence_destroyed_on_purpose_does_not_read_as_a_broken_case` failed, **and so
did `kokin-extract`'s pre-existing `a_shredded_blob_is_reported_as_shredded`** —
which is the more valuable half, because it shows the rewiring of
`load_readable_blob` onto the shared function preserved the behaviour that crate
already guarded rather than quietly loosening it.

## Known limitations

1. **A `RecoveryKey` still has no textual form.** It is 32 bytes of `Secret`
   with no encoding, so "the analyst wrote it down" is aspirational —
   `a_recovery_key_reaches_the_evidence...` holds the value across the reopen as
   a Rust value, which is honest about the crypto and dishonest about the
   workflow. Rendering it (and parsing it back, with a checksum, because it will
   be typed by hand) is its own piece of work.
2. **Nothing calls `open_case` outside tests yet.** `src-tauri` still exposes one
   command. This increment makes the reopen *possible* and correct; it does not
   make it reachable from the product.
3. **The fourth state is unreported.** Above: a case whose `blobs/` is missing
   looks healthy until each individual read fails. There is no "check this case
   is whole" operation, and a restore-verification pass is the obvious place for
   one.
4. **`blob_access` is per-hash.** A view listing fifty artifacts with their
   availability makes fifty queries. Fine at investigation scale on an indexed
   primary key, and worth revisiting when there is a UI that does it.
5. **A shredded row need not carry `shredded_utc`.** The append-only trigger
   enforces the key going away, not the bookkeeping around it, so
   `BlobAccess::Shredded` carries an `Option`. Tightening the trigger is a
   migration and would be a real improvement.

## Next recommended increment

**Increment 15 — the Tauri command surface.** Increment 13 said the UI stops
being deferrable, and this increment says the same thing from the other side: an
`OpenCase` that nothing outside tests ever opens is a fix nobody benefits from
yet.

The shape, decided but not yet built: managed `OpenCase` state behind a mutex; a
structured `CommandError { code, message }` so the UI never string-matches on
prose; view DTOs defined in `src-tauri` rather than adding `serde` to the domain
crates, so the wire format cannot quietly become the domain model; and
`#[tauri::command]` wrappers as one-liners over plain testable functions. The
search command returns results and coverage together in one payload, because a
UI that can render a result list without its caveat eventually will.

The hard constraint carries over unchanged: the CMK is live secret material that
must never escape the process. `OpenCase` does not implement `Serialize` and
must not gain it, and nothing crossing the IPC boundary may carry a key.
