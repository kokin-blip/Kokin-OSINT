# kokin-keys

Key hierarchy for encrypted cases: Argon2id derivation, case master key, KEK
wrapping, recovery keys.

Named `keys`, not `crypto`, so the name cannot be misread as cryptocurrency —
this crate is about key management for encrypted case files and nothing else.

```text
  passphrase ──Argon2id(salt, profile)──> KEK ──┐
                                                ├── wraps ──> CMK ──> SQLCipher
  recovery key ─────────────────────────────────┘
```

The case master key is random and never derived. It is the only key the storage
layer sees, and it is stored solely as ciphertext — wrapped independently under
a passphrase-derived KEK and under a recovery key. Changing the passphrase
rewraps the CMK and re-encrypts no case data.

A recovery key is thirty-two random bytes, and `phrase` is the form a person can
write on paper and type back: Crockford base32 in eight groups of seven, with a
24-bit checksum so a mistyped key is reported as mistyped rather than as a wrong
one. Those are different problems with different remedies.

See ADR-0015 for why, and `docs/threat-model/` for what this does not protect
against.
