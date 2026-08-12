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

    -- Append-only record of what happened in this case, chained by
    -- BLAKE3(prev_event_hash || canonical_cbor(payload)). Tamper-EVIDENT only:
    -- see crates/kokin-store/src/audit.rs and ADR-0014 for what that does and
    -- does not mean.
    CREATE TABLE audit_event (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        occurred_utc    TEXT NOT NULL,
        actor           TEXT NOT NULL,
        action          TEXT NOT NULL,
        subject_kind    TEXT,
        subject_id      TEXT,
        payload_json    TEXT NOT NULL DEFAULT '{}',
        prev_event_hash BLOB NOT NULL,
        event_hash      BLOB NOT NULL
    ) STRICT;

    -- The chain is only meaningful if hashes are unique; a duplicate would
    -- make two different histories verify.
    CREATE UNIQUE INDEX audit_event_hash ON audit_event(event_hash);

    CREATE INDEX audit_event_occurred ON audit_event(occurred_utc);
    "#,
    // 2: layers L0 (provenance) and L1 (lineage) from ADR-0006.
    //
    // L0 rows are facts about what was collected and are APPEND-ONLY, enforced
    // by triggers rather than by convention. L2 (entity, relationship, claim)
    // is mutable and arrives with the increment that creates entities.
    r#"
    -- ---------------------------------------------------------------------
    -- L0: provenance. Append-only. Never updated, never deleted.
    -- ---------------------------------------------------------------------

    -- Where evidence came from. canonical_locator is the identity used for
    -- deduplication; raw_locator is verbatim and is never edited, because
    -- editing it would falsify the record of what was actually fetched.
    CREATE TABLE source (
        id                TEXT PRIMARY KEY NOT NULL,
        kind              TEXT NOT NULL,
        raw_locator       TEXT NOT NULL,
        canonical_locator TEXT NOT NULL,
        first_seen_utc    TEXT NOT NULL
    ) STRICT;

    CREATE INDEX source_canonical ON source(canonical_locator);

    -- Content-addressed bytes. The bytes live in blobs/ (kokin-blob); this row
    -- carries the hash, the size, and the wrapped per-blob key.
    --
    -- wrapped_key is nullable ON PURPOSE: crypto-shredding sets it to NULL.
    -- The row, hash, size, and every derivation edge survive, so the record
    -- that this evidence existed is preserved while the bytes become
    -- permanently unreadable. See docs/limitations/deletion.md.
    CREATE TABLE blob (
        content_hash    TEXT PRIMARY KEY NOT NULL,
        size_bytes      INTEGER NOT NULL,
        wrapped_key     BLOB,
        wrapped_nonce   BLOB,
        stored_utc      TEXT NOT NULL,
        shredded_utc    TEXT
    ) STRICT;

    -- One retrieval. capture_completeness records what KIND of snapshot this
    -- is, so a static-HTML-only fetch of a JavaScript-heavy page is never
    -- mistaken for the full page a user would have seen.
    CREATE TABLE capture (
        id                   TEXT PRIMARY KEY NOT NULL,
        source_id            TEXT NOT NULL REFERENCES source(id),
        content_hash         TEXT REFERENCES blob(content_hash),
        requested_utc        TEXT NOT NULL,
        http_status          INTEGER,
        request_json         TEXT NOT NULL DEFAULT '{}',
        response_headers_json TEXT NOT NULL DEFAULT '{}',
        redirect_chain_json  TEXT NOT NULL DEFAULT '[]',
        capture_completeness TEXT NOT NULL,
        from_fixture         INTEGER NOT NULL DEFAULT 0,
        connector            TEXT NOT NULL,
        job_id               TEXT NOT NULL
    ) STRICT;

    CREATE INDEX capture_source ON capture(source_id);

    -- A typed thing derived from a capture or a file: an HTML document, an
    -- image, a PDF. Media type is what was observed, not what was claimed.
    CREATE TABLE artifact (
        id            TEXT PRIMARY KEY NOT NULL,
        content_hash  TEXT NOT NULL REFERENCES blob(content_hash),
        media_type    TEXT NOT NULL,
        byte_length   INTEGER NOT NULL,
        created_utc   TEXT NOT NULL
    ) STRICT;

    CREATE INDEX artifact_hash ON artifact(content_hash);

    -- ---------------------------------------------------------------------
    -- L1: lineage. Which code produced what, from what.
    -- ---------------------------------------------------------------------

    -- One execution of one transform. code_version and params_hash are what
    -- make a rerun after an upgrade distinguishable from the original run,
    -- which is the whole basis of observation supersession (ADR-0006).
    CREATE TABLE transform_run (
        id              TEXT PRIMARY KEY NOT NULL,
        transform_name  TEXT NOT NULL,
        transform_version TEXT NOT NULL,
        code_version    TEXT NOT NULL,
        params_hash     TEXT NOT NULL,
        started_utc     TEXT NOT NULL,
        finished_utc    TEXT,
        status          TEXT NOT NULL,
        error_code      TEXT,
        error_detail    TEXT
    ) STRICT;

    -- The lineage edge: this output came from that input, via that run.
    CREATE TABLE derivation (
        id             TEXT PRIMARY KEY NOT NULL,
        run_id         TEXT NOT NULL REFERENCES transform_run(id),
        input_kind     TEXT NOT NULL,
        input_id       TEXT NOT NULL,
        output_kind    TEXT NOT NULL,
        output_id      TEXT NOT NULL
    ) STRICT;

    CREATE INDEX derivation_output ON derivation(output_kind, output_id);
    CREATE INDEX derivation_input ON derivation(input_kind, input_id);

    -- ---------------------------------------------------------------------
    -- Append-only enforcement.
    --
    -- In the database, not in application code, because application code is
    -- where this kind of guarantee quietly erodes. A future contributor who
    -- "just needs to fix one row" gets an error naming the reason instead of
    -- silently rewriting provenance.
    --
    -- blob is the deliberate exception: crypto-shredding must be able to clear
    -- wrapped_key. The trigger below permits exactly that transition and
    -- nothing else.
    -- ---------------------------------------------------------------------

    CREATE TRIGGER source_is_append_only BEFORE UPDATE ON source
    BEGIN
        SELECT RAISE(ABORT, 'source is append-only: provenance cannot be edited');
    END;

    CREATE TRIGGER source_no_delete BEFORE DELETE ON source
    BEGIN
        SELECT RAISE(ABORT, 'source is append-only: provenance cannot be deleted');
    END;

    CREATE TRIGGER capture_is_append_only BEFORE UPDATE ON capture
    BEGIN
        SELECT RAISE(ABORT, 'capture is append-only: provenance cannot be edited');
    END;

    CREATE TRIGGER capture_no_delete BEFORE DELETE ON capture
    BEGIN
        SELECT RAISE(ABORT, 'capture is append-only: provenance cannot be deleted');
    END;

    CREATE TRIGGER artifact_is_append_only BEFORE UPDATE ON artifact
    BEGIN
        SELECT RAISE(ABORT, 'artifact is append-only: provenance cannot be edited');
    END;

    CREATE TRIGGER artifact_no_delete BEFORE DELETE ON artifact
    BEGIN
        SELECT RAISE(ABORT, 'artifact is append-only: provenance cannot be deleted');
    END;

    CREATE TRIGGER derivation_is_append_only BEFORE UPDATE ON derivation
    BEGIN
        SELECT RAISE(ABORT, 'derivation is append-only: lineage cannot be edited');
    END;

    CREATE TRIGGER derivation_no_delete BEFORE DELETE ON derivation
    BEGIN
        SELECT RAISE(ABORT, 'derivation is append-only: lineage cannot be deleted');
    END;

    -- A blob row may only ever change by being shredded. Any other update is
    -- rewriting the record of what was collected.
    CREATE TRIGGER blob_only_shred BEFORE UPDATE ON blob
    WHEN NOT (
        NEW.content_hash = OLD.content_hash
        AND NEW.size_bytes = OLD.size_bytes
        AND NEW.stored_utc = OLD.stored_utc
        AND NEW.wrapped_key IS NULL
        AND NEW.wrapped_nonce IS NULL
    )
    BEGIN
        SELECT RAISE(ABORT, 'blob is append-only except for crypto-shredding');
    END;

    CREATE TRIGGER blob_no_delete BEFORE DELETE ON blob
    BEGIN
        SELECT RAISE(ABORT, 'blob rows survive shredding: delete the key, not the row');
    END;

    -- transform_run is the one L1 table that legitimately mutates, because a
    -- run starts as 'running' and later becomes 'succeeded' or 'failed'. Only
    -- that completion is allowed; the identity of what ran is fixed.
    CREATE TRIGGER transform_run_only_completes BEFORE UPDATE ON transform_run
    WHEN NOT (
        NEW.id = OLD.id
        AND NEW.transform_name = OLD.transform_name
        AND NEW.transform_version = OLD.transform_version
        AND NEW.code_version = OLD.code_version
        AND NEW.params_hash = OLD.params_hash
        AND NEW.started_utc = OLD.started_utc
        AND OLD.status = 'running'
    )
    BEGIN
        SELECT RAISE(ABORT, 'a transform_run may only be completed, not rewritten');
    END;
    "#,
    // 3: the rest of L1 — the observation itself (ADR-0006).
    //
    // Migration 2 recorded that a transform ran and what it consumed. This
    // records what it *said*. Kept separate because it shipped separately, and
    // an applied migration is never edited.
    r#"
    -- A single extracted value, and exactly where in the artifact it came
    -- from. locator_json is what makes an observation checkable by a human:
    -- without it the value is an assertion, with it the reader can go and look.
    --
    -- value is stored verbatim, including hostile content. Nothing in the
    -- storage layer sanitises it, because sanitising on write would destroy the
    -- evidence; escaping is the renderer's job, at the point of display.
    CREATE TABLE observation (
        id            TEXT PRIMARY KEY NOT NULL,
        artifact_id   TEXT NOT NULL REFERENCES artifact(id),
        run_id        TEXT NOT NULL REFERENCES transform_run(id),
        kind          TEXT NOT NULL,
        value         TEXT NOT NULL,
        locator_json  TEXT NOT NULL,
        observed_utc  TEXT NOT NULL
    ) STRICT;

    CREATE INDEX observation_artifact ON observation(artifact_id);
    CREATE INDEX observation_run ON observation(run_id);
    CREATE INDEX observation_kind ON observation(kind, value);

    -- Rerunning a parser after an upgrade emits NEW observations; the old ones
    -- gain a row here (ADR-0006). They are never edited or deleted, because an
    -- upgrade must not be able to silently rewrite the basis of a conclusion an
    -- analyst already accepted.
    --
    -- superseded_by is nullable on purpose: a newer extractor that no longer
    -- makes an observation the old one made is withdrawing it, and "this is no
    -- longer observed" is a different fact from "this was replaced by that".
    CREATE TABLE observation_supersession (
        superseded_id  TEXT PRIMARY KEY NOT NULL REFERENCES observation(id),
        superseded_by  TEXT REFERENCES observation(id),
        run_id         TEXT NOT NULL REFERENCES transform_run(id),
        recorded_utc   TEXT NOT NULL
    ) STRICT;

    CREATE INDEX observation_supersession_by ON observation_supersession(superseded_by);

    -- Observations are L1 facts about what a parser saw, so they are
    -- append-only for the same reason provenance is.
    CREATE TRIGGER observation_is_append_only BEFORE UPDATE ON observation
    BEGIN
        SELECT RAISE(ABORT, 'observation is append-only: rerun the extractor, do not edit what it saw');
    END;

    CREATE TRIGGER observation_no_delete BEFORE DELETE ON observation
    BEGIN
        SELECT RAISE(ABORT, 'observation is append-only: supersede it, do not delete it');
    END;

    CREATE TRIGGER observation_supersession_is_append_only
    BEFORE UPDATE ON observation_supersession
    BEGIN
        SELECT RAISE(ABORT, 'a supersession is a historical fact and cannot be edited');
    END;

    CREATE TRIGGER observation_supersession_no_delete
    BEFORE DELETE ON observation_supersession
    BEGIN
        SELECT RAISE(ABORT, 'a supersession is a historical fact and cannot be deleted');
    END;
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

    /// Seed one row in each L0 table so the triggers have something to refuse.
    fn seed_l0(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO source VALUES ('s1','url','https://x/?utm_source=a','https://x/','2026-08-12T00:00:00Z');
             INSERT INTO blob VALUES ('hash1', 10, x'00', x'01', '2026-08-12T00:00:00Z', NULL);
             INSERT INTO capture VALUES ('c1','s1','hash1','2026-08-12T00:00:00Z',200,'{}','{}','[]','static_html_only',1,'http_fetch','job1');
             INSERT INTO artifact VALUES ('a1','hash1','text/html',10,'2026-08-12T00:00:00Z');
             INSERT INTO transform_run VALUES ('r1','fetch','1','abc','p1','2026-08-12T00:00:00Z',NULL,'running',NULL,NULL);
             INSERT INTO derivation VALUES ('d1','r1','capture','c1','artifact','a1');",
        )
        .unwrap();
    }

    /// Append-only is enforced by the database, not by application discipline.
    /// A trigger nobody attempts to violate is an untested trigger.
    #[test]
    fn provenance_tables_refuse_updates_and_deletes() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l0(&conn);

        let attempts = [
            (
                "UPDATE source SET raw_locator = 'edited' WHERE id = 's1'",
                "source update",
            ),
            ("DELETE FROM source WHERE id = 's1'", "source delete"),
            (
                "UPDATE capture SET http_status = 404 WHERE id = 'c1'",
                "capture update",
            ),
            ("DELETE FROM capture WHERE id = 'c1'", "capture delete"),
            (
                "UPDATE artifact SET media_type = 'text/plain' WHERE id = 'a1'",
                "artifact update",
            ),
            ("DELETE FROM artifact WHERE id = 'a1'", "artifact delete"),
            (
                "UPDATE derivation SET output_id = 'other' WHERE id = 'd1'",
                "derivation update",
            ),
            (
                "DELETE FROM derivation WHERE id = 'd1'",
                "derivation delete",
            ),
            (
                "DELETE FROM blob WHERE content_hash = 'hash1'",
                "blob delete",
            ),
        ];

        for (sql, what) in attempts {
            assert!(
                conn.execute_batch(sql).is_err(),
                "{what} was permitted - provenance can be rewritten"
            );
        }
    }

    /// Crypto-shredding is the single permitted mutation of a blob row: the
    /// key is destroyed, everything that records the blob existed survives.
    #[test]
    fn a_blob_may_be_shredded_but_not_otherwise_changed() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l0(&conn);

        conn.execute_batch(
            "UPDATE blob SET wrapped_key = NULL, wrapped_nonce = NULL,
                             shredded_utc = '2026-08-12T01:00:00Z'
             WHERE content_hash = 'hash1'",
        )
        .unwrap();

        // The record that this evidence existed is intact.
        let (size, key_is_null): (i64, bool) = conn
            .query_row(
                "SELECT size_bytes, wrapped_key IS NULL FROM blob WHERE content_hash = 'hash1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(size, 10);
        assert!(key_is_null);

        // But the hash or size cannot be rewritten under cover of a shred.
        assert!(conn
            .execute_batch("UPDATE blob SET size_bytes = 999 WHERE content_hash = 'hash1'")
            .is_err());
    }

    /// A run legitimately completes; it must not be able to change what ran.
    #[test]
    fn a_transform_run_may_complete_but_not_be_rewritten() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l0(&conn);

        conn.execute_batch(
            "UPDATE transform_run SET status = 'succeeded',
                    finished_utc = '2026-08-12T00:00:01Z' WHERE id = 'r1'",
        )
        .unwrap();

        // Changing which code ran, or re-completing a finished run, is refused.
        assert!(conn
            .execute_batch("UPDATE transform_run SET code_version = 'xyz' WHERE id = 'r1'")
            .is_err());
        assert!(conn
            .execute_batch("UPDATE transform_run SET status = 'failed' WHERE id = 'r1'")
            .is_err());
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

        let actual = schema_fingerprint(&conn).unwrap();

        if std::env::var("KOKIN_BLESS_SCHEMA").is_ok() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/golden_schema.txt");
            std::fs::write(
                path,
                format!(
                    "{actual}
"
                ),
            )
            .unwrap();
            println!("blessed {path}");
            return;
        }

        assert_eq!(
            actual,
            GOLDEN_SCHEMA.trim_end(),
            "the schema changed. If that is intended, append a migration (never              edit a shipped one) and re-bless with KOKIN_BLESS_SCHEMA=1."
        );
    }

    /// The expected schema, kept in a file rather than a string literal.
    ///
    /// Regenerate with:
    ///
    /// ```text
    /// KOKIN_BLESS_SCHEMA=1 cargo test -p kokin-store schema_matches
    /// ```
    ///
    /// The first version of this was a hand-written string literal, and it was
    /// wrong - it listed a table SQLite does not create until the first
    /// autoincrement insert. Hand-writing an expected value is exactly the
    /// mistake a golden test exists to catch, so the expected value is now
    /// copied from the database and never typed.
    const GOLDEN_SCHEMA: &str = include_str!("golden_schema.txt");
}
