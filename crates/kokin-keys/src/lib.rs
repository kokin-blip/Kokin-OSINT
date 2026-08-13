//! Key hierarchy for encrypted cases.
//!
//! Three keys, with deliberately different lifetimes:
//!
//! ```text
//!   passphrase ──Argon2id(salt, params)──> KEK ──┐
//!                                                ├── wraps ──> CMK ──> SQLCipher
//!   recovery key ────────────────────────────────┘
//! ```
//!
//! The **case master key** (CMK) is random, never derived, and is the only key
//! the storage layer ever sees. It is stored solely as ciphertext, wrapped
//! independently under a **key-encrypting key** (KEK) derived from the user's
//! passphrase, and under a **recovery key** the user writes down.
//!
//! That indirection buys the two operations a product needs and a
//! passphrase-as-key design cannot provide:
//!
//! - **Rotation** (change the passphrase) rewraps the CMK. Cheap, and it
//!   re-encrypts nothing, because the data key never changed.
//! - **Rekey** (change the CMK itself) requires re-encrypting the case. Rare,
//!   and reserved for a suspected key compromise.
//!
//! Increment 1 fed a passphrase straight into SQLCipher's `PRAGMA key`, which
//! conflated the two: every passphrase change would have been a full rekey, and
//! a forgotten passphrase would have been unrecoverable by construction.
//!
//! ## What this module does not do
//!
//! It does not protect against an attacker who can read this process's memory
//! while a case is open. Keys are zeroized on drop, which limits how long they
//! linger, but a live process holding a decrypted CMK is inherent to letting the
//! user read their own case. See the threat model, boundary B1.

#![doc = include_str!("../README.md")]

pub mod phrase;

pub use phrase::{from_phrase, to_phrase};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length of every symmetric key in this module.
pub const KEY_LEN: usize = 32;
/// Length of the per-case Argon2id salt.
pub const SALT_LEN: usize = 16;
/// XChaCha20-Poly1305 nonce length. The extended nonce is why this AEAD was
/// chosen: 192 bits is wide enough that random nonces do not need a counter.
pub const NONCE_LEN: usize = 24;

/// Errors that can arise while deriving or unwrapping keys.
///
/// `WrongKey` deliberately does not distinguish "bad passphrase" from "damaged
/// ciphertext". An AEAD failure cannot tell them apart, and inventing a
/// distinction would either be a guess or a padding-oracle-shaped hint.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("wrong passphrase or recovery key, or the wrapped key is damaged")]
    WrongKey,
    #[error("key derivation failed: {0}")]
    Derivation(String),
    #[error("the operating system random number generator failed: {0}")]
    Random(String),
    #[error("expected {expected} bytes, found {found}")]
    BadLength { expected: usize, found: usize },
    #[error("that is not a recovery key: check for a mistyped or missing character")]
    MistypedRecoveryKey,
    #[error("unsupported key-derivation profile: {0}")]
    UnsupportedProfile(u8),
}

type Result<T> = std::result::Result<T, KeyError>;

/// Argon2id cost parameters, stored in the case header so a case remains
/// openable after the defaults change.
///
/// This struct existing at all is the point. If parameters were compiled in,
/// raising them for new cases would silently make every existing case
/// underivable, and the failure would look exactly like a wrong passphrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfProfile {
    /// Stored in the header so the header can be parsed without guessing.
    pub id: u8,
    /// Memory cost, in kibibytes.
    pub m_cost_kib: u32,
    /// Iterations.
    pub t_cost: u32,
    /// Parallelism lanes.
    pub p_cost: u32,
}

impl KdfProfile {
    /// The current default: 64 MiB, 3 passes, 1 lane.
    ///
    /// Above OWASP's 19 MiB floor for Argon2id, because this is a desktop
    /// application protecting evidence about real people rather than a server
    /// authenticating thousands of users per second. The cost is paid once when
    /// a case is opened, where a few hundred milliseconds is invisible.
    ///
    /// One lane, not four: parallelism helps a server amortise across cores,
    /// and mainly helps an attacker with a GPU here. Memory hardness is the
    /// property doing the work.
    pub const V1: Self = Self {
        id: 1,
        m_cost_kib: 65_536,
        t_cost: 3,
        p_cost: 1,
    };

