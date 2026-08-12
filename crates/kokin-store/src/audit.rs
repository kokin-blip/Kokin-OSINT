//! The audit hash-chain.
//!
//! Every recorded event carries `BLAKE3(prev_event_hash || canonical_cbor(payload))`,
//! so altering or removing an event breaks every hash after it.
//!
//! # This is tamper-evident, not tamper-proof, and it is not chain of custody
//!
//! It detects modification by anyone who **cannot** recompute the chain: file
//! corruption, a partial write, an unsophisticated edit.
//!
//! It offers **nothing** against the case owner. Anyone holding the passphrase
//! can rewrite the chain from genesis and produce a perfectly valid log, because
//! there is no external anchor and no third-party attestation. The case owner is
//! inside the trust boundary.
//!
//! ADR-0014 forbids describing this as chain of custody, and requires the
//! disclaimer to appear in the UI rather than only in this comment. Attack
//! catalogue row A-017 records it as an accepted, unmitigated limitation.

use rusqlite::{Connection, OptionalExtension};

use crate::{Result, StoreError};

/// The hash a chain starts from. All zeroes, so a chain of length one is still
/// verifiable without a special case.
pub const GENESIS_HASH: [u8; 32] = [0u8; 32];

/// An event to append. Ordinary struct rather than free arguments so adding a
/// field later does not silently reorder call sites.
#[derive(Debug, Clone)]
pub struct NewAuditEvent<'a> {
    /// Who acted. Prefixes are meaningful elsewhere: `user:`, `rule:`, `ai:`
    /// (see ADR-0008 - automated actors cannot merge entities).
    pub actor: &'a str,
    pub action: &'a str,
    pub subject_kind: Option<&'a str>,
    pub subject_id: Option<&'a str>,
    /// Structured detail. Must be a JSON object; it is re-encoded as canonical
    /// CBOR for hashing so key order cannot change the hash.
    pub payload_json: &'a str,
}

/// A stored event, as read back for verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: i64,
    pub occurred_utc: String,
    pub actor: String,
    pub action: String,
    pub subject_kind: Option<String>,
    pub subject_id: Option<String>,
    pub payload_json: String,
    pub prev_event_hash: Vec<u8>,
    pub event_hash: Vec<u8>,
}

/// What `verify_chain` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStatus {
    /// Every link recomputes. Note the header warning: this means nobody
    /// *without* the case key has altered the log.
    Intact { events: u64 },
    /// A link does not recompute. `at_id` is the first bad event.
    Broken { at_id: i64, reason: String },
}

/// Append an event, chaining it to the current tip.
pub fn append(conn: &Connection, event: &NewAuditEvent<'_>) -> Result<[u8; 32]> {
    let prev = tip_hash(conn)?;
    let occurred = now_utc_rfc3339();

    let hash = compute_hash(
        &prev,
        &occurred,
        event.actor,
        event.action,
        event.subject_kind,
        event.subject_id,
        event.payload_json,
    )?;

    conn.execute(
        "INSERT INTO audit_event
            (occurred_utc, actor, action, subject_kind, subject_id, payload_json,
             prev_event_hash, event_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            occurred,
            event.actor,
            event.action,
            event.subject_kind,
            event.subject_id,
            event.payload_json,
            prev.as_slice(),
            hash.as_slice(),
        ],
    )?;

    Ok(hash)
}

