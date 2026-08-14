# Increment 18 — packaging a case

`kokin-export` has been a five-line stub since increment 1. It is the last piece
of the 11-step reference workflow that had no implementation, and ADR-0016 had
already committed to it: making a case a directory meant the working format and
the transport format had to be different things.

This increment writes the transport format: `package`, `verify` and `import`.

## Files changed

| File | What |
| --- | --- |
| `crates/kokin-export/src/container.rs` | New. The package file format and its closed set of legal entry names. |
| `crates/kokin-export/src/manifest.rs` | New. RFC 8785 JCS serialisation, and what a package says about itself. |
| `crates/kokin-export/src/package.rs` | New. `package` and `import`. |
| `crates/kokin-export/src/verify.rs` | New. Three verdicts, no passphrase. |
| `crates/kokin-export/src/lib.rs` | The error taxonomy and the module note on what an export is. |
| `crates/kokin-export/Cargo.toml` | Workspace crates plus `blake3`, `serde_json`, `thiserror`. |
| `crates/kokin-blob/src/lib.rs` | `relative_path` made public — see below. |
| `crates/kokin-export/tests/roundtrip.rs` | New. 17 tests against real cases on disk. |
| `docs/decision-log.csv` | D-034, D-035. |
| `docs/threat-model/attack-catalog.csv` | A-035, A-036. |

**No new external dependency.** Every crate `kokin-export` uses was already in the
tree and already in `docs/dependencies.csv`.

## The package is a copy, not a re-encryption

ADR-0015 keys the database with the raw case master key and puts the only
description of how to reach that CMK in a plaintext header. `rotate_passphrase`
already exploits this: changing a passphrase rewrites the header and does not
touch a byte of case data.

An export is the same move. The package carries a header with a fresh salt and
the same CMK wrapped under a passphrase given at export time, and the database
and every blob are copied across as the ciphertext they already are (D-034).
**Nothing is decrypted to make a package**, so no plaintext exists at any point,
not even briefly in a temporary file. The keyed blob filenames survive too,
because the CMK they are keyed with did not change.

The recovery wrap is deliberately dropped. Keeping it would mean every package
ever made becomes openable the day the source case's recovery key is exposed —
and the recovery key is the one value in this system users are told to write on
paper. The honest cost, which belongs in the UI and not only here: **a package
has no recovery path.** Lose the export passphrase and that package is gone.

What re-wrapping does *not* buy is key separation. The CMK is the same key, so
anyone holding the source case's header and passphrase can decrypt a package made
from it. Getting that would mean re-encrypting every blob under a new CMK, which
is a different feature at a different price.

## `verify` takes no passphrase, and returns three verdicts

Integrity and confidentiality are different questions. A courier, an archivist,
or a recipient who has not been given the passphrase yet should still be able to
establish that a file arrived intact, so everything verification needs is
plaintext by design: the manifest, the sizes, and BLAKE3 over ciphertext that
stays ciphertext.

Two verdicts would have been the obvious shape, and it is the wrong one for
exactly the reason increment 16 established for `artifact_document` (A-032).
`Partial` means the package is hash-clean and withholds evidence *by record* — a
crypto-shredded blob, an analyst's exclusion, or a blob the source case had
already lost. `Failed` means nothing in the package's own bookkeeping explains
the discrepancy, which leaves modification, truncation, or failing storage.
Collapsing them reports a deliberate act where there was an undetected failure.

**A package holding a shredded blob can never verify `Ok`, ever.** That is
permanent, and it is the honest price of crypto-shredding rather than a defect in
the check: the key is destroyed, so those bytes can never be compared against
their content hash by anyone again.

The manifest is **not a signature**, and the report says so in the same words
ADR-0014 uses for the audit chain. It detects a truncated download, a flipped
bit, a partial copy, and a file swapped by someone who did not think to update
the manifest. It does not detect a deliberate, competent edit, because anyone who
can change a blob can recompute its entry. There is no signing key in this phase.