    /// Resolve a profile id read from a case header.
    pub fn from_id(id: u8) -> Result<Self> {
        match id {
            1 => Ok(Self::V1),
            other => Err(KeyError::UnsupportedProfile(other)),
        }
    }

    fn to_argon2(self) -> Result<Argon2<'static>> {
        let params = Params::new(self.m_cost_kib, self.t_cost, self.p_cost, Some(KEY_LEN))
            .map_err(|e| KeyError::Derivation(e.to_string()))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

/// A 32-byte secret that clears itself when dropped.
///
/// One type for every key in the hierarchy, with the role expressed by the
/// wrapper types below, so no code path can hold key material that is not
/// zeroized.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Secret([u8; KEY_LEN]);

/// Constant-time equality.
///
/// Not derived: the derived `PartialEq` short-circuits on the first differing
/// byte, which leaks how much of a guessed key was correct through timing.
/// Currently only tests compare keys, but a key type that is unsafe to compare
/// is a trap waiting for the first caller who does.
impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        use subtle::ConstantTimeEq;
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for Secret {}

impl Secret {
    /// Generate a fresh secret from the operating system CSPRNG.
    pub fn generate() -> Result<Self> {
        let mut bytes = [0u8; KEY_LEN];
        getrandom::fill(&mut bytes).map_err(|e| KeyError::Random(e.to_string()))?;
        Ok(Self(bytes))
    }

    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let arr: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyError::BadLength {
            expected: KEY_LEN,
            found: bytes.len(),
        })?;
        Ok(Self(arr))
    }

    pub fn expose(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Lowercase hex, for handing to SQLCipher's raw-key pragma.
    ///
    /// Returns a `Zeroizing<String>` rather than a `String`, so the hex form
    /// cannot outlive its use and leave a copy of the key on the heap.
    pub fn to_hex(&self) -> zeroize::Zeroizing<String> {
        let mut s = String::with_capacity(KEY_LEN * 2);
        for byte in self.0 {
            use std::fmt::Write as _;
            // Writing to a String cannot fail; the Result is discarded rather
            // than unwrapped so this stays panic-free under the workspace lint.
            let _ = write!(s, "{byte:02x}");
        }
        zeroize::Zeroizing::new(s)
    }
}

/// Deliberately opaque: a key must never reach a log line.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Derived from the passphrase. Encrypts the CMK; never encrypts case data.
pub type Kek = Secret;
/// Encrypts case data. Random, never derived from anything the user knows.
pub type CaseMasterKey = Secret;
/// A second independent way to unwrap the CMK, for a forgotten passphrase.
pub type RecoveryKey = Secret;

/// A CMK encrypted under some wrapping key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedKey {
    pub nonce: [u8; NONCE_LEN],
    pub ciphertext: Vec<u8>,
}

/// Derive the key-encrypting key from a passphrase.
///
/// The salt must be unique per case and is stored in the case header. It is not
/// secret; it exists so two cases with the same passphrase do not share a KEK,
/// and so precomputation cannot be amortised across users.
pub fn derive_kek(passphrase: &str, salt: &[u8; SALT_LEN], profile: KdfProfile) -> Result<Kek> {
    let argon2 = profile.to_argon2()?;
    let mut out = [0u8; KEY_LEN];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| KeyError::Derivation(e.to_string()))?;
    Ok(Secret(out))
}

/// Generate a fresh per-case salt.
pub fn generate_salt() -> Result<[u8; SALT_LEN]> {
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|e| KeyError::Random(e.to_string()))?;
    Ok(salt)
}