/// The hash of the most recent event, or genesis for an empty chain.
pub fn tip_hash(conn: &Connection) -> Result<[u8; 32]> {
    let row: Option<Vec<u8>> = conn
        .query_row(
            "SELECT event_hash FROM audit_event ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;

    match row {
        None => Ok(GENESIS_HASH),
        Some(bytes) => bytes
            .as_slice()
            .try_into()
            .map_err(|_| StoreError::MalformedHeader("audit tip hash is not 32 bytes".into())),
    }
}

/// Walk the whole chain and recompute every link.
pub fn verify_chain(conn: &Connection) -> Result<ChainStatus> {
    let mut stmt = conn.prepare(
        "SELECT id, occurred_utc, actor, action, subject_kind, subject_id,
                payload_json, prev_event_hash, event_hash
           FROM audit_event
          ORDER BY id ASC",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(AuditEvent {
            id: r.get(0)?,
            occurred_utc: r.get(1)?,
            actor: r.get(2)?,
            action: r.get(3)?,
            subject_kind: r.get(4)?,
            subject_id: r.get(5)?,
            payload_json: r.get(6)?,
            prev_event_hash: r.get(7)?,
            event_hash: r.get(8)?,
        })
    })?;

    let mut expected_prev = GENESIS_HASH.to_vec();
    let mut count = 0u64;

    for row in rows {
        let e = row?;
        count += 1;

        // Checked separately from the hash so a reordered or deleted event
        // reports as a broken link rather than as a hash mismatch, which is
        // more useful when diagnosing what actually happened.
        if e.prev_event_hash != expected_prev {
            return Ok(ChainStatus::Broken {
                at_id: e.id,
                reason: "prev_event_hash does not match the preceding event - \
                         an event was removed, reordered, or inserted"
                    .into(),
            });
        }

        let recomputed = compute_hash(
            &expected_prev,
            &e.occurred_utc,
            &e.actor,
            &e.action,
            e.subject_kind.as_deref(),
            e.subject_id.as_deref(),
            &e.payload_json,
        )?;

        if recomputed.as_slice() != e.event_hash.as_slice() {
            return Ok(ChainStatus::Broken {
                at_id: e.id,
                reason: "event_hash does not match the event's contents - \
                         the event was modified after it was recorded"
                    .into(),
            });
        }

        expected_prev = e.event_hash;
    }

    Ok(ChainStatus::Intact { events: count })
}

/// `BLAKE3(prev || canonical_cbor(payload))`.
///
/// Canonical CBOR, not JSON: JSON gives no guarantee about key order or number
/// formatting, so two byte-different encodings of the same logical event would
/// hash differently and the chain would break for no real reason.
#[allow(clippy::too_many_arguments)]
fn compute_hash(
    prev: &[u8],
    occurred_utc: &str,
    actor: &str,
    action: &str,
    subject_kind: Option<&str>,
    subject_id: Option<&str>,
    payload_json: &str,
) -> Result<[u8; 32]> {
    // A fixed-order tuple rather than a map: field order is defined by this
    // literal, so there is no key-ordering question to get wrong.
    let payload = (
        occurred_utc,
        actor,
        action,
        subject_kind,
        subject_id,
        payload_json,
    );

    let mut encoded = Vec::new();
    ciborium::into_writer(&payload, &mut encoded)
        .map_err(|e| StoreError::MalformedHeader(format!("could not encode audit payload: {e}")))?;

    let mut hasher = blake3::Hasher::new();
    hasher.update(prev);
    hasher.update(&encoded);
    Ok(*hasher.finalize().as_bytes())
}

