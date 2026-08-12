//! The case header: the plaintext file that says how to unwrap a case key.
//!
//! It sits beside the encrypted database rather than inside it, because opening
//! the database needs the case master key and the CMK is what the header
//! describes. ADR-0016 records why the alternatives were rejected.
//!
//! **This file contains no secrets.** The salt is not secret — it exists so
//! precomputation cannot be amortised across cases. The wrapped keys are AEAD
//! ciphertext. This is the same shape as a LUKS or `age` header.

use std::path::{Path, PathBuf};

use kokin_keys::{
    derive_kek, generate_salt, unwrap_cmk, wrap_cmk, CaseMasterKey, KdfProfile, RecoveryKey,
    WrappedKey, KEY_LEN, NONCE_LEN, SALT_LEN,
};
use serde::{Deserialize, Serialize};

use crate::{Result, StoreError};

/// Bumped only for a change that older builds cannot read. The loader refuses
/// an unknown version by name, so a case from a newer build reports that
/// clearly instead of looking corrupt.
pub const HEADER_FORMAT_VERSION: u32 = 1;

pub const HEADER_FILENAME: &str = "header.json";
pub const DATABASE_FILENAME: &str = "case.db";
pub const BLOBS_DIRNAME: &str = "blobs";

/// A wrapped key as it appears on disk.
///
/// Hex rather than base64 so the header stays diffable and eyeball-checkable;
/// these are small and the size difference is irrelevant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WrappedKeyRecord {
    pub nonce_hex: String,
    pub ciphertext_hex: String,
}

impl WrappedKeyRecord {
    fn from_wrapped(w: &WrappedKey) -> Self {
        Self {
            nonce_hex: to_hex(&w.nonce),
            ciphertext_hex: to_hex(&w.ciphertext),
        }
    }

    fn to_wrapped(&self) -> Result<WrappedKey> {
        let nonce_bytes = from_hex(&self.nonce_hex)?;
        let nonce: [u8; NONCE_LEN] = nonce_bytes
            .as_slice()
            .try_into()
            .map_err(|_| StoreError::MalformedHeader("nonce is the wrong length".into()))?;
        Ok(WrappedKey {
            nonce,
            ciphertext: from_hex(&self.ciphertext_hex)?,
        })
    }
}

/// The on-disk header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaseHeader {
    pub format_version: u32,
    /// Stable identifier for this case. Also bound into the AEAD aad, so a
    /// wrapped key cannot be transplanted into a different case.
    pub case_id: String,
    pub created_utc: String,
    pub kdf_profile_id: u8,
    pub salt_hex: String,
    /// The CMK, wrapped under the passphrase-derived KEK.
    pub cmk_wrapped_by_passphrase: WrappedKeyRecord,
    /// The CMK, wrapped under the recovery key. Absent only if the user
    /// explicitly declined one, which the UI must make deliberate and loud.
    pub cmk_wrapped_by_recovery: Option<WrappedKeyRecord>,
}

impl CaseHeader {
    /// Bytes bound into every wrap for this case.
    ///
    /// Includes the KDF profile id as well as the case id, so a header edited
    /// to claim different KDF parameters fails to authenticate rather than
    /// silently deriving a key that decrypts nothing.
    fn aad(&self) -> Vec<u8> {
        format!("kokin:case:{}|kdf:{}", self.case_id, self.kdf_profile_id).into_bytes()
    }

    pub fn salt(&self) -> Result<[u8; SALT_LEN]> {
        from_hex(&self.salt_hex)?
            .as_slice()
            .try_into()
            .map_err(|_| StoreError::MalformedHeader("salt is the wrong length".into()))
    }

    pub fn profile(&self) -> Result<KdfProfile> {
        Ok(KdfProfile::from_id(self.kdf_profile_id)?)
    }
}

/// A case on disk: a directory holding the header, the database, and blobs.
#[derive(Debug, Clone)]
pub struct CasePaths {
    pub root: PathBuf,
}

impl CasePaths {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn header(&self) -> PathBuf {
        self.root.join(HEADER_FILENAME)
    }

    pub fn database(&self) -> PathBuf {
        self.root.join(DATABASE_FILENAME)
    }

    pub fn blobs(&self) -> PathBuf {
        self.root.join(BLOBS_DIRNAME)
    }
}

/// What `create_case` hands back, once.
///
/// The recovery key is returned by value and never persisted in usable form —
/// only its wrap of the CMK is stored. If the user does not record it here,
/// it is gone, and the UI must say so before this value is dropped.
pub struct NewCase {
    pub header: CaseHeader,
    pub cmk: CaseMasterKey,
    pub recovery_key: RecoveryKey,
}