/// Wrap the case master key under a KEK or recovery key.
///
/// `aad` is authenticated but not encrypted, and callers must pass the case
/// identifier plus the KDF profile id. This binds the ciphertext to its
/// context: a wrapped key lifted from one case header into another, or replayed
/// against different KDF parameters, fails to authenticate instead of silently
/// producing a key that decrypts nothing.
pub fn wrap_cmk(cmk: &CaseMasterKey, wrapping_key: &Secret, aad: &[u8]) -> Result<WrappedKey> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*wrapping_key.expose()));

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes).map_err(|e| KeyError::Random(e.to_string()))?;

    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce_bytes),
            Payload {
                msg: cmk.expose(),
                aad,
            },
        )
        // An AEAD encrypt failure here is not attacker-influenced; it would
        // mean an internal invariant broke. Report it rather than panicking.
        .map_err(|e| KeyError::Derivation(e.to_string()))?;

    Ok(WrappedKey {
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Recover the case master key. Fails if the key is wrong, the ciphertext was
/// modified, or the `aad` does not match what it was wrapped with.
pub fn unwrap_cmk(
    wrapped: &WrappedKey,
    wrapping_key: &Secret,
    aad: &[u8],
) -> Result<CaseMasterKey> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*wrapping_key.expose()));

    let mut plaintext = cipher
        .decrypt(
            &XNonce::from(wrapped.nonce),
            Payload {
                msg: &wrapped.ciphertext,
                aad,
            },
        )
        .map_err(|_| KeyError::WrongKey)?;

    let secret = Secret::from_slice(&plaintext);
    // The intermediate Vec is not zeroize-aware, so clear it explicitly rather
    // than leaving a copy of the CMK on the heap for the allocator to reuse.
    plaintext.zeroize();
    secret
}

/// Change the passphrase without re-encrypting the case.
///
/// The CMK is unwrapped with the old KEK and rewrapped with the new one; case
/// data is untouched. A fresh salt is generated, because reusing the old salt
/// with a new passphrase would leak that the two derivations are related.
pub fn rotate_passphrase(
    wrapped: &WrappedKey,
    old_kek: &Kek,
    new_kek: &Kek,
    aad: &[u8],
) -> Result<WrappedKey> {
    let cmk = unwrap_cmk(wrapped, old_kek, aad)?;
    wrap_cmk(&cmk, new_kek, aad)
}

