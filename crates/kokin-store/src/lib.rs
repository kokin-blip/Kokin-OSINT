//! Encrypted case storage.
//!
//! One case is one SQLCipher-encrypted SQLite file. Page-level encryption means
//! the FTS5 index is covered for free — see ADR-0004 for why application-level
//! field encryption was rejected.
//!
//! Increment 1 scope: prove the encrypted round-trip builds and runs on both
//! Windows and macOS. The three-layer schema (ADR-0006) lands in increment 3.

use std::path::Path;

use rusqlite::Connection;

/// Errors this crate can produce.
///
/// Deliberately distinguishes a wrong passphrase from a corrupt file: the UI
/// must be able to say "wrong passphrase" without implying data loss.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("could not decrypt the case: wrong passphrase, or the file is not a Kokin case")]
    WrongPassphraseOrNotACase,

    #[error("case file is encrypted with an unsupported cipher configuration")]
    UnsupportedCipherConfig,
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// The SQLCipher configuration a case file is written with.
///
/// Pinned explicitly rather than relying on the library default, so that a
/// future SQLCipher upgrade cannot silently change the on-disk format and
/// strand existing cases. Any change here requires a migration path.
const CIPHER_PAGE_SIZE: i64 = 4096;
const KDF_ITER: i64 = 256_000;

/// Open (or create) an encrypted case file.
///
/// The passphrase is applied before any other statement runs. SQLCipher does not
/// validate the key at `PRAGMA key` time — it fails on first read — so this
/// function forces a read of `sqlite_master` to surface a wrong passphrase
/// immediately rather than at some arbitrary later query.
pub fn open_encrypted(path: impl AsRef<Path>, passphrase: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    apply_key(&conn, passphrase)?;
    verify_readable(&conn)?;

    // Durability: WAL survives a crash mid-write, which the spec requires for
    // autosave and crash recovery.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    Ok(conn)
}

fn apply_key(conn: &Connection, passphrase: &str) -> Result<()> {
    // pragma_update would quote this as a bound value; SQLCipher requires the
    // key as a literal, so it is escaped by doubling single quotes.
    let escaped = passphrase.replace('\'', "''");
    conn.execute_batch(&format!("PRAGMA key = '{escaped}';"))?;
    conn.execute_batch(&format!(
        "PRAGMA cipher_page_size = {CIPHER_PAGE_SIZE};
         PRAGMA kdf_iter = {KDF_ITER};"
    ))?;
    Ok(())
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
/// produce unencrypted case files while every test still passed.
pub fn is_sqlcipher(conn: &Connection) -> Result<bool> {
    let version: std::result::Result<String, _> =
        conn.query_row("PRAGMA cipher_version", [], |r| r.get(0));
    Ok(matches!(version, Ok(v) if !v.is_empty()))
}

#[cfg(test)]
// Tests are the one place a panic is the correct response to an unexpected
// value: it is how the failure gets reported. The workspace-wide ban on
// unwrap/expect exists for production paths, where a panic loses a case.
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn temp_case(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "kokin-test-{name}-{}.kokincase",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// The load-bearing test for increment 1. If this fails on macOS CI, the
    /// whole storage decision (ADR-0003/0004) is wrong and we find out on day
    /// one rather than in month three.
    #[test]
    fn encrypted_case_survives_close_and_reopen() {
        let path = temp_case("roundtrip");

        let conn = open_encrypted(&path, "correct horse battery staple").unwrap();
        assert!(
            is_sqlcipher(&conn).unwrap(),
            "linked against plain SQLite, not SQLCipher — case files would be unencrypted"
        );
        conn.execute_batch("CREATE TABLE probe(v TEXT); INSERT INTO probe VALUES ('kokin');")
            .unwrap();
        drop(conn);

        let conn = open_encrypted(&path, "correct horse battery staple").unwrap();
        let v: String = conn
            .query_row("SELECT v FROM probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "kokin");
        drop(conn);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn wrong_passphrase_is_rejected_and_named_as_such() {
        let path = temp_case("wrongpass");

        let conn = open_encrypted(&path, "the right one").unwrap();
        conn.execute_batch("CREATE TABLE probe(v TEXT);").unwrap();
        drop(conn);

        let err = open_encrypted(&path, "the wrong one").unwrap_err();
        assert!(
            matches!(err, StoreError::WrongPassphraseOrNotACase),
            "expected a named passphrase error, got: {err}"
        );

        std::fs::remove_file(&path).unwrap();
    }

    /// The file on disk must not contain plaintext. This is the test that would
    /// catch an accidental unencrypted build in a way a functional test cannot.
    #[test]
    fn case_file_contains_no_plaintext() {
        let path = temp_case("plaintext");

        let conn = open_encrypted(&path, "passphrase").unwrap();
        conn.execute_batch(
            "CREATE TABLE probe(v TEXT); INSERT INTO probe VALUES ('SUPERSECRETMARKER');",
        )
        .unwrap();
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .unwrap();
        drop(conn);

        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes.windows(17).any(|w| w == b"SUPERSECRETMARKER"),
            "plaintext found in the case file — encryption is not active"
        );
        // A plain SQLite file starts with this magic; an encrypted one must not.
        assert!(
            !bytes.starts_with(b"SQLite format 3\0"),
            "case file has a plain SQLite header — encryption is not active"
        );

        std::fs::remove_file(&path).unwrap();
    }
}
