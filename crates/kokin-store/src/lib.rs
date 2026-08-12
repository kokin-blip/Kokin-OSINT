//! Encrypted case storage.
//!
//! A case is a bundle directory (ADR-0016):
//!
//! ```text
//! MyCase.kokincase/
//!   header.json    plaintext: salt, KDF profile, wrapped case master key
//!   case.db        SQLCipher, keyed by the raw CMK
//!   blobs/         content-addressed evidence (kokin-blob, increment 4)
//! ```
//!
//! The database is keyed with the **case master key**, not the passphrase, so
//! changing a passphrase rewrites only `header.json` and re-encrypts nothing.
//! Page-level encryption covers the FTS5 index for free — see ADR-0004 for why
//! application-level field encryption was rejected.

pub mod audit;
pub mod header;
pub mod migrations;

use std::path::Path;

use rusqlite::Connection;

pub use audit::{append as append_audit_event, verify_chain, ChainStatus, NewAuditEvent};
pub use header::{
    create_header, read_header, rotate_passphrase, unlock_with_passphrase,
    unlock_with_recovery_key, write_header, CaseHeader, CasePaths, NewCase,
};
pub use kokin_keys::{CaseMasterKey, KdfProfile, RecoveryKey};

/// Errors this crate can produce.
///
/// The distinctions here are the ones a user-facing message depends on. "Wrong
/// passphrase" must not imply data loss, and "this directory is not a case"
/// must not look like corruption.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("key error: {0}")]
    Key(#[from] kokin_keys::KeyError),

    #[error("could not decrypt the case: wrong passphrase, or the file is not a Kokin case")]
    WrongPassphraseOrNotACase,

    #[error("{0} is not a Kokin case (no header.json)")]
    NotACase(String),

    #[error("the case header is damaged or not valid JSON: {0}")]
    MalformedHeader(String),

    #[error("case header format version {found} is not supported (this build reads version {supported})")]
    UnsupportedHeaderVersion { found: u32, supported: u32 },

    #[error("this case has no recovery key")]
    NoRecoveryKey,

    #[error("case file is encrypted with an unsupported cipher configuration")]
    UnsupportedCipherConfig,

    #[error("case schema version {found} is newer than this build supports ({supported})")]
    SchemaFromTheFuture { found: i64, supported: i64 },
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// The SQLCipher configuration a case database is written with.
///
/// Pinned explicitly rather than relying on the library default, so a future
/// SQLCipher upgrade cannot silently change the on-disk format and strand
/// existing cases. Any change here requires a migration path.
const CIPHER_PAGE_SIZE: i64 = 4096;

/// Create a new case bundle.
///
/// Returns the recovery key **once**. It is never stored in usable form — only
/// its wrap of the CMK is — so if the caller does not surface it to the user
/// here, it is gone. The UI must make that consequence clear before this value
/// is dropped.
pub fn create_case(
    root: impl AsRef<Path>,
    case_id: &str,
    passphrase: &str,
) -> Result<(Connection, CasePaths, RecoveryKey)> {
    let paths = CasePaths::new(root);

    if paths.header().exists() {
        return Err(StoreError::MalformedHeader(format!(
            "{} already contains a case",
            paths.root.display()
        )));
    }

    std::fs::create_dir_all(&paths.root)?;
    std::fs::create_dir_all(paths.blobs())?;

    let new_case = create_header(case_id, passphrase, KdfProfile::V1)?;
    let conn = open_database(&paths, &new_case.cmk)?;
    migrations::migrate(&conn)?;

    audit::append(
        &conn,
        &NewAuditEvent {
            actor: "user:local",
            action: "case.created",
            subject_kind: Some("case"),
            subject_id: Some(case_id),
            payload_json: "{}",
        },
    )?;

    // Written last: if anything above fails, no header exists and the directory
    // is not mistaken for a valid case.
    write_header(&paths, &new_case.header)?;

    Ok((conn, paths, new_case.recovery_key))
}

/// Open an existing case with a passphrase.
pub fn open_case(root: impl AsRef<Path>, passphrase: &str) -> Result<(Connection, CasePaths)> {
    let paths = CasePaths::new(root);
    let header = read_header(&paths)?;
    let cmk = unlock_with_passphrase(&header, passphrase)?;
    finish_open(&paths, &header, &cmk, "passphrase")
}

/// Open an existing case with the recovery key, for a forgotten passphrase.
pub fn open_case_with_recovery_key(
    root: impl AsRef<Path>,
    recovery_key: &RecoveryKey,
) -> Result<(Connection, CasePaths)> {
    let paths = CasePaths::new(root);
    let header = read_header(&paths)?;
    let cmk = unlock_with_recovery_key(&header, recovery_key)?;
    finish_open(&paths, &header, &cmk, "recovery_key")
}

/// Shared tail of both open paths.
///
/// Records **which key opened the case**. An investigator reviewing activity
/// history should be able to see that a case was opened with the recovery key
/// rather than the passphrase, because that is exactly the event worth noticing
/// if it was not them.
fn finish_open(
    paths: &CasePaths,
    header: &CaseHeader,
    cmk: &CaseMasterKey,
    unlocked_with: &str,
) -> Result<(Connection, CasePaths)> {
    let conn = open_database(paths, cmk)?;
    migrations::migrate(&conn)?;

    audit::append(
        &conn,
        &NewAuditEvent {
            actor: "user:local",
            action: "case.opened",
            subject_kind: Some("case"),
            subject_id: Some(&header.case_id),
            payload_json: &format!("{{\"unlocked_with\":\"{unlocked_with}\"}}"),
        },
    )?;

    Ok((conn, paths.clone()))
}

/// Change a case's passphrase.
///
/// Rewrites `header.json` only. The database is untouched, because the key that
/// encrypts it did not change. The existing recovery key keeps working — a
/// passphrase change must not silently invalidate a recovery key the user wrote
/// down months ago.
pub fn change_passphrase(
    root: impl AsRef<Path>,
    old_passphrase: &str,
    new_passphrase: &str,
) -> Result<()> {
    let paths = CasePaths::new(root);
    let header = read_header(&paths)?;
    let updated = rotate_passphrase(&header, old_passphrase, new_passphrase)?;

    // Unlock before the header changes; the CMK is the same either way, but
    // deriving from the old passphrase is what proves the caller knew it.
    let cmk = unlock_with_passphrase(&header, old_passphrase)?;

    // The header is written first, then the event recorded. The header and the
    // database are separate files, so there is no atomic option here, and the
    // ordering decides which way an interrupted rotation fails:
    //
    //   header first -> a crash leaves a real rotation unlogged.
    //   audit first  -> a crash leaves a log entry for a rotation that never
    //                   happened.
    //
    // An incomplete log is recoverable; a log that asserts something false is
    // not, and it is precisely the failure this project exists to avoid. If the
    // append below fails, the error is returned rather than swallowed, so the
    // caller learns the rotation succeeded but was not recorded.
    write_header(&paths, &updated)?;

    let conn = open_database(&paths, &cmk)?;
    migrations::migrate(&conn)?;
    audit::append(
        &conn,
        &NewAuditEvent {
            actor: "user:local",
            action: "case.passphrase_rotated",
            subject_kind: Some("case"),
            subject_id: Some(&header.case_id),
            payload_json: "{}",
        },
    )?;

    Ok(())
}

/// Open the SQLCipher database with a raw key.
fn open_database(paths: &CasePaths, cmk: &CaseMasterKey) -> Result<Connection> {
    let conn = Connection::open(paths.database())?;

    // `x'...'` supplies the key directly, bypassing SQLCipher's own KDF. The
    // KDF work was already done by Argon2id at 64 MiB, which is far stronger
    // than SQLCipher's default PBKDF2 (ADR-0016).
    let literal = header::raw_key_literal(cmk);
    conn.execute_batch(&format!("PRAGMA key = {};", literal.as_str()))?;
    conn.execute_batch(&format!("PRAGMA cipher_page_size = {CIPHER_PAGE_SIZE};"))?;

    verify_readable(&conn)?;

    // WAL survives a crash mid-write, which the spec requires for autosave and
    // crash recovery.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    Ok(conn)
}

/// Force SQLCipher to actually decrypt a page, converting the generic
/// "file is not a database" into a meaningful error.
fn verify_readable(conn: &Connection) -> Result<()> {
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::NotADatabase =>
        {
            Err(StoreError::WrongPassphraseOrNotACase)
        }
        Err(e) => Err(StoreError::Sqlite(e)),
    }
}