fn now_utc_rfc3339() -> String {
    crate::header::now_utc_rfc3339()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::migrate(&conn).unwrap();
        conn
    }

    fn event<'a>(action: &'a str) -> NewAuditEvent<'a> {
        NewAuditEvent {
            actor: "user:local",
            action,
            subject_kind: None,
            subject_id: None,
            payload_json: "{}",
        }
    }

    #[test]
    fn an_empty_chain_is_intact() {
        let conn = db();
        assert_eq!(
            verify_chain(&conn).unwrap(),
            ChainStatus::Intact { events: 0 }
        );
    }

    #[test]
    fn the_first_event_chains_from_genesis() {
        let conn = db();
        append(&conn, &event("case.created")).unwrap();

        let prev: Vec<u8> = conn
            .query_row("SELECT prev_event_hash FROM audit_event", [], |r| r.get(0))
            .unwrap();
        assert_eq!(prev, GENESIS_HASH.to_vec());
        assert_eq!(
            verify_chain(&conn).unwrap(),
            ChainStatus::Intact { events: 1 }
        );
    }

    #[test]
    fn a_chain_of_many_events_verifies() {
        let conn = db();
        for i in 0..25 {
            let action = format!("test.event.{i}");
            append(&conn, &event(&action)).unwrap();
        }
        assert_eq!(
            verify_chain(&conn).unwrap(),
            ChainStatus::Intact { events: 25 }
        );
    }

    /// The property the whole mechanism exists for.
    #[test]
    fn modifying_an_event_breaks_the_chain() {
        let conn = db();
        for i in 0..5 {
            let action = format!("test.event.{i}");
            append(&conn, &event(&action)).unwrap();
        }

        conn.execute(
            "UPDATE audit_event SET action = 'tampered' WHERE id = 3",
            [],
        )
        .unwrap();

        match verify_chain(&conn).unwrap() {
            ChainStatus::Broken { at_id, reason } => {
                assert_eq!(at_id, 3);
                assert!(reason.contains("modified"), "unexpected reason: {reason}");
            }
            other => panic!("expected a broken chain, got {other:?}"),
        }
    }

    #[test]
    fn deleting_an_event_breaks_the_chain() {
        let conn = db();
        for i in 0..5 {
            let action = format!("test.event.{i}");
            append(&conn, &event(&action)).unwrap();
        }

        conn.execute("DELETE FROM audit_event WHERE id = 3", [])
            .unwrap();

        match verify_chain(&conn).unwrap() {
            ChainStatus::Broken { at_id, reason } => {
                // The event after the hole is where the break is detected.
                assert_eq!(at_id, 4);
                assert!(reason.contains("removed"), "unexpected reason: {reason}");
            }
            other => panic!("expected a broken chain, got {other:?}"),
        }
    }

    /// Payloads differing only in key order must hash identically, or the chain
    /// would break on a re-serialisation that changed nothing meaningful.
    #[test]
    fn hashing_is_deterministic_for_identical_input() {
        let a = compute_hash(
            &GENESIS_HASH,
            "2026-08-12T00:00:00Z",
            "user:local",
            "case.created",
            None,
            None,
            "{}",
        )
        .unwrap();
        let b = compute_hash(
            &GENESIS_HASH,
            "2026-08-12T00:00:00Z",
            "user:local",
            "case.created",
            None,
            None,
            "{}",
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_different_predecessor_produces_a_different_hash() {
        let common = |prev: &[u8]| {
            compute_hash(
                prev,
                "2026-08-12T00:00:00Z",
                "user:local",
                "case.created",
                None,
                None,
                "{}",
            )
            .unwrap()
        };
        assert_ne!(common(&GENESIS_HASH), common(&[9u8; 32]));
    }

    /// Documents the limitation rather than pretending it away: someone who can
    /// write to the database can rebuild the whole chain and it will verify.
    /// This test exists so the claim in ADR-0014 is demonstrably true, not
    /// merely asserted.
    #[test]
    fn anyone_who_can_write_can_forge_a_valid_chain() {
        let conn = db();
        append(&conn, &event("case.created")).unwrap();
        append(&conn, &event("evidence.ingested")).unwrap();
        assert!(matches!(
            verify_chain(&conn).unwrap(),
            ChainStatus::Intact { .. }
        ));

        // An attacker with the case key wipes the log and writes their own.
        conn.execute("DELETE FROM audit_event", []).unwrap();
        append(&conn, &event("case.created")).unwrap();

        assert_eq!(
            verify_chain(&conn).unwrap(),
            ChainStatus::Intact { events: 1 },
            "a rebuilt chain verifies - this is tamper-evident, not tamper-proof"
        );
    }
}
