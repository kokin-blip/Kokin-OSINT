# ADR-0015: A wrapped case master key, not a passphrase-derived data key

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard once cases exist. Changing the hierarchy means
  rewrapping every case header, and changing the AEAD means a format migration.

## Context

Increment 1 fed the user's passphrase directly into SQLCipher's `PRAGMA key` to
prove the round-trip worked. That is correct as a proof and wrong as a product,
because it makes the passphrase *be* the data key. Two consequences follow
immediately:

- Changing the passphrase means re-encrypting the entire case, since the key
  that encrypts the data has changed.
- A forgotten passphrase is unrecoverable **by construction** — there is no
  second door, and no way to add one later without re-encrypting.

## Decision

Three keys with different lifetimes:

| Key | Origin | Encrypts | Lifetime |
|---|---|---|---|
| **KEK** | Argon2id(passphrase, per-case salt, profile) | the CMK | changes with the passphrase |
| **CMK** | OS CSPRNG | case data, via SQLCipher | effectively permanent |
| **Recovery key** | OS CSPRNG | the CMK | written down by the user |

The CMK exists only as ciphertext at rest, wrapped independently under the KEK
and under the recovery key. Two doors to the same key, neither derived from the
other.

This separates **rotation** (rewrap the CMK — cheap, re-encrypts nothing) from
**rekey** (generate a new CMK — requires re-encrypting the case, reserved for
suspected key compromise). Conflating those two is the defect being fixed.

## Choices worth recording

**Argon2id, not scrypt or PBKDF2.** Argon2id is the current recommendation and
resists both GPU and side-channel attack. PBKDF2 is not memory-hard, which is
the property that matters against commodity cracking hardware.

**Profile V1: m=64 MiB, t=3, p=1.** Above OWASP's 19 MiB floor, because this is
a desktop application protecting evidence about real people, not a server
authenticating thousands of users per second — the cost is paid once per case
open, where a few hundred milliseconds is invisible. `p=1` rather than 4:
parallelism mainly helps a server amortise across cores, and mainly helps an
attacker with a GPU here. Memory hardness does the work.

**Parameters live in the case header, not in the binary.** This is the detail
that is easy to skip and expensive to retrofit. If parameters were compiled in,
raising them for new cases would make every existing case underivable, and the
symptom would be indistinguishable from a wrong passphrase — close to
undiagnosable in the field. `KdfProfile::from_id` resolves the stored id, and an
unknown id is a *named* error rather than a derivation failure.

**XChaCha20-Poly1305 for wrapping.** The 192-bit extended nonce is wide enough
that random nonces need no counter state, which removes a class of nonce-reuse
bugs outright. AES-GCM would require nonce bookkeeping in the header for no
benefit here.

**AAD binds the wrap to its context.** Callers pass the case identifier and KDF
profile id as additional authenticated data, so a wrapped key lifted from one
case header into another — or replayed against different parameters — fails to
authenticate instead of silently producing a key that decrypts nothing.

**Constant-time comparison.** `Secret` implements `PartialEq` via `subtle`
rather than deriving it. The derived version short-circuits on the first
differing byte, leaking how much of a guessed key was correct. Only tests
compare keys today, but a key type that is unsafe to compare is a trap for the
first caller who does.

## Verification

Nine tests in `kokin-keys`, all offline and fast. Beyond round-trip and
wrong-key, three carry specific weight:

- `a_wrapped_key_cannot_be_transplanted_into_another_case` — the reason AAD is a
  required parameter and not an optional extra.
- `rotating_the_passphrase_does_not_change_the_master_key` — asserts the
  property this whole ADR exists for.
- `kdf_known_answer` — pins the exact derivation output. **Cross-checked on
  2026-08-12 against `argon2-cffi` 25.1.0, which binds the reference C
  implementation (phc-winner-argon2); both produced `48efc4f3…c67bd4` byte for
  byte.** So it pins conformance to Argon2id as specified, not merely to
  whatever the `argon2` crate happens to do — which is the difference between a
  known-answer test and a self-referential snapshot.

## Consequences

- Opening a case costs one Argon2id derivation at 64 MiB. **Measured at
  114.2 ms** (mean of 5, release build, Ryzen 5 development machine, via
  `cargo run -p kokin-keys --example kdf_timing --release`). That is inside the
  latency budget this profile was chosen against, so V1 stands. The example is
  kept in the crate so the trade-off can be re-measured rather than re-argued.
- A user who loses both passphrase and recovery key has lost the case. That is
  the correct outcome for a local-first tool with no escrow, and the UI must say
  so *before* the recovery key is dismissed, not after.
- The human-facing encoding of the recovery key (BIP39 words or Crockford
  base32, both far more transcribable than hex) is a UX decision deferred to the
  increment that builds the case-creation flow. The key itself is 32 bytes
  regardless, so the encoding is not a format decision.
- **Not addressed:** an attacker who can read this process's memory while a case
  is open. Keys are zeroized on drop, limiting how long they linger, but a live
  process holding a decrypted CMK is inherent to letting the user read their own
  case. Threat model boundary B1; strength of the adversary model is D-008,
  still open.