/// True if the linked SQLite actually is SQLCipher.
///
/// Guards against a build that silently links plain SQLite — which would
/// produce unencrypted case files while every functional test still passed.
pub fn is_sqlcipher(conn: &Connection) -> Result<bool> {
    let version: std::result::Result<String, _> =
        conn.query_row("PRAGMA cipher_version", [], |r| r.get(0));
    Ok(matches!(version, Ok(v) if !v.is_empty()))
}

/// The linked cipher backend's version, or `None` if this is plain SQLite.
///
/// `cipher_version` is a property of the linked library, not of any database,
/// so this needs no key, no file, and no key derivation — which is what makes
/// it usable as a cheap startup probe. Creating a throwaway case just to answer
/// this question would run Argon2id at 64 MiB for no reason.
pub fn cipher_backend() -> Result<Option<String>> {
    let conn = Connection::open_in_memory()?;
    let version: std::result::Result<String, _> =
        conn.query_row("PRAGMA cipher_version", [], |r| r.get(0));
    Ok(match version {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    })
}

#[cfg(test)]
// Tests are the one place a panic is the correct response to an unexpected
// value: it is how the failure gets reported. The workspace-wide ban on
// unwrap/expect exists for production paths, where a panic loses a case.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_case_dir(name: &str) -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "kokin-test-{name}-{}-{n}.kokincase",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn a_case_survives_close_and_reopen() {
        let dir = temp_case_dir("roundtrip");

        let (conn, _paths, _recovery) = create_case(&dir, "case-001", "correct horse").unwrap();
        assert!(
            is_sqlcipher(&conn).unwrap(),
            "linked against plain SQLite, not SQLCipher - case files would be unencrypted"
        );
        conn.execute_batch("CREATE TABLE probe(v TEXT); INSERT INTO probe VALUES ('kokin');")
            .unwrap();
        drop(conn);

        let (conn, _paths) = open_case(&dir, "correct horse").unwrap();
        let v: String = conn
            .query_row("SELECT v FROM probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "kokin");
        drop(conn);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn wrong_passphrase_is_rejected_and_named_as_such() {
        let dir = temp_case_dir("wrongpass");
        let (conn, ..) = create_case(&dir, "case-002", "the right one").unwrap();
        drop(conn);

        let err = open_case(&dir, "the wrong one").unwrap_err();
        assert!(
            matches!(err, StoreError::Key(kokin_keys::KeyError::WrongKey)),
            "expected a named wrong-key error, got: {err}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The reason the key hierarchy exists. Without it, this test is impossible
    /// to write, because there would be nothing but the passphrase.
    #[test]
    fn a_recovery_key_opens_a_case_whose_passphrase_is_lost() {
        let dir = temp_case_dir("recovery");

        let (conn, _paths, recovery) = create_case(&dir, "case-003", "forgotten").unwrap();
        conn.execute_batch("CREATE TABLE probe(v TEXT); INSERT INTO probe VALUES ('survived');")
            .unwrap();
        drop(conn);

        let (conn, _paths) = open_case_with_recovery_key(&dir, &recovery).unwrap();
        let v: String = conn
            .query_row("SELECT v FROM probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "survived");
        drop(conn);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Rotation must not re-encrypt data, and must not break the recovery key.
    #[test]
    fn changing_the_passphrase_preserves_data_and_the_recovery_key() {
        let dir = temp_case_dir("rotate");

        let (conn, _paths, recovery) = create_case(&dir, "case-004", "old passphrase").unwrap();
        conn.execute_batch("CREATE TABLE probe(v TEXT); INSERT INTO probe VALUES ('intact');")
            .unwrap();
        drop(conn);

        change_passphrase(&dir, "old passphrase", "new passphrase").unwrap();

        // The new passphrase works and the data is untouched.
        let (conn, _paths) = open_case(&dir, "new passphrase").unwrap();
        let v: String = conn
            .query_row("SELECT v FROM probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "intact");
        drop(conn);

        // The old one does not.
        assert!(open_case(&dir, "old passphrase").is_err());

        // And the recovery key written down before the change still works.
        let (conn, _paths) = open_case_with_recovery_key(&dir, &recovery).unwrap();
        let v: String = conn
            .query_row("SELECT v FROM probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "intact");
        drop(conn);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_directory_without_a_header_is_not_a_case() {
        let dir = temp_case_dir("notacase");
        std::fs::create_dir_all(&dir).unwrap();

        let err = open_case(&dir, "whatever").unwrap_err();
        assert!(
            matches!(err, StoreError::NotACase(_)),
            "expected NotACase, got: {err}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A damaged header must not surface as a confusing SQLite error.
    #[test]
    fn a_corrupt_header_is_named_as_such() {
        let dir = temp_case_dir("corrupt");
        let (conn, paths, _r) = create_case(&dir, "case-005", "passphrase").unwrap();
        drop(conn);

        std::fs::write(paths.header(), "{ this is not json").unwrap();

        let err = open_case(&dir, "passphrase").unwrap_err();
        assert!(
            matches!(err, StoreError::MalformedHeader(_)),
            "expected MalformedHeader, got: {err}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A case written by a newer build must say so rather than look corrupt.
    #[test]
    fn a_header_from_a_future_version_is_rejected_by_name() {
        let dir = temp_case_dir("future");
        let (conn, paths, _r) = create_case(&dir, "case-006", "passphrase").unwrap();
        drop(conn);

        let text = std::fs::read_to_string(paths.header()).unwrap();
        let bumped = text.replace("\"format_version\": 1", "\"format_version\": 9999");
        assert_ne!(text, bumped, "the header layout changed - fix this test");
        std::fs::write(paths.header(), bumped).unwrap();

        let err = open_case(&dir, "passphrase").unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::UnsupportedHeaderVersion { found: 9999, .. }
            ),
            "expected UnsupportedHeaderVersion, got: {err}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Editing the header to claim different KDF parameters must fail to
    /// authenticate rather than silently deriving a key that decrypts nothing.
    #[test]
    fn tampering_with_the_header_kdf_profile_is_detected() {
        let dir = temp_case_dir("aadtamper");
        let (conn, paths, _r) = create_case(&dir, "case-007", "passphrase").unwrap();
        drop(conn);

        let text = std::fs::read_to_string(paths.header()).unwrap();
        // Profile 1 is the only one defined, so claim an undefined one.
        let tampered = text.replace("\"kdf_profile_id\": 1", "\"kdf_profile_id\": 2");
        assert_ne!(text, tampered, "the header layout changed - fix this test");
        std::fs::write(paths.header(), tampered).unwrap();

        let err = open_case(&dir, "passphrase").unwrap_err();
        assert!(
            matches!(
                err,
                StoreError::Key(kokin_keys::KeyError::UnsupportedProfile(2))
            ),
            "expected the unknown profile to be rejected, got: {err}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The audit chain must record the case lifecycle and still verify. An
    /// audit mechanism nothing writes to is decorative.
    #[test]
    fn the_case_lifecycle_is_recorded_in_a_verifiable_chain() {
        let dir = temp_case_dir("auditlifecycle");

        let (conn, _paths, recovery) = create_case(&dir, "case-010", "old passphrase").unwrap();
        drop(conn);

        change_passphrase(&dir, "old passphrase", "new passphrase").unwrap();

        let (conn, _paths) = open_case(&dir, "new passphrase").unwrap();
        drop(conn);

        let (conn, _paths) = open_case_with_recovery_key(&dir, &recovery).unwrap();

        let actions: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT action FROM audit_event ORDER BY id")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            actions,
            vec![
                "case.created",
                "case.passphrase_rotated",
                "case.opened",
                "case.opened",
            ]
        );

        // Which key opened the case is recorded, because an unexpected
        // recovery-key unlock is exactly the event worth noticing.
        let last_payload: String = conn
            .query_row(
                "SELECT payload_json FROM audit_event ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            last_payload.contains("recovery_key"),
            "expected the recovery-key unlock to be recorded, got: {last_payload}"
        );

        assert_eq!(
            verify_chain(&conn).unwrap(),
            ChainStatus::Intact { events: 4 }
        );
        drop(conn);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The database file must not contain plaintext. This is the test that
    /// would catch an accidental unencrypted build in a way a functional test
    /// cannot.
    #[test]
    fn case_database_contains_no_plaintext() {
        let dir = temp_case_dir("plaintext");

        let (conn, paths, _r) = create_case(&dir, "case-008", "passphrase").unwrap();
        conn.execute_batch(
            "CREATE TABLE probe(v TEXT); INSERT INTO probe VALUES ('SUPERSECRETMARKER');",
        )
        .unwrap();
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .unwrap();
        drop(conn);

        let bytes = std::fs::read(paths.database()).unwrap();
        assert!(
            !bytes.windows(17).any(|w| w == b"SUPERSECRETMARKER"),
            "plaintext found in the case database - encryption is not active"
        );
        assert!(
            !bytes.starts_with(b"SQLite format 3\0"),
            "case database has a plain SQLite header - encryption is not active"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The header is plaintext by design, but it must never contain key
    /// material that could open the case on its own.
    #[test]
    fn the_header_carries_no_usable_key_material() {
        let dir = temp_case_dir("headersecrets");
        // A passphrase distinctive enough that a substring match means the
        // value itself leaked, not merely the word "passphrase" appearing in a
        // field name like cmk_wrapped_by_passphrase.
        const SECRET: &str = "ZZQX-header-leak-canary-4718";
        let (conn, paths, recovery) = create_case(&dir, "case-009", SECRET).unwrap();
        drop(conn);

        let text = std::fs::read_to_string(paths.header()).unwrap();

        // The recovery key itself must not appear - only its wrap of the CMK.
        let recovery_hex = recovery.to_hex();
        assert!(
            !text.contains(recovery_hex.as_str()),
            "the recovery key is written into the plaintext header"
        );
        assert!(
            !text.contains(SECRET),
            "the passphrase is written into the plaintext header"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
