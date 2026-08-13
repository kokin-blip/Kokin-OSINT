//! Entity resolution: deciding that two entities are one person.
//!
//! # Nothing here rewrites an entity
//!
//! The conventional implementation of merge picks a survivor, repoints the
//! foreign keys and deletes the absorbed row. ADR-0008 refuses that, because it
//! destroys the evidence that argued *against* the merge along with everything
//! else — and the evidence against is precisely what an analyst needs when they
//! come back six weeks later and doubt themselves.
//!
//! So every entity keeps its row, its identifiers and its evidence links
//! forever. Identity is *read* through [`canonical_id`]. Splitting is
//! deactivating a merge-map row. Undoing is a further decision that leaves the
//! original one visible in history.
//!
//! # A machine may argue for a merge; only a person may make one
//!
//! [`propose`] takes anything as a proposer — a rule, a model, an analyst — and
//! is deliberately cheap, because a proposal is a question and questions should
//! be easy to raise. [`merge`] refuses any actor prefixed `rule:` or `ai:`.
//!
//! The check in this crate exists so the caller gets a named error. **The
//! guarantee is the `CHECK` constraint in migration 7**, which holds even for
//! code that never calls this module — see
//! `automated_actors_may_propose_a_merge_but_never_decide_one`. What neither can
//! catch is automation writing `actor = "user:local"`. That is a lie rather
//! than a bypass, no schema can detect it, and the audit chain is where it is
//! answerable.
//!
//! # The merge map is one level deep
//!
//! Migration 7 makes chains structurally impossible, so [`canonical_id`] is one
//! indexed lookup rather than a recursive walk over rows an analyst can toggle.
//! The cost lands here: merging two entities that already belong to clusters
//! means deactivating the losing cluster's rows and re-pointing its members at
//! the survivor, all inside one decision.

use rusqlite::Connection;

use crate::{new_id, GraphError, Result};
use kokin_store::now_utc_rfc3339 as now_utc;

/// A proposal that two entities are the same. A question, not a claim.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub left_entity: String,
    pub right_entity: String,
    /// `rule:shared_identifier`, `ai:name_similarity`, `user:local`.
    pub proposed_by: String,
    /// Why, in words. There is deliberately no similarity score: see the
    /// migration 7 comment, and ADR-0007 on why this product has no composite
    /// number anywhere.
    pub rationale: String,
    pub created_utc: String,
}

/// Actors that may raise a question but not settle one.
fn is_automated(actor: &str) -> bool {
    let lower = actor.to_ascii_lowercase();
    lower.starts_with("rule:") || lower.starts_with("ai:")
}