/// Build a header for a new case, generating the salt, CMK, and recovery key.
pub fn create_header(case_id: &str, passphrase: &str, profile: KdfProfile) -> Result<NewCase> {
    let salt = generate_salt()?;
    let cmk = CaseMasterKey::generate()?;
    let recovery_key = RecoveryKey::generate()?;

    // Built first with placeholder wraps so `aad()` has one definition rather
    // than the format string being repeated at each call site.
    let mut header = CaseHeader {
        format_version: HEADER_FORMAT_VERSION,
        case_id: case_id.to_string(),
        created_utc: now_utc_rfc3339(),
        kdf_profile_id: profile.id,
        salt_hex: to_hex(&salt),
        cmk_wrapped_by_passphrase: WrappedKeyRecord {
            nonce_hex: String::new(),
            ciphertext_hex: String::new(),
        },
        cmk_wrapped_by_recovery: None,
    };
    let aad = header.aad();

    let kek = derive_kek(passphrase, &salt, profile)?;
    header.cmk_wrapped_by_passphrase = WrappedKeyRecord::from_wrapped(&wrap_cmk(&cmk, &kek, &aad)?);
    header.cmk_wrapped_by_recovery = Some(WrappedKeyRecord::from_wrapped(&wrap_cmk(
        &cmk,
        &recovery_key,
        &aad,
    )?));

    Ok(NewCase {
        header,
        cmk,
        recovery_key,
    })
}

/// Recover the case master key from a passphrase.
pub fn unlock_with_passphrase(header: &CaseHeader, passphrase: &str) -> Result<CaseMasterKey> {
    let kek = derive_kek(passphrase, &header.salt()?, header.profile()?)?;
    Ok(unwrap_cmk(
        &header.cmk_wrapped_by_passphrase.to_wrapped()?,
        &kek,
        &header.aad(),
    )?)
}

/// Recover the case master key from the recovery key.
pub fn unlock_with_recovery_key(
    header: &CaseHeader,
    recovery_key: &RecoveryKey,
) -> Result<CaseMasterKey> {
    let record = header
        .cmk_wrapped_by_recovery
        .as_ref()
        .ok_or(StoreError::NoRecoveryKey)?;
    Ok(unwrap_cmk(
        &record.to_wrapped()?,
        recovery_key,
        &header.aad(),
    )?)
}

/// Change the passphrase without touching case data.
///
/// Only the header changes: a fresh salt, a new KEK, and a rewrapped CMK. The
/// database is not re-encrypted because the key that encrypts it did not
/// change — the property ADR-0015 exists for.
///
/// A new salt is generated rather than reused, so the old and new wraps are not
/// derived from related material.
pub fn rotate_passphrase(
    header: &CaseHeader,
    old_passphrase: &str,
    new_passphrase: &str,
) -> Result<CaseHeader> {
    let cmk = unlock_with_passphrase(header, old_passphrase)?;
    let profile = header.profile()?;

    let new_salt = generate_salt()?;
    let mut updated = header.clone();
    updated.salt_hex = to_hex(&new_salt);

    // aad is unchanged: it binds case id and profile id, neither of which moved.
    let aad = updated.aad();
    let new_kek = derive_kek(new_passphrase, &new_salt, profile)?;
    updated.cmk_wrapped_by_passphrase =
        WrappedKeyRecord::from_wrapped(&wrap_cmk(&cmk, &new_kek, &aad)?);

    // The recovery wrap is deliberately left alone: rotating a passphrase must
    // not silently invalidate a recovery key the user wrote down months ago.
    Ok(updated)
}

pub fn write_header(paths: &CasePaths, header: &CaseHeader) -> Result<()> {
    let json = serde_json::to_string_pretty(header)
        .map_err(|e| StoreError::MalformedHeader(e.to_string()))?;
    std::fs::write(paths.header(), json)?;
    Ok(())
}

pub fn read_header(paths: &CasePaths) -> Result<CaseHeader> {
    let path = paths.header();
    if !path.exists() {
        return Err(StoreError::NotACase(paths.root.display().to_string()));
    }
    let text = std::fs::read_to_string(&path)?;
    let header: CaseHeader =
        serde_json::from_str(&text).map_err(|e| StoreError::MalformedHeader(e.to_string()))?;

    if header.format_version != HEADER_FORMAT_VERSION {
        return Err(StoreError::UnsupportedHeaderVersion {
            found: header.format_version,
            supported: HEADER_FORMAT_VERSION,
        });
    }
    Ok(header)
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Writing to a String cannot fail.
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn from_hex(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        return Err(StoreError::MalformedHeader(
            "hex field has an odd length".into(),
        ));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| StoreError::MalformedHeader("hex field is not hexadecimal".into()))
        })
        .collect()
}

/// RFC 3339 timestamp without pulling in a date library.
///
/// `chrono` or `time` would be a dependency carried solely for a display string
/// in one file. Ordering and arithmetic on case timestamps are not needed here;
/// when they are, that is the moment to take the dependency deliberately.
fn now_utc_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_unix(secs as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Howard Hinnant's days-from-civil algorithm, inverted. Public domain.
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };

    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Render the CMK for SQLCipher's raw-key pragma.
///
/// `x'...'` tells SQLCipher the value is the key itself, bypassing its own KDF.
/// That is correct here: Argon2id at 64 MiB has already done far more work than
/// SQLCipher's default PBKDF2 would (ADR-0015, ADR-0016).
///
/// The surrounding double quotes are required and are not ordinary SQL blob
/// syntax — SQLCipher parses the pragma argument as a string and then inspects
/// it for the `x'...'` form. Without them the pragma is a syntax error, which
/// is how this was found.
pub fn raw_key_literal(cmk: &CaseMasterKey) -> zeroize::Zeroizing<String> {
    let hex = cmk.to_hex();
    zeroize::Zeroizing::new(format!("\"x'{}'\"", hex.as_str()))
}

/// Compile-time assurance the key length matches what the pragma expects.
const _: () = assert!(KEY_LEN == 32);
