# Increment 4 — content-addressed encrypted blob storage

Date: 2026-08-12 · Branch: `feat/phase0-foundations`

## Files changed

`crates/kokin-blob/src/lib.rs` (new, the whole increment) and its `Cargo.toml`.
No other crate consumes it yet — wiring it into the ingest pipeline is
increment 6.

## Behaviour added

Streaming `put` and `get` for large binary evidence, stored outside the case
database (ADR-0003) under `MyCase.kokincase/blobs/`.

- **Hash-on-write, one pass, bounded memory.** Plaintext is hashed with BLAKE3
  and encrypted in 64 KiB chunks as it is read. A 1 GiB blob costs one 64 KiB
  buffer, not 1 GiB of RAM — "evidence" includes disk images and video.
- **Per-blob keys**, each wrapped under the case master key. Destroying one
  wrapped key is the crypto-shred operation; hash, size, and lineage survive.
- **Chunked AEAD** (XChaCha20-Poly1305) with the chunk index as additional
  authenticated data, so chunks cannot be reordered or dropped undetected.
- **Size limit enforced during the read**, not after (attack catalogue A-011).

## The design decision worth arguing about

Blobs are addressed by `BLAKE3(plaintext)` — that is what makes deduplication
possible — but **that value is not used as the filename**. The stored name is
`BLAKE3_keyed(cmk, content_hash)`.

A content hash is derived from the bytes. If it were the filename, anyone with
filesystem access and a candidate file could hash it, look for the name, and
confirm whether that exact file is in the case. The case contents would still be
encrypted while the directory listing leaked membership — which for an OSINT
tool holding evidence about real people is close to the worst kind of leak,
because the question "is this person's photo in your case?" is often the whole
question.

Keying the name under the case master key defeats that: without the key the
filenames are indistinguishable from random, and two cases holding identical
evidence produce different names. Two tests pin this.

## Tests executed and results

```
cargo test --workspace --all-targets    46 passed, 0 failed
                                        (14 blob + 23 store + 9 keys)
cargo clippy --workspace --all-targets  0 warnings (-D warnings)
cargo fmt --all -- --check              clean
cargo deny check bans licenses sources advisories
                                        advisories ok, bans ok, licenses ok, sources ok
```

Fourteen blob tests. The ones carrying weight:

- multi-chunk round trip at 3 chunks + 1234 bytes, so the final short chunk and
  the nonce counter are exercised rather than only the single-chunk path
- **the on-disk name is not the content hash**, and **differs between cases**
- a modified blob fails authentication, reported at the chunk that failed
- **a truncated blob is caught by the content hash** — every surviving chunk
  still authenticates, so per-chunk AEAD alone would pass it. This is why the
  whole-content check is not redundant
- the size limit is enforced during the read, leaving no partial file behind
- destroying the wrapped key makes a blob unreadable while its ciphertext, and
  therefore the record that it existed, remains
- **a source returning one byte at a time still produces full chunks** — `Read`
  may legally return short, and reading once per chunk would have inflated AEAD
  overhead on any network-backed stream

## A real bug this increment found

The first version deduplicated silently: on a content-hash collision it
discarded the newly written ciphertext and returned a `BlobRef` carrying the
**new** per-blob key. That key cannot decrypt the **existing** stored file, so
the returned ref was unusable.

The original `identical_content_hashes_identically` test did not catch it,
because it only compared hashes and never read either blob back. Adding a test
that reads back after a dedupe hit failed immediately with
`Corrupt { chunk: 0 }`.

Fixed by making dedupe explicit: `put` returns `PutOutcome::Stored(BlobRef)` or
`PutOutcome::AlreadyPresent { content_hash, size }`, and the caller reuses the
ref it already holds for that content. That is the correct semantics anyway —
the point of dedupe is one set of bytes stored once and referenced by many
pieces of provenance.

The lesson is narrower than "write more tests": a round-trip test that never
completes the round trip is not a round-trip test.

## Performance measurements

Fourteen tests run in 0.17s including a ~200 KB multi-chunk blob. No throughput
benchmark yet; BLAKE3 and ChaCha20 are both fast enough that I/O will dominate,
but that is an expectation, not a measurement, and it should be measured before
any claim is made about ingest speed.

## Known limitations

1. **No archive-bomb limiter yet** (A-010). The per-blob size cap is in place,
   but nested-archive expansion limits — 100:1 ratio, 1 GB total, 10k entries,
   depth 3 — belong with the extraction path that unpacks archives, and that
   does not exist yet. A-010 remains `planned`.
2. **Deduplication saves a write, not necessarily disk.** It is per-case and
   per-content; there is no cross-case dedupe, deliberately, since that would
   mean a shared store and cross-case leakage.
3. **No integrity sweep.** Nothing yet walks the store to verify blobs against
   their hashes; export `verify` (increment 11) is where that belongs.
4. **`remove` is not a secure erase.** It unlinks the file. Real erasure is
   crypto-shredding — destroying the wrapped key — and the doc comment says so,
   because deleting the ciphertext while leaving the key would be false
   assurance given any backup.
5. Nothing consumes this crate yet.

## Manual verification

```
cargo test -p kokin-blob        # 14 tests
```

## Next recommended increment

**Increment 5 — `kokin-net`:** the egress broker. It is the largest single
security surface in the project (ADR-0010) and PoC P4's kill criterion — any
bypass halts connector work — so it should be built and attacked before
anything depends on it.
