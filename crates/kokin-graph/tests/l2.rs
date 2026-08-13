//! L2 against the real pipeline.
//!
//! Every entity in these tests is grounded in an observation that a real
//! extractor made from a real artifact that a real ingest wrote. Hand-written
//! rows would only prove that L2 works against whatever shape this file
//! imagined, and the claim under test is that increments 6, 7 and 8 compose.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_blob::BlobStore;
use kokin_extract::{extract_artifact, Limits};
use kokin_graph::{
    add_identifier, assess, assessments_for, create_entity, evidence_for, initialise, relate,
    EvidenceKind, EvidenceRole, GraphError, Grounding, NewEntity, Subject, SubjectKind,
};
use kokin_ingest::{ingest_file, IngestContext};
use kokin_keys::CaseMasterKey;
use rusqlite::Connection;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A page with two things on it that are plainly related to each other and
/// plainly not established as related — which is the interesting case.
const PAGE: &str = concat!(
    "<html><head><title>Acme Holdings</title></head>\n",
    "<body><a href=\"mailto:press@ACME.example\">Press</a>\n",
    "<a href=\"https://acme.example/about\">About</a></body></html>"
);

struct Case {
    conn: Connection,
    dir: PathBuf,
    observations: Vec<String>,
}

impl Case {
    fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("kokin-graph-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let case_dir = dir.join("case.kokincase");
        let (mut conn, paths, _recovery) =
            kokin_store::create_case(&case_dir, "case-graph", "passphrase").unwrap();
        let blobs = BlobStore::new(paths.blobs());
        let cmk = CaseMasterKey::generate().unwrap();

        let page = dir.join("page.html");
        std::fs::write(&page, PAGE).unwrap();

        let artifact = ingest_file(
            &mut conn,
            &blobs,
            &cmk,
            &page,
            &IngestContext {
                connector: "file_import",
                job_id: "job-graph",
            },
        )
        .unwrap()
        .artifact_id;

        let extracted =
            extract_artifact(&mut conn, &blobs, &cmk, &artifact, Limits::default()).unwrap();

        Case {
            conn,
            dir,
            observations: extracted.observation_ids,
        }
    }

    /// An observation to ground a row in. Any of them will do; what matters is
    /// that it is a real one with a locator behind it.
    ///
    /// Returns an owned id rather than a `Grounding`, so holding the evidence
    /// does not hold a borrow of the connection it has to be written through.
    fn observation(&self) -> String {
        self.observations[0].clone()
    }