## Exclusion keeps the record of what was excluded

The data model names redaction, exclusion and crypto-shredding as three different
operations. Exclusion is the one this layer implements, and the temptation is to
implement it as "do not write the blob".

That produces a case whose lineage points at documents that were never mentioned,
and the recipient cannot tell that from a bug. So an excluded document leaves the
package with its `blob` row, its content hash and its `derivation` edges intact,
its bytes absent, and an `omissions` entry saying which of the three reasons
applies. `an_excluded_document_leaves_its_row_and_its_absence_on_the_record`
checks both halves: the row and the lineage arrive, and the bytes do not.

## No archive format

A package holds four kinds of entry with names this crate chooses. tar and zip
would bring arbitrary paths, symlinks, hardlinks and compression — zip-slip,
writes outside the destination, and decompression bombs, a class this project
already wrote a limiter for in increment 4 — and every one of those features
would be imported solely in order to be restricted again (D-035, A-035).

Instead there is a whitelist. `EntryName` is a closed enum, `EntryName::parse` is
the only constructor from wire bytes, and destinations are chosen by *matching on
the enum* rather than by composing a path from the name that arrived. Traversal
is not filtered; it is not expressible. Uppercase hex is refused rather than
folded, because one spelling is two files on Linux and one on Windows, which is a
way to smuggle a second file past a manifest that lists the address once.

Nothing is compressed. The payload is AEAD ciphertext and a SQLCipher database,
both incompressible, so compression would cost CPU, buy nothing, and reintroduce
the one attack this format is otherwise immune to.

## The bug this increment shipped with for an afternoon

The case database runs in **WAL mode**. Writes live in `case.db-wal` until a
checkpoint folds them in, and `package` copied `case.db` alone.

The result was a package that was internally consistent, hashed correctly,
verified `Ok`, imported cleanly, and was missing everything done since the last
checkpoint — which is the work an analyst is most likely to be exporting. Nothing
about the package could reveal it, because the manifest faithfully described the
stale bytes.

It was found by the round-trip test failing on `QueryReturnedNoRows` for an
entity created moments before the export, and it is now A-036 with a test of its
own. `package` issues `wal_checkpoint(TRUNCATE)` first — TRUNCATE rather than
PASSIVE, because PASSIVE reports success regardless of how much it left behind,
which is the same silent partial copy with an extra step.

Worth recording as a general shape: **this class of bug produces artefacts that
pass every integrity check, because the integrity check describes the wrong
bytes.** Hashes prove a file was not altered after it was written. They prove
nothing about whether the right thing was written.

## The blob layout rule now lives in one place

`survey_blobs` originally looked for blobs at `blobs/<storage_name>`. The store
shards two levels deep — `blobs/ab/cd/<name>.blob` — via a private `path_for`,
so every blob in every case looked missing, and a clean case verified `Partial`
with an omission list naming its entire contents.

The fix was not to copy the layout into `kokin-export`. `kokin_blob::relative_path`
is now public and both crates call it, because two copies of that rule would mean
changing the fan-out later produces packages that import into a case whose blobs
are all unfindable — with every hash still matching.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace
    312 passed, 0 failed, 1 ignored     (289 before; +23 export)