#[cfg(test)]
// Tests are the one place a panic is the correct response to an unexpected
// value: a failed assertion should stop the test, not be handled.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// A cheap profile, so the test suite is not dominated by KDF work.
    /// Never use this for a real case.
    const TEST_PROFILE: KdfProfile = KdfProfile {
        id: 1,
        m_cost_kib: 1024,
        t_cost: 1,
        p_cost: 1,
    };

    const AAD: &[u8] = b"case:01J0000000000000000000000000|kdf:1";

    #[test]
    fn round_trip_recovers_the_master_key() {
        let salt = generate_salt().unwrap();
        let kek = derive_kek("correct horse battery staple", &salt, TEST_PROFILE).unwrap();
        let cmk = CaseMasterKey::generate().unwrap();

        let wrapped = wrap_cmk(&cmk, &kek, AAD).unwrap();
        let recovered = unwrap_cmk(&wrapped, &kek, AAD).unwrap();

        assert_eq!(cmk.expose(), recovered.expose());
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let salt = generate_salt().unwrap();
        let kek = derive_kek("right", &salt, TEST_PROFILE).unwrap();
        let wrong = derive_kek("wrong", &salt, TEST_PROFILE).unwrap();
        let cmk = CaseMasterKey::generate().unwrap();
        let wrapped = wrap_cmk(&cmk, &kek, AAD).unwrap();

        assert_eq!(unwrap_cmk(&wrapped, &wrong, AAD), Err(KeyError::WrongKey));
    }

    /// The reason `aad` is a required parameter rather than an optional extra.
    #[test]
    fn a_wrapped_key_cannot_be_transplanted_into_another_case() {
        let salt = generate_salt().unwrap();
        let kek = derive_kek("passphrase", &salt, TEST_PROFILE).unwrap();
        let cmk = CaseMasterKey::generate().unwrap();
        let wrapped = wrap_cmk(&cmk, &kek, b"case:AAAA|kdf:1").unwrap();

        // Same passphrase, same KEK, different case identifier.
        assert_eq!(
            unwrap_cmk(&wrapped, &kek, b"case:BBBB|kdf:1"),
            Err(KeyError::WrongKey)
        );
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let salt = generate_salt().unwrap();
        let kek = derive_kek("passphrase", &salt, TEST_PROFILE).unwrap();
        let cmk = CaseMasterKey::generate().unwrap();
        let mut wrapped = wrap_cmk(&cmk, &kek, AAD).unwrap();

        wrapped.ciphertext[0] ^= 0x01;

        assert_eq!(unwrap_cmk(&wrapped, &kek, AAD), Err(KeyError::WrongKey));
    }

    #[test]
    fn a_recovery_key_unwraps_the_same_master_key() {
        let salt = generate_salt().unwrap();
        let kek = derive_kek("passphrase", &salt, TEST_PROFILE).unwrap();
        let recovery = RecoveryKey::generate().unwrap();
        let cmk = CaseMasterKey::generate().unwrap();

        let by_passphrase = wrap_cmk(&cmk, &kek, AAD).unwrap();
        let by_recovery = wrap_cmk(&cmk, &recovery, AAD).unwrap();

        // Two independent paths to the same data key - which is the whole
        // reason the CMK is not derived from the passphrase.
        assert_eq!(
            unwrap_cmk(&by_passphrase, &kek, AAD).unwrap().expose(),
            unwrap_cmk(&by_recovery, &recovery, AAD).unwrap().expose(),
        );
    }

    #[test]
    fn rotating_the_passphrase_does_not_change_the_master_key() {
        let salt = generate_salt().unwrap();
        let old = derive_kek("old passphrase", &salt, TEST_PROFILE).unwrap();
        let new_salt = generate_salt().unwrap();
        let new = derive_kek("new passphrase", &new_salt, TEST_PROFILE).unwrap();

        let cmk = CaseMasterKey::generate().unwrap();
        let wrapped = wrap_cmk(&cmk, &old, AAD).unwrap();
        let rewrapped = rotate_passphrase(&wrapped, &old, &new, AAD).unwrap();

        // The data key is unchanged, so no case data needs re-encrypting.
        assert_eq!(
            unwrap_cmk(&rewrapped, &new, AAD).unwrap().expose(),
            cmk.expose()
        );
        // And the old passphrase no longer opens the rewrapped copy.
        assert_eq!(unwrap_cmk(&rewrapped, &old, AAD), Err(KeyError::WrongKey));
    }

    /// Known-answer test. This is the load-bearing test of the module.
    ///
    /// It pins the exact bytes our derivation produces for fixed inputs. If
    /// anyone changes the algorithm, the version, the profile constants, or the
    /// output length, existing cases stop opening - and the symptom is
    /// indistinguishable from a wrong passphrase, which is close to
    /// undiagnosable in the field. This test turns that into a build failure.
    ///
    /// The expected value is **independently verified**. It was cross-checked
    /// on 2026-08-12 against `argon2-cffi` 25.1.0, which binds the reference C
    /// implementation (phc-winner-argon2), using the same password, salt,
    /// m=1024, t=1, p=1, len=32, Argon2id, version 0x13. Both produced
    /// `48efc4f3…c67bd4` byte for byte. So this pins conformance to Argon2id as
    /// specified, not merely to whatever the `argon2` crate happens to do.
    #[test]
    fn kdf_known_answer() {
        let salt = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let profile = KdfProfile {
            id: 1,
            m_cost_kib: 1024,
            t_cost: 1,
            p_cost: 1,
        };
        let kek = derive_kek("kokin known answer", &salt, profile).unwrap();

        assert_eq!(
            kek.to_hex().as_str(),
            KAT_EXPECTED,
            "the key-derivation output changed - every existing case would fail \
             to open, and the failure would look exactly like a wrong passphrase"
        );
    }

    /// Argon2id, v0x13, m=1024 KiB, t=1, p=1, len=32, password
    /// `kokin known answer`, salt `000102…0f`. Cross-checked against the
    /// reference C implementation; see `kdf_known_answer`.
    const KAT_EXPECTED: &str = "48efc4f38557058bccb7e4a1709d05a54d4bd2edae4156954863af7bebc67bd4";

    #[test]
    fn profile_ids_round_trip_and_unknown_ids_are_rejected() {
        assert_eq!(KdfProfile::from_id(1).unwrap(), KdfProfile::V1);
        assert_eq!(
            KdfProfile::from_id(99),
            Err(KeyError::UnsupportedProfile(99))
        );
    }

    #[test]
    fn debug_never_prints_key_material() {
        let secret = Secret::from_bytes([0xAB; KEY_LEN]);
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, "Secret(<redacted>)");
        assert!(!rendered.contains("ab"));
    }
}
