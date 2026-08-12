# Increment 3 — the case bundle, and what it forced us to change

Date: 2026-08-12 · Branch: `feat/phase0-foundations`

## What this increment found

Increment 3 was planned as "open/create, migrations, append-only triggers, audit
hash-chain". Writing it surfaced an architectural contradiction that had to be
resolved first.

ADR-0015 keys the case database with the **case master key**, which exists only
as ciphertext — wrapped under a passphrase-derived KEK and under a recovery key.
To open a case you must first read the salt, the KDF profile, and the wrapped
keys. If those live *inside* the encrypted database, opening it requires the key
that the header is needed to obtain. A circular dependency.

The alternatives were each worse:

- **Key the database with the KEK instead.** The header could then live inside,
  but a recovery key could no longer open anything — it deletes the recovery
  path, which is half of why ADR-0015 exists.
- **Prefix the header to the database file.** SQLCipher's
  `cipher_plaintext_header_size` reserves leading bytes but they must remain a
  valid SQLite header; it is not free space. A custom container needs a custom
  VFS — a lot of error-prone machinery in the one place where bugs mean
  unopenable evidence.

Checking ADR-0003 showed the premise was already broken. It is titled "one case
is one SQLite file" and its own consequences section says:

> Blobs do not live in the database. Large binary evidence goes to the
> content-addressed store (`kokin-blob`) with only its hash in SQLite.

**A case was never going to be one file.** That went unnoticed because increment
1 stored nothing but a probe table. ADR-0016 records the revision: a case is a
bundle directory. ADR-0003's status now says which of its claims survive.

Consequently the audit hash-chain and append-only triggers are **not** in this
increment. The `audit_event` table exists in migration 1 so the next increment
does not have to migrate rows that already exist, but the chaining logic is
deferred. Splitting here was the right call: a schema revision and a new
integrity mechanism in one increment would have made a bisect useless.

## Files changed

| File | What changed |
|---|---|
| `crates/kokin-store/src/header.rs` | New. Header read/write, unlock by passphrase or recovery key, rotation. |
| `crates/kokin-store/src/migrations.rs` | New. `user_version` migrations, schema fingerprint, golden test. |
| `crates/kokin-store/src/lib.rs` | Rewritten around `create_case` / `open_case` / `change_passphrase`. |
| `src-tauri/src/lib.rs` | Probe now queries the linked library instead of creating a throwaway case. |
| `docs/adr/0016-a-case-is-a-bundle-directory.md` | New; revises ADR-0003. |
| `docs/adr/0003-*.md` | Status updated to record what it no longer claims. |
| `docs/dependencies.csv` | `serde_json` added — it had been in use since increment 1 without a registry row. |

## Behaviour added

- **Create a case**: makes the bundle, generates salt / CMK / recovery key,
  writes the header last so a failed create leaves no directory that looks like
  a valid case.
- **Open** by passphrase or by recovery key.
- **Change passphrase**: rewrites `header.json` only. No case data is
  re-encrypted, and the existing recovery key keeps working.
- **Migrations** with `user_version`; a case from a newer build is refused by
  name rather than opened and misread.

This is reference-workflow **step 1** (create an encrypted local case) and
**step 9** (close and reopen without data loss).

## Tests executed and results

```
cargo test --workspace --all-targets    23 passed, 0 failed   (14 store + 9 keys)
cargo clippy --workspace --all-targets  0 warnings (-D warnings)
cargo fmt --all -- --check              clean
cargo deny check bans licenses sources advisories
                                        advisories ok, bans ok, licenses ok, sources ok
npm run check                           103 files, 0 errors, 0 warnings
python scripts/lint_research.py         24 sources, 18 claim rows
python scripts/lint_crossrefs.py        121 references across 24 files
```

The fourteen store tests, with the ones that carry weight called out:

- a case survives close and reopen
- wrong passphrase is rejected **and named as such**
- **a recovery key opens a case whose passphrase is lost** — impossible to write
  before increment 2, because there was nothing but the passphrase
- **changing the passphrase preserves data and the recovery key** — asserts the
  new passphrase works, the old one fails, *and* a recovery key written down
  before the change still works
- a directory without a header is not a case
- a corrupt header is named as such, not surfaced as a SQLite error
- a header from a future version is rejected by name
- **tampering with the header's KDF profile is detected** — the reason `aad`
  binds the profile id and not just the case id
- the case database contains no plaintext and lacks the SQLite magic header
- **the header carries no usable key material** — a canary passphrase and the
  recovery key must not appear in the plaintext header
