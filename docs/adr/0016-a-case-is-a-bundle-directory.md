# ADR-0016: A case is a bundle directory, not a single file

- **Status:** accepted — **revises ADR-0003**
- **Date:** 2026-08-12
- **Reversibility:** hard once cases exist in the field. Easy today: no case has
  ever been created outside a test.

## Context

ADR-0003 is titled "one case is one SQLite file", and it is already
self-contradictory. Its own consequences section says:

> Blobs do not live in the database. Large binary evidence goes to the
> content-addressed store (`kokin-blob`) with only its hash in SQLite.

So a case was never going to be one file. That went unnoticed because increment
1 stored nothing but a probe table.

Increment 3 forces the issue for a second, independent reason. ADR-0015 keys the
case database with the **case master key**, which exists only as ciphertext,
wrapped under a passphrase-derived KEK and a recovery key. To open a case you
must first read the salt, the KDF profile id, and the wrapped keys — but those
would live inside the very database the CMK is needed to decrypt.

Storing the header inside the encrypted database is a circular dependency. The
alternatives are:

**Key the database with the KEK instead.** Then the header can live inside,
because the passphrase alone opens it. But the recovery key could no longer open
anything — a key derived from the passphrase is, by definition, unreachable
without the passphrase. That deletes the recovery path, which is half the reason
ADR-0015 exists.

**Prefix the header to the database file.** SQLCipher's
`cipher_plaintext_header_size` reserves leading bytes, but they must remain a
valid SQLite header; it is not free space. A custom container would need a
custom VFS to give SQLite an offset view of the file. That is a lot of
error-prone machinery in the one part of the system where bugs mean unopenable
evidence.

**Make the case a directory.** The header is a plaintext file beside the
encrypted database.

## Decision

A case is a directory with a `.kokincase` extension:

```text
MyCase.kokincase/
  header.json      plaintext - format version, case id, salt,
                   KDF profile id, CMK wrapped under the KEK,
                   CMK wrapped under the recovery key
  case.db          SQLCipher, keyed by the raw CMK
  blobs/           content-addressed, per-blob envelope encryption
```

**The header contains no secrets.** A salt is not secret — it exists to stop
precomputation being amortised across cases. The wrapped keys are ciphertext
under AEAD. This is the same shape as LUKS and `age`: a plaintext header
describing how to unwrap a key, next to the data that key protects.

## Consequences

- **ADR-0003's "a case is a file you can copy, back up, and hand to someone"
  survives**, because copying a directory is not meaningfully harder. The claim
  it loses is that a case is a *single* file, which blobs had already cost us.
- **Export becomes more important, not less.** `kokin-export` produces a single
  verifiable package for handing a case to someone; the working format is
  optimised for the application, the export format for transport. Increment 11.
- Deleting a case means removing a directory. A partially deleted case must be
  detectable, so `header.json` carries a format version and the loader treats a
  missing or unparseable header as a named error rather than "not a case".
- On macOS this directory can later be marked a bundle so Finder presents it as
  one item. Cosmetic, and deliberately not done now — it must not be load-bearing
  for correctness, since Windows has no equivalent.
- The header is plaintext, so **the existence and creation time of a case, its
  format version, and its KDF parameters are visible to anyone with filesystem
  access**. The case *contents* are not. Plausible deniability about a case
  existing was never in the threat model, and this ADR makes that explicit rather
  than leaving it implied.
- `case.db` is keyed with the raw 32-byte CMK via `PRAGMA key = "x'...'"`, not a
  passphrase. That bypasses SQLCipher's own KDF entirely, which is correct: the
  KDF work is done once by Argon2id at 64 MiB (ADR-0015), which is far stronger
  than SQLCipher's default PBKDF2. `kdf_iter` becomes irrelevant for a raw key.

## Verification

Tests in `kokin-store` cover: create and reopen by passphrase; open by recovery
key; rotate the passphrase and confirm the old one fails while the case data is
untouched; a corrupt or missing header producing a named error rather than a
generic SQLite failure.