    fn entity(&mut self, type_key: &str, name: &str) -> String {
        let evidence = [Grounding::supporting_observation(&self.observations[0])];
        create_entity(
            &mut self.conn,
            NewEntity {
                type_key,
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

/// The acceptance criterion, end to end: an analytical row exists, and asking
/// it "why" returns the evidence — which is an observation, which carries a
/// locator into an artifact whose bytes are content-addressed.
#[test]
fn an_entity_opens_the_evidence_it_was_built_from() {
    let mut case = Case::new("opens-evidence");
    let observation = case.observations[0].clone();
    let id = case.entity("organisation", "Acme Holdings");

    let evidence = evidence_for(
        &case.conn,
        Subject {
            kind: SubjectKind::Entity,
            id: &id,
        },
    )
    .unwrap();

    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].evidence_kind, "observation");
    assert_eq!(evidence[0].evidence_id, observation);
    assert_eq!(evidence[0].role, "supports");

    // And that observation really does point back into a stored artifact.
    let (kind, locator): (String, String) = case
        .conn
        .query_row(
            "SELECT kind, locator_json FROM observation WHERE id = ?1",
            [&observation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(!kind.is_empty());
    assert!(
        locator.contains("html_byte_range"),
        "the evidence should be locatable, got {locator}"
    );
}

/// ADR-0006's rule, at the API.
#[test]
fn nothing_in_l2_can_be_created_without_evidence() {
    let mut case = Case::new("ungrounded-api");

    let err = create_entity(
        &mut case.conn,
        NewEntity {
            type_key: "person",
            display_name: "Nobody",
            notes: "",
        },
        &[],
    )
    .unwrap_err();
    assert!(matches!(err, GraphError::Ungrounded { .. }), "{err}");

    let entity = case.entity("person", "Somebody");
    let err =
        add_identifier(&mut case.conn, &entity, "email_address", "a@b.test", &[]).unwrap_err();
    assert!(matches!(err, GraphError::Ungrounded { .. }), "{err}");
}

/// The check in this crate is a courtesy; the database is the guarantee.
///
/// Tested by bypassing the crate entirely, because a rule enforced only by the
/// code that happens to call it is not enforced — the next caller writes its
/// own INSERT and the rule is gone.
#[test]
fn the_rule_holds_even_going_around_this_crate() {
    let case = Case::new("bypass");
    let now = kokin_store::now_utc_rfc3339();

    // Seed the registries first, and prove the type really is there.
    //
    // Without this the INSERT below still fails - on the entity_type foreign
    // key, which is the wrong reason. The first version of this test did
    // exactly that and passed with the grounding trigger removed, which is a
    // test that reports a guarantee nobody is providing.
    initialise(&case.conn).unwrap();
    let type_exists: bool = case
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM entity_type WHERE key = 'person')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(type_exists, "the test is not set up to isolate the trigger");

    let refused = case.conn.execute(
        "INSERT INTO entity (id, type_key, display_name, notes, created_utc, updated_utc)
         VALUES ('smuggled', 'person', 'Straight to SQL', '', ?1, ?1)",
        [&now],
    );
    assert!(
        refused.is_err(),
        "an ungrounded entity was inserted by going around the API"
    );

    // And the same row is accepted the moment its evidence exists, so what the
    // trigger objected to was the missing evidence and nothing else.
    case.conn
        .execute(
            "INSERT INTO evidence_link
                (id, subject_kind, subject_id, evidence_kind, evidence_id, role, created_utc)
             VALUES ('el-smuggled', 'entity', 'smuggled', 'observation', ?1, 'supports', ?2)",
            [&case.observations[0], &now],
        )
        .unwrap();
    case.conn
        .execute(
            "INSERT INTO entity (id, type_key, display_name, notes, created_utc, updated_utc)
             VALUES ('smuggled', 'person', 'Straight to SQL', '', ?1, ?1)",
            [&now],
        )
        .unwrap();
}

/// And the rule read backwards: enforcing it only at insert would leave it
/// defeatable one statement later.
#[test]
fn a_live_entity_cannot_be_stripped_of_its_last_evidence() {
    let mut case = Case::new("strip");
    let id = case.entity("organisation", "Acme Holdings");

    let links = evidence_for(
        &case.conn,
        Subject {
            kind: SubjectKind::Entity,
            id: &id,
        },
    )
    .unwrap();

    let refused = case.conn.execute(
        "DELETE FROM evidence_link WHERE id = ?1",
        [&links[0].link_id],
    );
    assert!(refused.is_err(), "an entity outlived its own evidence");
}

/// Evidence that argues *against* a conclusion has somewhere to live. This is
/// the thing a graph-native model cannot represent: there an edge exists or it
/// does not, and the case for its absence has nowhere to go.
#[test]
fn contradicting_evidence_is_recorded_next_to_the_thing_it_contradicts() {
    let mut case = Case::new("contradicts");
    let acme = case.entity("organisation", "Acme Holdings");
    let press = case.entity("email_address", "press@ACME.example");

    let observation = case.observation();
    let supports = Grounding::supporting_observation(&observation);
    let contradicts = Grounding {
        kind: EvidenceKind::Observation,
        id: &case.observations[1],
        role: EvidenceRole::Contradicts,
    };

    let edge = relate(
        &mut case.conn,
        &acme,
        &press,
        "publishes_contact",
        (None, None),
        &[supports, contradicts],
    )
    .unwrap();

    let evidence = evidence_for(
        &case.conn,
        Subject {
            kind: SubjectKind::Relationship,
            id: &edge,
        },
    )
    .unwrap();

    let roles: Vec<&str> = evidence.iter().map(|e| e.role.as_str()).collect();
    assert!(roles.contains(&"supports"));
    assert!(
        roles.contains(&"contradicts"),
        "an edge cannot record the case against itself"
    );
}

/// Seven dimensions, seven rows, and nothing that adds them up.
#[test]
fn confidence_is_recorded_per_dimension_and_never_totalled() {
    let mut case = Case::new("dimensions");
    let id = case.entity("organisation", "Acme Holdings");
    let subject = Subject {
        kind: SubjectKind::Entity,
        id: &id,
    };

    assess(
        &mut case.conn,
        subject,
        "source_reliability",
        "usually_reliable",
        r#"["the company's own site"]"#,
        "user:local",
    )
    .unwrap();
    assess(
        &mut case.conn,
        subject,
        "extraction_certainty",
        "exact",
        r#"["read from a title element"]"#,
        "rule:html_extract",
    )
    .unwrap();
    assess(
        &mut case.conn,
        subject,
        "temporal_consistency",
        "capture_bounded_only",
        r#"["the page carries no date"]"#,
        "rule:html_extract",
    )
    .unwrap();

    let recorded = assessments_for(&case.conn, subject).unwrap();
    assert_eq!(recorded.len(), 3);

    let dimensions: Vec<&str> = recorded.iter().map(|a| a.dimension.as_str()).collect();
    assert_eq!(
        dimensions,
        vec![
            "extraction_certainty",
            "source_reliability",
            "temporal_consistency"
        ]
    );

    // Every one carries its own reasoning, and none carries a total.
    for assessment in &recorded {
        assert!(!assessment.contributing_factors_json.is_empty());
    }
}

/// A rule may argue; only a human may conclude. Until PoC P8 measures the
/// username sweep's false-positive rate, an automated pass may say two
/// identifiers look alike and may not say they belong to the same actor.
#[test]
fn an_automated_actor_cannot_assign_a_value_reserved_for_a_human() {
    let mut case = Case::new("machine-limits");
    let id = case.entity("username", "acmepress");
    let subject = Subject {
        kind: SubjectKind::Entity,
        id: &id,
    };

    let err = assess(
        &mut case.conn,
        subject,
        "identifier_match",
        "controlled_by_same_actor",
        "[]",
        "rule:username_sweep",
    )
    .unwrap_err();
    assert!(
        matches!(err, GraphError::MachineMayNotAssign { .. }),
        "a rule asserted common control: {err}"
    );

    // The same rule may record what it is actually entitled to say,
    assess(
        &mut case.conn,
        subject,
        "identifier_match",
        "similar_unverified",
        r#"["the same handle on 20 platforms"]"#,
        "rule:username_sweep",
    )
    .unwrap();

    // and a human may reach the stronger conclusion.
    assess(
        &mut case.conn,
        subject,
        "identifier_match",
        "controlled_by_same_actor",
        r#"["the domain's own page states this address"]"#,
        "user:local",
    )
    .unwrap();
}

/// The limit is in the database too, not only in this crate.
#[test]
fn the_machine_limit_holds_even_going_around_this_crate() {
    let mut case = Case::new("machine-bypass");
    let id = case.entity("username", "acmepress");
    let now = kokin_store::now_utc_rfc3339();

    let refused = case.conn.execute(
        "INSERT INTO assessment
            (id, subject_kind, subject_id, dimension, scale_version, value_key,
             contributing_factors_json, actor, assessed_utc)
         VALUES ('smuggled','entity',?1,'identifier_match',1,'controlled_by_same_actor','[]','rule:sweep',?2)",
        [&id, &now],
    );
    assert!(
        refused.is_err(),
        "a rule asserted common control by going around the API"
    );
}

/// A value that is not in the scale cannot be written, which is what makes the
/// scale files load-bearing rather than documentation.
#[test]
fn an_invented_scale_value_is_refused_by_name() {
    let mut case = Case::new("invented-value");
    let id = case.entity("organisation", "Acme Holdings");

    let err = assess(
        &mut case.conn,
        Subject {
            kind: SubjectKind::Entity,
            id: &id,
        },
        "source_reliability",
        "pretty_good_actually",
        "[]",
        "user:local",
    )
    .unwrap_err();
    assert!(matches!(err, GraphError::UnknownScaleValue { .. }), "{err}");

    let err = assess(
        &mut case.conn,
        Subject {
            kind: SubjectKind::Entity,
            id: &id,
        },
        "vibes",
        "good",
        "[]",
        "user:local",
    )
    .unwrap_err();
    assert!(matches!(err, GraphError::UnknownScale { .. }), "{err}");
}

/// Reassessment replaces the value; the history lives in the audit chain.
#[test]
fn reassessing_replaces_the_value_and_leaves_the_change_in_the_audit_chain() {
    let mut case = Case::new("reassess");
    let id = case.entity("organisation", "Acme Holdings");
    let subject = Subject {
        kind: SubjectKind::Entity,
        id: &id,
    };

    assess(
        &mut case.conn,
        subject,
        "review_status",
        "unreviewed",
        "[]",
        "user:local",
    )
    .unwrap();
    assess(
        &mut case.conn,
        subject,
        "review_status",
        "disputed",
        r#"["a second source says otherwise"]"#,
        "user:local",
    )
    .unwrap();

    let recorded = assessments_for(&case.conn, subject).unwrap();
    assert_eq!(recorded.len(), 1, "one row per dimension, per ADR-0007");
    assert_eq!(recorded[0].value_key, "disputed");

    // Both readings survive as history, and the chain still verifies.
    let events: i64 = case
        .conn
        .query_row(
            "SELECT COUNT(*) FROM audit_event WHERE action = 'assessment.recorded'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(events, 2, "the earlier reading left no trace");

    assert!(matches!(
        kokin_store::verify_chain(&case.conn).unwrap(),
        kokin_store::ChainStatus::Intact { .. }
    ));
}

/// The case carries what its words meant, so a later build that revises a scale
/// cannot silently restate a judgement already made.
#[test]
fn a_case_carries_the_scale_definitions_its_assessments_were_made_under() {
    let case = Case::new("self-describing");
    initialise(&case.conn).unwrap();

    let (scales, values): (i64, i64) = case
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM scale), (SELECT COUNT(*) FROM scale_value)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(scales, 7);
    assert!(
        values >= 35,
        "expected every value of every scale, got {values}"
    );

    // Including the prose that makes a value mean something.
    let yaml: String = case
        .conn
        .query_row(
            "SELECT source_yaml FROM scale WHERE id = 'identifier_match'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(yaml.contains("calibration_examples"));
    assert!(yaml.contains("threshold_rationale"));
    assert!(yaml.contains("known_failure_modes"));

    // And a case's record of what an assessment meant is not ours to rewrite.
    let refused = case.conn.execute(
        "UPDATE scale SET status = 'active' WHERE id = 'identifier_match'",
        [],
    );
    assert!(refused.is_err(), "a shipped scale version was rewritten");
}

/// Seeding twice must not double the registry or disturb what is there.
#[test]
fn initialising_repeatedly_changes_nothing() {
    let case = Case::new("idempotent");
    initialise(&case.conn).unwrap();

    let before: (i64, i64, i64) = case
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM scale), (SELECT COUNT(*) FROM scale_value),
                    (SELECT COUNT(*) FROM entity_type)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();

    initialise(&case.conn).unwrap();
    initialise(&case.conn).unwrap();

    let after: (i64, i64, i64) = case
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM scale), (SELECT COUNT(*) FROM scale_value),
                    (SELECT COUNT(*) FROM entity_type)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();

    assert_eq!(before, after);
}

/// Normalisation is where false matches are manufactured, so it does as little
/// as it can and the original is always kept beside it.
#[test]
fn normalisation_folds_the_email_domain_and_leaves_the_local_part_alone() {
    let mut case = Case::new("normalise");
    let entity = case.entity("organisation", "Acme Holdings");
    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];

    let id = add_identifier(
        &mut case.conn,
        &entity,
        "email_address",
        "Press@ACME.example",
        &evidence,
    )
    .unwrap();

    let (value, normalized): (String, String) = case
        .conn
        .query_row(
            "SELECT value, normalized FROM identifier WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

    assert_eq!(value, "Press@ACME.example", "the evidence was rewritten");
    assert_eq!(
        normalized, "Press@acme.example",
        "the domain is case-insensitive by specification; the local part is not"
    );
}

/// An unknown type is named rather than silently accepted, because entity_type
/// is a registry and a typo would otherwise create a category of one.
#[test]
fn an_unknown_entity_type_is_refused_by_name() {
    let mut case = Case::new("unknown-type");
    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];

    let err = create_entity(
        &mut case.conn,
        NewEntity {
            type_key: "spaceship",
            display_name: "Acme I",
            notes: "",
        },
        &evidence,
    )
    .unwrap_err();

    assert!(matches!(err, GraphError::UnknownEntityType { .. }), "{err}");
}

/// The whole shape, once: two entities read off one page, an edge between them,
/// and an honest rating of what that edge is actually worth.
#[test]
fn two_things_on_one_page_are_related_at_co_occurrence_and_no_higher() {
    let mut case = Case::new("co-occurrence");
    let acme = case.entity("organisation", "Acme Holdings");
    let press = case.entity("email_address", "press@ACME.example");

    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];
    let edge = relate(
        &mut case.conn,
        &acme,
        &press,
        "mentioned_together",
        (None, None),
        &evidence,
    )
    .unwrap();