- migrations: fresh reaches target, running twice is a no-op, a newer schema is
  refused, and the schema matches a **golden fingerprint**

## Two bugs the tests caught

1. **`PRAGMA key = x'...'` is a syntax error.** SQLCipher parses the pragma
   argument as a string and *then* inspects it for the `x'...'` form, so the
   literal must be wrapped in double quotes: `PRAGMA key = "x'...'"`. This is
   not ordinary SQL blob syntax and is easy to get wrong from memory.
2. **The golden fingerprint I hand-wrote was wrong.** It included
   `sqlite_sequence`, which the query already excludes via `name NOT LIKE
   'sqlite_%'` and which SQLite does not create until the first AUTOINCREMENT
   insert. Writing an expected value by hand is exactly the mistake a golden
   test exists to catch, and it caught mine.

A third was my own test being wrong rather than the code: asserting the header
never contains the string "passphrase" failed on the *field name*
`cmk_wrapped_by_passphrase`. Replaced with a distinctive canary value, which is
what the test meant to check.

## Performance measurements

Store tests take 16.2s wall, dominated by Argon2id: eight tests each create or
open a case at profile V1 (64 MiB, ~114 ms measured in increment 2), several
more than once. That is the KDF working as intended, not a slow test suite.

Worth noting for later: the test profile in `kokin-keys` is 1 MiB precisely so
its own tests stay fast. `kokin-store` deliberately does **not** use it — these
tests exercise the real profile end to end, and 16s is an acceptable price for
that on a suite this small. If it grows, that is the knob.

## Security and privacy implications

- **The header is plaintext by design and contains no secrets.** A salt is not
  secret; the wrapped keys are AEAD ciphertext. Same shape as LUKS or `age`. A
  test asserts neither the passphrase nor the recovery key appears in it.
- **Newly documented exposure:** the header being plaintext means the existence
  of a case, its creation time, format version, and KDF parameters are visible
  to anyone with filesystem access. Case *contents* are not. Plausible
  deniability about a case existing was never in the threat model; ADR-0016 now
  says so explicitly rather than leaving it implied.
- `aad` binds each wrap to the case id **and** the KDF profile id, so an edited
  header fails to authenticate instead of deriving a key that decrypts nothing.
- The database is keyed with the raw CMK, bypassing SQLCipher's own KDF. That is
  deliberate: Argon2id at 64 MiB has already done far more work than SQLCipher's
  default PBKDF2, and doing both would be cost without benefit.
- Rotation does not invalidate the recovery key. Silently breaking a recovery
  key the user wrote down months ago would be a data-loss bug that only shows up
  when it is too late to fix.

## Known limitations

1. **No audit hash-chain yet.** The table exists; the chaining does not.
2. **No append-only triggers on L0.** The L0 tables do not exist yet either —
   the three-layer schema (ADR-0006) lands with the increment that has something
   to store.
3. **No concurrent-access lock.** ADR-0003 calls for WAL plus a lock file; two
   application instances opening one case is currently undefined behaviour.
4. **The recovery key still has no human-facing encoding.** Returned as 32 raw
   bytes. Not transcribable; BIP39 or Crockford base32 belongs with the UI that
   shows it.
5. **`create_case` is not atomic.** The header is written last, so a crash
   mid-create leaves a directory with a database and no header — correctly
   detected as "not a case", but not cleaned up.
6. **Nothing in the UI uses any of this.** The Tauri layer still exposes only
   the encryption-status probe.

## Manual verification

```
cargo test -p kokin-store          # 14 tests
```

To inspect a real bundle:

```
cargo test -p kokin-store a_case_survives -- --nocapture
```

Then, in a scratch program or test, create a case and look at the directory: it
must contain `header.json`, `case.db`, and `blobs/`. `header.json` must be
readable JSON containing a salt and two wrapped keys, and **must not** contain
the passphrase. `case.db` must not begin with `SQLite format 3`.

## Next recommended increment

**Increment 3b — the audit hash-chain and append-only enforcement:**
`event_hash = BLAKE3(prev_event_hash || canonical_cbor(payload))` over
`audit_event`, a `verify` walk, and triggers rejecting UPDATE and DELETE on L0
tables. Plus the UI disclaimer required by ADR-0014, which must state that this
is tamper-evident within the app's trust boundary and **is not chain of
custody** — the disclaimer belongs in the UI, not only in the docs.

Then increment 4 (`kokin-blob`) has somewhere to record what it did.
