//! Schema migrations.
//!
//! SQLite has no migration framework, so this is an explicit, tested strategy
//! (ADR-0003): `PRAGMA user_version` is the schema version, migrations are an
//! ordered list applied in one transaction each, and a golden-schema test pins
//! the result so a migration cannot drift from what a fresh database produces.
//!
//! A case written by a *newer* build is refused by name rather than opened and
//! silently misread — the alternative is an older build writing rows a newer
//! schema expects to be shaped differently.

use rusqlite::Connection;

use crate::{Result, StoreError};

/// Each entry is applied once, in order, and its index+1 becomes the resulting
/// `user_version`. **Never edit a migration that has shipped** — append a new
/// one, or existing cases diverge from new ones with no way to tell.
const MIGRATIONS: &[&str] = &[
    // 1: the minimum a case needs to identify itself. The three-layer evidence
    // schema (ADR-0006) lands in the increment that has something to store.
    r#"
    CREATE TABLE case_meta (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    ) STRICT;

    -- Append-only record of what happened in this case. The hash chain that
    -- makes it tamper-evident is added in the next increment; the table exists
    -- now so that increment does not have to migrate rows that already exist.
    CREATE TABLE audit_event (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        occurred_utc  TEXT NOT NULL,
        actor         TEXT NOT NULL,
        action        TEXT NOT NULL,
        subject_kind  TEXT,
        subject_id    TEXT,
        payload_json  TEXT NOT NULL DEFAULT '{}'
    ) STRICT;

    CREATE INDEX audit_event_occurred ON audit_event(occurred_utc);
    "#,
];

/// The schema version this build writes and understands.
pub fn target_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// Bring a database up to `target_version()`, or fail if it is newer.
pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if current > target_version() {
        return Err(StoreError::SchemaFromTheFuture {
            found: current,
            supported: target_version(),
        });
    }

    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }

        // Each migration is one transaction: a failure leaves user_version
        // untouched, so the next open retries from the same point rather than
        // finding a half-applied schema.
        conn.execute_batch("BEGIN")?;
        match conn.execute_batch(sql) {
            Ok(()) => {
                // user_version does not accept a bound parameter.
                conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
                conn.execute_batch("COMMIT")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(StoreError::Sqlite(e));
            }
        }
    }

    Ok(())
}

/// A stable, sorted description of the schema, for the golden test.
///
/// Reads `sqlite_master` rather than the migration text, so it describes what
/// the database actually is, not what we believe we asked for.
pub fn schema_fingerprint(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT type, name, COALESCE(sql, '')
           FROM sqlite_master
          WHERE name NOT LIKE 'sqlite_%'
          ORDER BY type, name",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(format!(
            "{}\t{}\t{}",
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            // Normalise whitespace so reindenting a migration does not read as
            // a schema change, while a real structural change still does.
            r.get::<_, String>(2)?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        ))
    })?;

    let mut lines = Vec::new();
    for row in rows {
        lines.push(row?);
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn migrating_a_fresh_database_reaches_the_target_version() {
        let conn = memory_db();
        migrate(&conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, target_version());
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        let first = schema_fingerprint(&conn).unwrap();

        // Must not fail by trying to CREATE TABLE a second time.
        migrate(&conn).unwrap();
        assert_eq!(schema_fingerprint(&conn).unwrap(), first);
    }

    #[test]
    fn a_newer_schema_is_refused_by_name() {
        let conn = memory_db();
        conn.execute_batch("PRAGMA user_version = 9999;").unwrap();

        let err = migrate(&conn).unwrap_err();
        assert!(
            matches!(err, StoreError::SchemaFromTheFuture { found: 9999, .. }),
            "expected SchemaFromTheFuture, got: {err}"
        );
    }

    /// Golden-schema test.
    ///
    /// Pins the exact schema the migrations produce. It fails on any structural
    /// change, which is the point: editing a shipped migration would leave
    /// existing cases on a different schema from new ones, with nothing to
    /// detect the divergence. When this fails, either append a migration, or
    /// update this constant *and* be certain the change is additive.
    #[test]
    fn schema_matches_the_golden_fingerprint() {
        let conn = memory_db();
        migrate(&conn).unwrap();

        assert_eq!(schema_fingerprint(&conn).unwrap(), GOLDEN_SCHEMA);
    }

    const GOLDEN_SCHEMA: &str = "\
index\taudit_event_occurred\tCREATE INDEX audit_event_occurred ON audit_event(occurred_utc)
table\taudit_event\tCREATE TABLE audit_event ( id INTEGER PRIMARY KEY AUTOINCREMENT, occurred_utc TEXT NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL, subject_kind TEXT, subject_id TEXT, payload_json TEXT NOT NULL DEFAULT '{}' ) STRICT
table\tcase_meta\tCREATE TABLE case_meta ( key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL ) STRICT";
}