cargo clippy --workspace --all-targets -- -D warnings   clean
cargo fmt --all --check                                 clean
python scripts/lint_crossrefs.py        OK: 489 references across 91 files
python scripts/lint_sql_ordering.py     OK across 47 files
python scripts/lint_research.py         OK: 24 sources, 18 claim rows
```

## Manual verification

**A/B — the checkpoint.** Removed `checkpoint(&open.conn)?` from `package`.
Result: four tests failed —
`work_done_immediately_before_the_export_is_in_the_package`,
`an_exported_case_opens_under_its_new_passphrase`,
`an_excluded_document_leaves_its_row_and_its_absence_on_the_record` and
`the_audit_chain_still_verifies_after_the_trip`. Four rather than one because
every test that builds a case and immediately packages it is exercising the same
property; the first is the one that names it.

**A/B — the shard rule.** The original flat-path bug is itself the A/B, and it is
worth noting what it looked like from outside: no error, no crash, a package that
verified `Partial` and listed every document in the case as an omission. A
verdict of `Partial` is a normal, expected state, which is exactly why a bug that
produces one is quiet.

**Two test bugs found by their own failures**, both mine rather than the code's.
`a_modified_package_fails_and_says_which_entry` flipped a byte an eighth of the
way from the end and hit `case.db`, which is the largest entry — the assertion
that a blob was named was checking the wrong thing about a package that was
correctly reported as damaged. And `the_audit_chain_still_verifies_after_the_trip`
asserted the manifest's head equals the restored case's head, which is false and
should be: opening the restored case appends `case.opened`, exactly as opening
any case does. **An equal count would have meant the copy had stopped recording.**
The assertion now checks the packaged head is the event *before* the restored
case's own open.

## The one comment that was a lie

`assemble` copies the staged package into the final file with the manifest in
front, and the container writes an entry from a `Read` while reading one into a
`Write`. Bridging those by buffering was the quick way, and the loop carried the
comment `// Streamed through, not buffered: the database entry is the whole
case.` — which said the opposite of what the code did, and gave the correct
reason for wanting it.

`Writer::begin_entry` now returns an `EntryWriter` that implements `Write` and
hashes as it goes, so `assemble` streams and the buffer is gone. The whole case
database is no longer held in memory to move it a few bytes down a file.

`a_document_larger_than_the_copy_buffer_survives_intact` covers the path with a
1 MiB non-repeating blob, because every other test's evidence fits in a single
64 KiB read and would not notice a chunk-loop error at all.

The general point is small and worth keeping: **a comment describing an intention
reads exactly like a comment describing behaviour**, and this one survived being
written, reviewed and committed inside a document arguing for honest reporting.

## Known limitations

1. **The package is written twice.** Once to staging, once to the final file,
   because the manifest describes hashes that only exist after the bytes are
   written and has to appear first. The alternative reads every blob twice
   instead; neither holds a case in memory but both do double the I/O.
2. **`verify` does not open the database**, so it cannot check that the case
   inside is coherent — only that the bytes match the manifest. A package can
   verify `Ok` and contain a corrupt SQLCipher file. That check needs the
   passphrase, which is the whole thing `verify` is designed not to need.
3. **No command-layer wiring.** `package`, `verify` and `import` are reachable
   from the test suite and from no Tauri command. Deliberate: the acceptance test
   is next and drives the crates directly.
4. **`docs/testing/acceptance-plan.csv` does not exist yet.** It maps rows to the
   11 reference-workflow steps and belongs with increment 19, which is the test it
   describes. Creating a partial one now would be a document nobody is using.
5. **No signature, and no RFC 3161 timestamp.** Tamper-evident, not tamper-proof.
   Deferred with ADR-0014's reasoning, not overlooked.
6. **Import does not verify first.** It checks every entry against the manifest as
   it writes, and refuses and cleans up on a mismatch, but it does not run the
   full `verify` pass — that is a separate call needing no passphrase, and folding
   it in would mean verifying twice or offering an import that quietly skipped it.
7. **A package cannot be opened by any other tool.** The container is
   purpose-built, so there is no `unzip` fallback in an emergency. Accepted: the
   contents are encrypted and unreadable without the passphrase regardless.

## Next recommended increment

**Increment 19 — the gate-5 acceptance test.** Every crate the 11-step reference
workflow needs now exists. One CI test walking the whole sequence: create an
encrypted case, ingest, extract, create entities and a relationship, show the
relationship with its evidence, accept it, search, review the activity history,
close and reopen with no data loss, export and restore — then repeat it offline
from recorded fixtures.

`docs/testing/acceptance-plan.csv` maps each row to its step, and that is where
the gaps between what the spec asks for and what these eighteen increments built
will become visible as a list rather than as an impression.