/// Put a pair in the canonical order the schema stores them in, so "A might be
/// B" and "B might be A" are one row rather than two.
fn ordered<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Suggest that two entities are the same.
///
/// Idempotent per proposer: a rule that runs nightly re-raising the same pair
/// updates its rationale rather than stacking up a thousand identical
/// questions.
pub fn propose(
    conn: &Connection,
    a: &str,
    b: &str,
    proposed_by: &str,
    rationale: &str,
) -> Result<String> {
    if a == b {
        return Err(GraphError::SelfMerge { entity: a.into() });
    }
    let (left, right) = ordered(a, b);
    for id in [left, right] {
        require_entity(conn, id)?;
    }

    let id = new_id()?;
    conn.execute(
        "INSERT INTO er_candidate
            (id, left_entity, right_entity, proposed_by, rationale, created_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(left_entity, right_entity, proposed_by)
         DO UPDATE SET rationale = excluded.rationale",
        rusqlite::params![id, left, right, proposed_by, rationale, now_utc()],
    )?;

    Ok(conn.query_row(
        "SELECT id FROM er_candidate
          WHERE left_entity = ?1 AND right_entity = ?2 AND proposed_by = ?3",
        rusqlite::params![left, right, proposed_by],
        |r| r.get(0),
    )?)
}

/// Proposals nobody has decided yet.
///
/// A candidate is open until some decision names the same pair. Derived rather
/// than stored: a status column would be a second place for the truth to live,
/// and the decisions are the truth.
pub fn open_candidates(conn: &Connection, limit: usize) -> Result<Vec<Candidate>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.left_entity, c.right_entity, c.proposed_by, c.rationale, c.created_utc
           FROM er_candidate c
          WHERE NOT EXISTS (
                SELECT 1 FROM er_decision d
                 WHERE d.left_entity = c.left_entity
                   AND d.right_entity = c.right_entity
          )
          ORDER BY c.created_utc, c.id
          LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok(Candidate {
            id: r.get(0)?,
            left_entity: r.get(1)?,
            right_entity: r.get(2)?,
            proposed_by: r.get(3)?,
            rationale: r.get(4)?,
            created_utc: r.get(5)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Record that two entities are the same person, and project it.
///
/// Returns the decision id. The survivor is the older of the two clusters'
/// canonical entities, tie-broken by id: an analyst expects the entity they
/// created first to be the one that remains, and an arbitrary choice here would
/// make the same two merges produce different-looking cases.
pub fn merge(
    conn: &mut Connection,
    a: &str,
    b: &str,
    actor: &str,
    rationale: &str,
    candidate_id: Option<&str>,
) -> Result<String> {
    if is_automated(actor) {
        return Err(GraphError::MachineMayNotMerge {
            actor: actor.into(),
        });
    }
    if a == b {
        return Err(GraphError::SelfMerge { entity: a.into() });
    }
    require_entity(conn, a)?;
    require_entity(conn, b)?;

    let left_canonical = canonical_id(conn, a)?;
    let right_canonical = canonical_id(conn, b)?;
    if left_canonical == right_canonical {
        return Err(GraphError::AlreadyMerged {
            entity: a.into(),
            canonical: left_canonical,
        });
    }

    let (survivor, absorbed_canonical) = older_first(conn, &left_canonical, &right_canonical)?;
    let (left, right) = ordered(a, b);
    let decision = new_id()?;
    let now = now_utc();

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO er_decision
            (id, candidate_id, left_entity, right_entity, action, actor, rationale,
             hypothesis_group_id, reverses_id, decided_utc)
         VALUES (?1, ?2, ?3, ?4, 'merge', ?5, ?6, NULL, NULL, ?7)",
        rusqlite::params![decision, candidate_id, left, right, actor, rationale, now],
    )?;

    // Everything in the losing cluster moves, not just the entity named. The
    // members already carry active rows pointing at their old canonical, and
    // migration 7 forbids those rows coexisting with the new ones, so this is
    // deactivate-then-insert rather than a single write.
    let mut members = cluster(&tx, &absorbed_canonical)?;
    // The losing canonical is absorbed as well as its members. Leaving it out
    // would deactivate its cluster and then repoint everything except the
    // entity the analyst actually named, which reads as a merge that half
    // happened.
    members.push(absorbed_canonical.clone());
    tx.execute(
        "UPDATE er_merge_map SET active = 0
          WHERE canonical_id = ?1 AND active = 1",
        [&absorbed_canonical],
    )?;
    for member in members {
        tx.execute(
            "INSERT INTO er_merge_map
                (id, entity_id, canonical_id, decision_id, active, created_utc)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            rusqlite::params![new_id()?, member, survivor, decision, now],
        )?;
    }

    tx.commit()?;
    Ok(decision)
}

/// Record that two entities are *not* the same.
///
/// Open to automated actors, and that asymmetry is the point: deciding two
/// people are different destroys nothing and can be reconsidered at any time,
/// while a wrong merge dissolves the distinction between two real people.
pub fn reject(
    conn: &Connection,
    a: &str,
    b: &str,
    actor: &str,
    rationale: &str,
    candidate_id: Option<&str>,
) -> Result<String> {
    let (left, right) = ordered(a, b);
    let decision = new_id()?;
    conn.execute(
        "INSERT INTO er_decision
            (id, candidate_id, left_entity, right_entity, action, actor, rationale,
             hypothesis_group_id, reverses_id, decided_utc)
         VALUES (?1, ?2, ?3, ?4, 'reject', ?5, ?6, NULL, NULL, ?7)",
        rusqlite::params![
            decision,
            candidate_id,
            left,
            right,
            actor,
            rationale,
            now_utc()
        ],
    )?;
    Ok(decision)
}

/// Take an entity back out of its cluster.
///
/// The merge-map row is deactivated, never deleted, and the decision that made
/// the merge stays exactly where it was. A case can always be asked what it
/// used to believe.
pub fn split(conn: &mut Connection, entity: &str, actor: &str, rationale: &str) -> Result<String> {
    let canonical = canonical_id(conn, entity)?;
    if canonical == entity {
        return Err(GraphError::NotAbsorbed {
            entity: entity.into(),
        });
    }

    let reverses: Option<String> = conn
        .query_row(
            "SELECT decision_id FROM er_merge_map WHERE entity_id = ?1 AND active = 1",
            [entity],
            |r| r.get(0),
        )
        .ok();

    let (left, right) = ordered(entity, &canonical);
    let decision = new_id()?;
    let now = now_utc();

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO er_decision
            (id, candidate_id, left_entity, right_entity, action, actor, rationale,
             hypothesis_group_id, reverses_id, decided_utc)
         VALUES (?1, NULL, ?2, ?3, 'split', ?4, ?5, NULL, ?6, ?7)",
        rusqlite::params![decision, left, right, actor, rationale, reverses, now],
    )?;
    tx.execute(
        "UPDATE er_merge_map SET active = 0 WHERE entity_id = ?1 AND active = 1",
        [entity],
    )?;
    tx.commit()?;
    Ok(decision)
}

/// The entity this one currently resolves to, or itself.
///
/// One indexed lookup, because migration 7 guarantees the map is one level
/// deep. If that invariant ever breaks this returns a stale canonical rather
/// than looping, which is the failure worth having.
pub fn canonical_id(conn: &Connection, entity: &str) -> Result<String> {
    let found: Option<String> = conn
        .query_row(
            "SELECT canonical_id FROM er_merge_map WHERE entity_id = ?1 AND active = 1",
            [entity],
            |r| r.get(0),
        )
        .ok();
    Ok(found.unwrap_or_else(|| entity.to_string()))
}

/// Every entity that currently resolves to this one, excluding itself.
pub fn cluster(conn: &Connection, canonical: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT entity_id FROM er_merge_map
          WHERE canonical_id = ?1 AND active = 1
          ORDER BY entity_id",
    )?;
    let rows = stmt.query_map([canonical], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Every decision touching this entity, newest first.
///
/// The history is the product here, not a debugging aid: an analyst who merged
/// two people in March needs to be able to read why they thought so.
pub fn history(conn: &Connection, entity: &str) -> Result<Vec<(String, String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT action, actor, rationale, decided_utc
           FROM er_decision
          WHERE left_entity = ?1 OR right_entity = ?1
          ORDER BY decided_utc DESC, id DESC",
    )?;
    let rows = stmt.query_map([entity], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn require_entity(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entity WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(GraphError::SubjectMissing {
            kind: "entity",
            id: id.into(),
        })
    }
}

/// Older first, by creation time then insertion order.
///
/// `created_utc` cannot break this on its own. It is a second-resolution ISO
/// string, and the entities an analyst is deciding between were very often
/// created by the same extraction inside the same second — so ties are the
/// normal case, not an edge one.
///
/// The tie goes to `rowid`, SQLite's insertion order. Ordering by entity id
/// instead looks equivalent and is not: ids are random hex, so the survivor
/// would be a coin flip, and the same case rebuilt from the same sequence of
/// merges would end up displaying a different name. That is not a wrong answer
/// the way A-023 was — either cluster is correct — but it is an unreproducible
/// one, and an analyst who merges two records should not have to find out which
/// name won by looking.
fn older_first(conn: &Connection, a: &str, b: &str) -> Result<(String, String)> {
    let key = |id: &str| -> Result<(String, i64)> {
        Ok(conn.query_row(
            "SELECT created_utc, rowid FROM entity WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    };
    if key(a)? <= key(b)? {
        Ok((a.to_string(), b.to_string()))
    } else {
        Ok((b.to_string(), a.to_string()))
    }
}
