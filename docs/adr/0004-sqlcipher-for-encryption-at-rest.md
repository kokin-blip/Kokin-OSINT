# ADR-0004: SQLCipher for the case database; envelope encryption for blobs

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard for the database format (existing cases would need a
  migration path); easy for the blob scheme.

## Context

The privacy architecture requires encryption for case databases, caches,
exports, and backups. "Encrypted" must mean the file on disk yields nothing
useful to someone who steals the laptop — including through the search index,
which in a naive design is exactly where the plaintext ends up.

## Alternatives considered

**Application-level field encryption.** Encrypt sensitive column values before
insert. Leaves the FTS index in plaintext — meaning the search index becomes a
complete plaintext copy of the evidence. Fatal. Also breaks range queries and
ordering.

**OS-level encryption (BitLocker / FileVault).** Free, transparent, and does
nothing for our threat model: the disk is decrypted whenever the user is logged
in, which is precisely when the case file is at risk.

**SQLCipher.** Page-level AES-256-CBC with HMAC over the entire database file,
including indexes, FTS content, temporary tables, and the WAL. Mature, widely
audited, and the de-facto standard for encrypted SQLite.

## Decision

SQLCipher for the case database, via `rusqlite`'s
`bundled-sqlcipher-vendored-openssl` feature. Blobs are encrypted individually
under per-blob keys wrapped by the case key, which is what makes cryptographic
erasure of a single piece of evidence possible without rewriting the database
(see the redaction/deletion model in `docs/data-model/`).

Page-level encryption covering the FTS index "for free" is the deciding
property. It is the difference between an encrypted database and an encrypted
database with a plaintext index sitting next to it.

Configuration is pinned explicitly (`cipher_page_size = 4096`,
`kdf_iter = 256000`) rather than left at the library default, so that a future
SQLCipher upgrade cannot silently change the on-disk format and strand existing
cases.

## Verification

Three tests in `kokin-store` enforce this, and they run on both platforms in CI:

1. An encrypted case survives close and reopen.
2. A wrong passphrase produces a *named* error, distinguishable from corruption.
3. **The file on disk contains no plaintext and does not carry the
   `SQLite format 3\0` header.** This is the test that catches a build which
   silently linked plain SQLite — a failure mode that every functional test
   would otherwise pass.

The application also exposes the linked cipher backend in its UI, so a
mis-built binary is visible to the user and not only to CI.

## Consequences

- **Building on Windows requires a native Windows perl** (Strawberry Perl or
  ActivePerl) because vendored OpenSSL's `Configure` is a Perl script that
  explicitly rejects cygwin perls for MSVC targets. Verified 2026-08-12: both
  msys64 perl and Git-for-Windows perl fail — msys64 with
  "This perl implementation doesn't produce Windows like paths", Git's with a
  missing `Locale::Maketext::Simple`. This is a documented prerequisite in the
  README, and CI verifies perl is present before building so the failure is
  legible rather than appearing as a C compiler error.
- Build times are long on a cold cache; OpenSSL is compiled from source.
- The passphrase must be held in memory while a case is open. Memory hygiene
  (zeroization, auto-lock) is `kokin-crypto`'s problem, in increment 2.
- Deletion semantics rest on this: destroying a wrapped blob key is genuine
  cryptographic erasure, but it also makes that blob's integrity permanently
  unverifiable. Stated honestly in `docs/limitations/deletion.md`.
