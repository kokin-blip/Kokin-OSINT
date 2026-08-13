//! Entity resolution against the real L2 API.
//!
//! Every entity here is created through `create_entity` with real evidence,
//! because the claim under test is that merging is a projection *over* the
//! analytical layer that leaves the analytical layer untouched — and that claim
//! is only meaningful if the rows being left untouched are real ones.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_graph::resolution::{
    canonical_id, cluster, history, merge, open_candidates, propose, reject, split,
};
use kokin_graph::{add_identifier, create_entity, initialise, Grounding, NewEntity};
use rusqlite::Connection;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Case {
    conn: Connection,
    dir: PathBuf,
    observation: String,
}

impl Case {
    fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("kokin-er-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let case_dir = dir.join("case.kokincase");
        let (conn, _paths, _recovery) =
            kokin_store::create_case(&case_dir, "case-er", "passphrase").unwrap();
        initialise(&conn).unwrap();

        // One real piece of evidence for everything here to hang off.
        conn.execute_batch(
            "INSERT INTO source VALUES ('s1','url','https://x/','https://x/','2026-08-12T00:00:00Z');
             INSERT INTO blob VALUES ('h1', 10, x'00', x'01', '2026-08-12T00:00:00Z', NULL);
             INSERT INTO capture VALUES ('c1','s1','h1','2026-08-12T00:00:00Z',200,'{}','{}','[]','static_html_only',1,'http_fetch','j1');
             INSERT INTO artifact VALUES ('a1','h1','text/html',10,'2026-08-12T00:00:00Z');
             INSERT INTO transform_run VALUES ('r1','extract.html','1','v','p','2026-08-12T00:00:00Z',NULL,'succeeded',NULL,NULL);
             INSERT INTO observation VALUES ('o1','a1','r1','page.title','Muller','{}','2026-08-12T00:00:00Z');",
        )
        .unwrap();

        Case {
            conn,
            dir,
            observation: "o1".to_string(),
        }
    }