    let subject = Subject {
        kind: SubjectKind::Relationship,
        id: &edge,
    };

    // This is all an extractor is entitled to conclude from co-presence, and
    // the point of recording it is that the edge cannot later be mistaken for
    // something stronger.
    assess(
        &mut case.conn,
        subject,
        "relationship_confidence",
        "co_occurrence",
        r#"["both appear on one captured page; nothing states a connection"]"#,
        "rule:html_extract",
    )
    .unwrap();

    let recorded = assessments_for(&case.conn, subject).unwrap();
    assert_eq!(recorded[0].value_key, "co_occurrence");
    assert_eq!(recorded[0].actor, "rule:html_extract");

    // And the edge still opens onto the page it came from.
    assert_eq!(evidence_for(&case.conn, subject).unwrap().len(), 1);
}

/// L2 writes are part of the case's history, not a side channel around it.
#[test]
fn the_audit_chain_records_l2_and_still_verifies() {
    let mut case = Case::new("audit");
    let entity = case.entity("organisation", "Acme Holdings");
    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];
    add_identifier(&mut case.conn, &entity, "domain", "ACME.example", &evidence).unwrap();

    let mut stmt = case
        .conn
        .prepare("SELECT action FROM audit_event ORDER BY id")
        .unwrap();
    let actions: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert!(actions.contains(&"entity.created".to_string()));
    assert!(actions.contains(&"identifier.added".to_string()));

    assert!(matches!(
        kokin_store::verify_chain(&case.conn).unwrap(),
        kokin_store::ChainStatus::Intact { .. }
    ));
}