    fn entity(&mut self, name: &str) -> String {
        let evidence = [Grounding::supporting_observation(&self.observation)];
        create_entity(
            &mut self.conn,
            NewEntity {
                type_key: "person",
                display_name: name,
                notes: "",
            },
            &evidence,
        )
        .unwrap()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// ADR-0008 in one test: a merge changes no entity row, and everything the
/// absorbed entity was grounded in is still there afterwards.
#[test]
fn merging_changes_no_entity_and_destroys_no_evidence() {
    let mut case = Case::new("nondestructive");
    let a = case.entity("Jürgen Müller");
    let b = case.entity("J. Muller");
    let evidence = [Grounding::supporting_observation(&case.observation)];
    add_identifier(
        &mut case.conn,
        &b,
        "email_address",
        "jm@x.example",
        &evidence,
    )
    .unwrap();

    let before: Vec<(String, String)> = snapshot(&case.conn);

    merge(&mut case.conn, &a, &b, "user:local", "same person", None).unwrap();

    assert_eq!(
        snapshot(&case.conn),
        before,
        "a merge rewrote the analytical layer"
    );
    assert_eq!(canonical_id(&case.conn, &b).unwrap(), a);
    assert_eq!(canonical_id(&case.conn, &a).unwrap(), a);
    assert_eq!(cluster(&case.conn, &a).unwrap(), vec![b.clone()]);

    // The absorbed entity's identifier is still its own and still grounded.
    let owner: String = case
        .conn
        .query_row(
            "SELECT entity_id FROM identifier WHERE value = 'jm@x.example'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(owner, b, "the identifier was repointed at the survivor");
}

fn snapshot(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT id, display_name FROM entity ORDER BY id")
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    rows
}

/// The asymmetry the whole design turns on. A machine may argue at length that
/// two entities are one person; it may not conclude it. It *may* conclude they
/// are different, because that destroys nothing.
#[test]
fn a_rule_may_propose_and_reject_but_never_merge() {
    let mut case = Case::new("machine");
    let a = case.entity("Jürgen Müller");
    let b = case.entity("J. Muller");

    propose(&case.conn, &a, &b, "rule:shared_identifier", "same email").unwrap();
    assert_eq!(open_candidates(&case.conn, 10).unwrap().len(), 1);

    for actor in ["rule:shared_identifier", "ai:names", "AI:Names"] {
        let err = merge(&mut case.conn, &a, &b, actor, "looks the same", None).unwrap_err();
        assert!(
            matches!(err, kokin_graph::GraphError::MachineMayNotMerge { .. }),
            "{actor}: {err:?}"
        );
    }

    reject(
        &case.conn,
        &a,
        &b,
        "rule:shared_identifier",
        "different DOB",
        None,
    )
    .unwrap();
    assert!(
        open_candidates(&case.conn, 10).unwrap().is_empty(),
        "a decided pair is still being offered as an open question"
    );
    assert_eq!(canonical_id(&case.conn, &b).unwrap(), b);
}

/// Splitting restores the distinction and keeps the record of having lost it.
#[test]
fn splitting_restores_two_entities_and_keeps_the_history() {
    let mut case = Case::new("split");
    let a = case.entity("Jürgen Müller");
    let b = case.entity("J. Muller");
    merge(&mut case.conn, &a, &b, "user:local", "same person", None).unwrap();

    split(&mut case.conn, &b, "user:local", "different birth years").unwrap();

    assert_eq!(canonical_id(&case.conn, &b).unwrap(), b);
    assert!(cluster(&case.conn, &a).unwrap().is_empty());

    let past = history(&case.conn, &b).unwrap();
    let actions: Vec<&str> = past.iter().map(|(action, ..)| action.as_str()).collect();
    assert!(
        actions.contains(&"merge") && actions.contains(&"split"),
        "splitting erased the merge that preceded it: {actions:?}"
    );

    // And the case can be re-merged afterwards, which is the reactivation path
    // migration 7's third trigger guards.
    merge(
        &mut case.conn,
        &a,
        &b,
        "user:local",
        "the DOB was a typo",
        None,
    )
    .unwrap();
    assert_eq!(canonical_id(&case.conn, &b).unwrap(), a);
}

/// Merging two entities that already have clusters moves every member, not just
/// the two that were named. A half-moved cluster would leave entities resolving
/// to an entity that is itself absorbed — the state migration 7 forbids.
#[test]
fn merging_two_clusters_moves_every_member() {
    let mut case = Case::new("clusters");
    let a = case.entity("Jürgen Müller");
    let b = case.entity("J. Muller");
    let c = case.entity("Jurgen M.");
    let d = case.entity("JM");

    merge(&mut case.conn, &a, &b, "user:local", "same", None).unwrap();
    merge(&mut case.conn, &c, &d, "user:local", "same", None).unwrap();
    // Two clusters: {a: b} and {c: d}. Now join them by naming the *absorbed*
    // members, not the canonicals, which is what an analyst looking at a result
    // list would actually click.
    merge(&mut case.conn, &b, &d, "user:local", "all one person", None).unwrap();

    let survivor = canonical_id(&case.conn, &a).unwrap();
    assert_eq!(survivor, a, "the oldest entity should survive");
    for member in [&b, &c, &d] {
        assert_eq!(
            canonical_id(&case.conn, member).unwrap(),
            a,
            "{member} did not move to the surviving cluster"
        );
    }
    let mut members = cluster(&case.conn, &a).unwrap();
    members.sort();
    let mut expected = vec![b, c, d];
    expected.sort();
    assert_eq!(members, expected);
}

/// Which entity survives must not depend on a coin flip.
///
/// `created_utc` is second-resolution and one extraction creates many entities
/// inside one second, so this tie is ordinary. Entity ids are random hex, so
/// breaking it on id makes the same case, rebuilt from the same merges, display
/// a different name — correct either way and reproducible neither way.
///
/// Ids are forced here because real ones are random: the pipeline produces the
/// failing arrangement about half the time, which is how the two tests above
/// found it, and half the time is not a test.
#[test]
fn the_older_entity_survives_even_when_both_were_created_in_one_second() {
    let case = Case::new("survivor");

    // Two entities, same timestamp, inserted in a known order, with ids that
    // sort *opposite* to that order.
    case.conn
        .execute_batch(
            "INSERT INTO evidence_link VALUES ('elz','entity','zzz-first','observation','o1','supports','2026-08-12T00:00:00Z');
             INSERT INTO entity VALUES ('zzz-first','person','First','','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');
             INSERT INTO evidence_link VALUES ('ela','entity','aaa-second','observation','o1','supports','2026-08-12T00:00:00Z');
             INSERT INTO entity VALUES ('aaa-second','person','Second','','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');",
        )
        .unwrap();

    let mut case = case;
    merge(
        &mut case.conn,
        "aaa-second",
        "zzz-first",
        "user:local",
        "same person",
        None,
    )
    .unwrap();

    assert_eq!(
        canonical_id(&case.conn, "aaa-second").unwrap(),
        "zzz-first",
        "the entity created first did not survive"
    );
}

/// A nightly rule re-raising the same pair must not accumulate a thousand
/// copies of one question, or the queue becomes unusable and the questions stop
/// being read.
#[test]
fn re_proposing_a_pair_updates_it_rather_than_duplicating_it() {
    let mut case = Case::new("dedupe");
    let a = case.entity("Jürgen Müller");
    let b = case.entity("J. Muller");

    let first = propose(&case.conn, &a, &b, "rule:r", "same email").unwrap();
    // The same pair named the other way round is the same question.
    let second = propose(&case.conn, &b, &a, "rule:r", "same email and phone").unwrap();
    assert_eq!(first, second);

    let open = open_candidates(&case.conn, 10).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].rationale, "same email and phone");

    // A different proposer arguing the same pair is a different fact and gets
    // its own row: an analyst should see that two independent things noticed.
    propose(&case.conn, &a, &b, "ai:names", "names are close").unwrap();
    assert_eq!(open_candidates(&case.conn, 10).unwrap().len(), 2);
}

/// Merging an entity into a cluster it is already in is a no-op that must be
/// named rather than silently written, or repeated clicks stack up decisions
/// that all say the same thing.
#[test]
fn merging_something_already_merged_is_refused_by_name() {
    let mut case = Case::new("already");
    let a = case.entity("Jürgen Müller");
    let b = case.entity("J. Muller");
    merge(&mut case.conn, &a, &b, "user:local", "same", None).unwrap();

    let err = merge(&mut case.conn, &b, &a, "user:local", "same", None).unwrap_err();
    assert!(
        matches!(err, kokin_graph::GraphError::AlreadyMerged { .. }),
        "{err:?}"
    );

    let err = split(&mut case.conn, &a, "user:local", "nothing to undo").unwrap_err();
    assert!(
        matches!(err, kokin_graph::GraphError::NotAbsorbed { .. }),
        "{err:?}"
    );
}
