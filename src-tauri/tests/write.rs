//! What the interface may put into a case, and what it may not claim about it.
//!
//! These run against a real case on disk for the same reason the read tests do:
//! every claim here is about what four crates and a schema do together, and the
//! interesting failures are the ones where each layer is individually correct.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_osint_lib::{read, write};
use kokin_osint_lib::{
    AssessRequest, CommandError, ErrorCode, ExtractRequest, GroundingInput, IngestFileRequest,
    MergeRequest, NewEntityRequest, NewIdentifierRequest, NewRelationshipRequest, RejectRequest,
    SearchRequest, Session, SplitRequest,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const PASSPHRASE: &str = "correct horse battery staple";
const PAGE: &[u8] = b"<html><title>Tungsten Holdings</title><body><p>Contact press@tungsten.example.</p></body></html>";

/// A case with one document imported and read, and the observation it produced.
struct Fixture {
    dir: PathBuf,
    session: Session,
    artifact: String,
    observation: String,
}

fn fixture(name: &str) -> Fixture {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("kokin-write-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let session = Session::default();
    session
        .create(&dir.join("case.kokincase"), "case-write", PASSPHRASE)
        .unwrap();

    let page = dir.join("page.html");
    std::fs::write(&page, PAGE).unwrap();

    // Through the command layer, not around it: the fixture itself is the first
    // test that ingest and extract work at all.
    let artifact = session
        .with_case(|open| write::ingest_file(open, &ingest_request(&page)))
        .unwrap()
        .artifact_id;
    session
        .with_case(|open| {
            write::extract(
                open,
                &ExtractRequest {
                    artifact_id: artifact.clone(),
                },
            )
        })
        .unwrap();

    let observation = session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT id FROM observation WHERE artifact_id = ?1 ORDER BY rowid LIMIT 1",
                    [&artifact],
                    |r| r.get::<_, String>(0),
                )
                .unwrap())
        })
        .unwrap();

    Fixture {
        dir,
        session,
        artifact,
        observation,
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.session.close();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn ingest_request(path: &std::path::Path) -> IngestFileRequest {
    IngestFileRequest {
        path: path.display().to_string(),
    }
}

/// The common case: one observation, offered as support.
fn cites(observation: &str) -> Vec<GroundingInput> {
    vec![GroundingInput {
        kind: "observation".to_string(),
        id: observation.to_string(),
        role: "supports".to_string(),
    }]
}

fn person(f: &Fixture, name: &str) -> String {
    f.session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "person".to_string(),
                    display_name: name.to_string(),
                    notes: String::new(),
                    evidence: cites(&f.observation),
                },
            )
        })
        .unwrap()
        .id
}

// ---------------------------------------------------------------------------
// The actor
// ---------------------------------------------------------------------------

/// The whole control, stated as the thing an attacker would try.
///
/// `kokin_graph` refuses `rule:` and `ai:` actors, migration 7 refuses them
/// again — and the crate's own note says neither can catch automation writing
/// `user:local`. This boundary is where that becomes reachable, so no request
/// type may carry an actor at all.
#[test]
fn a_request_cannot_name_its_own_actor() {
    // Every write request, as JSON that is otherwise valid, with an actor added.
    let smuggled = [
        (
            "create_entity",
            serde_json::json!({
                "type_key": "person", "display_name": "X",
                "evidence": [{"kind": "observation", "id": "o1"}],
                "actor": "ai:demo"
            }),
        ),
        (
            "add_identifier",
            serde_json::json!({
                "entity_id": "e1", "namespace": "username", "value": "x",
                "evidence": [{"kind": "observation", "id": "o1"}],
                "actor": "ai:demo"
            }),
        ),
        (
            "relate",
            serde_json::json!({
                "from_entity": "e1", "to_entity": "e2", "kind": "knows",
                "evidence": [{"kind": "observation", "id": "o1"}],
                "actor": "ai:demo"
            }),
        ),
        (
            "assess",
            serde_json::json!({
                "subject_kind": "entity", "subject_id": "e1",
                "dimension": "source_reliability", "value_key": "mixed",
                "contributing_factors": ["because"],
                "actor": "ai:demo"
            }),
        ),
        (
            "merge",
            serde_json::json!({
                "left": "e1", "right": "e2", "rationale": "same",
                "actor": "ai:demo"
            }),
        ),
        (
            "split",
            serde_json::json!({"entity": "e1", "rationale": "no", "actor": "ai:demo"}),
        ),
        (
            "reject",
            serde_json::json!({
                "left": "e1", "right": "e2", "rationale": "no",
                "actor": "ai:demo"
            }),
        ),
    ];

    for (name, body) in smuggled {
        let mut without = body.clone();
        without.as_object_mut().unwrap().remove("actor");

        let accepted = match name {
            "create_entity" => serde_json::from_value::<NewEntityRequest>(body).is_ok(),
            "add_identifier" => serde_json::from_value::<NewIdentifierRequest>(body).is_ok(),
            "relate" => serde_json::from_value::<NewRelationshipRequest>(body).is_ok(),
            "assess" => serde_json::from_value::<AssessRequest>(body).is_ok(),
            "merge" => serde_json::from_value::<MergeRequest>(body).is_ok(),
            "split" => serde_json::from_value::<SplitRequest>(body).is_ok(),
            "reject" => serde_json::from_value::<RejectRequest>(body).is_ok(),
            other => panic!("unlisted request type {other}"),
        };
        assert!(!accepted, "{name} accepted a caller-supplied actor");

        // And the same request without it is fine, so the refusal above is about
        // the actor rather than about the request being malformed.
        let ok = match name {
            "create_entity" => serde_json::from_value::<NewEntityRequest>(without).is_ok(),
            "add_identifier" => serde_json::from_value::<NewIdentifierRequest>(without).is_ok(),
            "relate" => serde_json::from_value::<NewRelationshipRequest>(without).is_ok(),
            "assess" => serde_json::from_value::<AssessRequest>(without).is_ok(),
            "merge" => serde_json::from_value::<MergeRequest>(without).is_ok(),
            "split" => serde_json::from_value::<SplitRequest>(without).is_ok(),
            "reject" => serde_json::from_value::<RejectRequest>(without).is_ok(),
            other => panic!("unlisted request type {other}"),
        };
        assert!(ok, "{name} is malformed even without the actor");
    }
}

/// Every actor this case records, after every kind of write, is the person.
#[test]
fn every_write_is_attributed_to_the_person_at_the_keyboard() {
    let f = fixture("actors");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "Mercer, Alex");

    f.session
        .with_case(|open| {
            write::add_identifier(
                open,
                &NewIdentifierRequest {
                    entity_id: a.clone(),
                    namespace: "email_address".to_string(),
                    value: "press@tungsten.example".to_string(),
                    evidence: cites(&f.observation),
                },
            )?;
            write::relate(
                open,
                &NewRelationshipRequest {
                    from_entity: a.clone(),
                    to_entity: b.clone(),
                    kind: "mentioned_with".to_string(),
                    started_utc: None,
                    ended_utc: None,
                    evidence: cites(&f.observation),
                },
            )?;
            write::assess(
                open,
                &AssessRequest {
                    subject_kind: "entity".to_string(),
                    subject_id: a.clone(),
                    dimension: "source_reliability".to_string(),
                    value_key: "usually_reliable".to_string(),
                    contributing_factors: vec!["the company's own contact page".to_string()],
                },
            )?;
            write::merge(
                open,
                &MergeRequest {
                    left: a.clone(),
                    right: b.clone(),
                    rationale: "same address in the filings".to_string(),
                    candidate_id: None,
                },
            )
        })
        .unwrap();

    // Every column in this case that names who did something.
    let actors: Vec<String> = f
        .session
        .with_case(|open| {
            let mut all = Vec::new();
            for sql in [
                "SELECT actor FROM assessment",
                "SELECT actor FROM er_decision",
                "SELECT actor FROM audit_event",
            ] {
                let mut stmt = open.conn.prepare(sql).unwrap();
                let rows = stmt
                    .query_map([], |r| r.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                all.extend(rows);
            }
            Ok(all)
        })
        .unwrap();

    assert!(!actors.is_empty(), "nothing recorded an actor at all");
    for actor in &actors {
        assert_eq!(
            actor,
            write::ACTOR,
            "a write was attributed to {actor}, not to the person"
        );
        assert!(
            !actor.starts_with("rule:") && !actor.starts_with("ai:"),
            "an automated actor reached the case through a command"
        );
    }
}

/// The refusal this layer is keeping out of reach is real, and mapped.
///
/// Called directly against `kokin_graph`, because no command can construct the
/// arguments — which is the point of the two tests above.
#[test]
fn a_machine_is_refused_the_decision_only_a_person_may_make() {
    let f = fixture("machine");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "Mercer, Alex");

    let code = f
        .session
        .with_case(|open| {
            let err = kokin_graph::resolution::merge(
                &mut open.conn,
                &a,
                &b,
                "ai:name_similarity",
                "they look alike",
                None,
            )
            .unwrap_err();
            Ok(CommandError::from(err).code)
        })
        .unwrap();

    assert_eq!(
        code,
        ErrorCode::MachineMayNotAct,
        "a refusal arrived as something the analyst would read as a fault"
    );
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// An ungrounded write is refused, *by this layer*, with something the analyst
/// can act on.
///
/// The code alone does not establish that. `kokin_graph` refuses an ungrounded
/// entity too, and `GraphError::Ungrounded` maps to this same `EvidenceRequired`
/// — so an assertion on the code passes identically whether or not the check in
/// `grounding()` exists at all. Deleting that check was found to break no test,
/// which is how a layer comes to be defended by a suite that never exercised it.
///
/// What is only true of the command layer is *what it can say*. `kokin_graph`
/// reports "nothing in L2 exists without evidence: an entity was created with
/// none" — accurate, and a statement about the schema. This layer knows the
/// caller and can name the field it left out and the kinds that would fill it,
/// which is the entire reason the check is duplicated here (D-033's neighbour in
/// spirit). So the message is asserted, narrowly: it must offer the caller an
/// observation. That is a substring assertion on prose, which D-028 rejects for
/// *branching* and is right to; here it is the only thing that distinguishes the
/// two layers, and the alternative is a test that cannot fail.
#[test]
fn a_write_that_cites_nothing_is_refused() {
    let f = fixture("ungrounded");
    let err = f
        .session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "person".to_string(),
                    display_name: "Nobody".to_string(),
                    notes: String::new(),
                    evidence: vec![],
                },
            )
        })
        .unwrap_err();

    assert_eq!(err.code, ErrorCode::EvidenceRequired);
    assert!(
        err.message.contains("observation"),
        "the refusal did not come from the layer that knows what the caller \
         could have cited, so grounding() is no longer being exercised: {}",
        err.message
    );

    // The same, on the two other paths that share `grounding()`. add_identifier
    // and relate reach different kokin_graph functions, and a check applied to
    // one entry point and not the others is the ordinary way this regresses.
    let entity = person(&f, "Somebody");
    let identifier = f.session.with_case(|open| {
        write::add_identifier(
            open,
            &NewIdentifierRequest {
                entity_id: entity.clone(),
                namespace: "username".to_string(),
                value: "nobody".to_string(),
                evidence: vec![],
            },
        )
    });
    let related = f.session.with_case(|open| {
        write::relate(
            open,
            &NewRelationshipRequest {
                from_entity: entity.clone(),
                to_entity: entity.clone(),
                kind: "knows".to_string(),
                started_utc: None,
                ended_utc: None,
                evidence: vec![],
            },
        )
    });

    for (what, result) in [("add_identifier", identifier), ("relate", related)] {
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::EvidenceRequired, "{what}");
        assert!(
            err.message.contains("observation"),
            "{what} let the layer below do the refusing: {}",
            err.message
        );
    }
}

/// Omitting the field entirely does not quietly become an empty list.
#[test]
fn a_request_that_omits_its_evidence_does_not_deserialise() {
    let without = serde_json::json!({"type_key": "person", "display_name": "Nobody"});
    assert!(serde_json::from_value::<NewEntityRequest>(without).is_err());

    let with = serde_json::json!({
        "type_key": "person", "display_name": "Nobody",
        "evidence": [{"kind": "observation", "id": "o1"}]
    });
    assert!(serde_json::from_value::<NewEntityRequest>(with).is_ok());
}

/// Evidence arguing against a conclusion has somewhere to live, end to end.
///
/// Checked on an entity through the read command, and on an identifier straight
/// against the case — because `EntityView.evidence` is entity-scoped, so an
/// identifier's own evidence is stored correctly and displayed nowhere. That is
/// a gap in the reads rather than in these writes, and this is where it is
/// visible as a fact instead of an omission.
#[test]
fn evidence_may_argue_against_the_thing_it_is_attached_to() {
    let f = fixture("contradicts");

    let contradicting = vec![GroundingInput {
        kind: "observation".to_string(),
        id: f.observation.clone(),
        role: "contradicts".to_string(),
    }];

    let entity = f
        .session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "person".to_string(),
                    display_name: "A. Mercer".to_string(),
                    notes: "the page argues this is not the same Mercer".to_string(),
                    evidence: contradicting.clone(),
                },
            )
        })
        .unwrap()
        .id;

    let view = f
        .session
        .with_case(|open| read::entity(open, &entity))
        .unwrap();
    let roles: Vec<&str> = view.evidence.iter().map(|e| e.role.as_str()).collect();
    assert_eq!(
        roles,
        vec!["contradicts"],
        "the role was flattened between the request and the case"
    );

    let identifier = f
        .session
        .with_case(|open| {
            write::add_identifier(
                open,
                &NewIdentifierRequest {
                    entity_id: entity.clone(),
                    namespace: "username".to_string(),
                    value: "amercer".to_string(),
                    evidence: contradicting,
                },
            )
        })
        .unwrap()
        .id;

    let role: String = f
        .session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT role FROM evidence_link
                      WHERE subject_kind = 'identifier' AND subject_id = ?1",
                    [&identifier],
                    |r| r.get(0),
                )
                .unwrap())
        })
        .unwrap();
    assert_eq!(role, "contradicts");
}

/// An analytical row is never evidence for another analytical row.
#[test]
fn an_entity_cannot_be_offered_as_evidence() {
    let f = fixture("selfsupport");
    let entity = person(&f, "A. Mercer");

    let err = f
        .session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "person".to_string(),
                    display_name: "Derived".to_string(),
                    notes: String::new(),
                    evidence: vec![GroundingInput {
                        kind: "entity".to_string(),
                        id: entity.clone(),
                        role: "supports".to_string(),
                    }],
                },
            )
        })
        .unwrap_err();

    assert_eq!(err.code, ErrorCode::InvalidValue);
}

// ---------------------------------------------------------------------------
// A judgement states its reasons
// ---------------------------------------------------------------------------

#[test]
fn a_judgement_with_no_stated_reason_is_refused() {
    let f = fixture("unexplained");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "Mercer, Alex");

    let assessed = |factors: Vec<String>| {
        f.session.with_case(|open| {
            write::assess(
                open,
                &AssessRequest {
                    subject_kind: "entity".to_string(),
                    subject_id: a.clone(),
                    dimension: "source_reliability".to_string(),
                    value_key: "mixed".to_string(),
                    contributing_factors: factors,
                },
            )
        })
    };

    assert_eq!(
        assessed(vec![]).unwrap_err().code,
        ErrorCode::ExplanationRequired
    );
    // Whitespace is not a reason.
    assert_eq!(
        assessed(vec!["   ".to_string()]).unwrap_err().code,
        ErrorCode::ExplanationRequired
    );
    assert!(assessed(vec!["the contact page".to_string()]).is_ok());

    let merged = f.session.with_case(|open| {
        write::merge(
            open,
            &MergeRequest {
                left: a.clone(),
                right: b.clone(),
                rationale: "  ".to_string(),
                candidate_id: None,
            },
        )
    });
    assert_eq!(merged.unwrap_err().code, ErrorCode::ExplanationRequired);
}

/// What the analyst typed is what the case holds, as structure rather than text.
#[test]
fn the_reasons_given_are_what_the_case_stores() {
    let f = fixture("reasons");
    let entity = person(&f, "A. Mercer");

    f.session
        .with_case(|open| {
            write::assess(
                open,
                &AssessRequest {
                    subject_kind: "entity".to_string(),
                    subject_id: entity.clone(),
                    dimension: "source_reliability".to_string(),
                    value_key: "usually_reliable".to_string(),
                    contributing_factors: vec![
                        "the company's own contact page".to_string(),
                        "no contradicting source".to_string(),
                    ],
                },
            )
        })
        .unwrap();

    let view = f
        .session
        .with_case(|open| read::entity(open, &entity))
        .unwrap();
    let dimension = view
        .confidence
        .iter()
        .find(|d| d.dimension == "source_reliability")
        .expect("the dimension is missing entirely");
    let stored = &dimension.values[0].contributing_factors_json;

    let parsed: Vec<String> = serde_json::from_str(stored)
        .expect("the why panel would be reading something that is not JSON");
    assert_eq!(
        parsed,
        vec![
            "the company's own contact page".to_string(),
            "no contradicting source".to_string()
        ]
    );
}

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

/// The document the case holds is the document that was on disk, and the case
/// can say where it came from.
///
/// Both halves matter and only the first is obvious. An ingest that writes the
/// bytes and skips a provenance row produces a case that answers "what does this
/// say" perfectly and "how do you know" not at all — and since every read path in
/// this product goes through the artifact, nothing else would notice. So this
/// walks the L0 chain the command claims to have written: the source it came
/// from, the capture that fetched it, the artifact those produced, and the L1
/// edge recording that the artifact was derived from the capture.
#[test]
fn an_imported_file_arrives_whole_and_with_its_provenance() {
    let f = fixture("import");

    // A second file rather than the fixture's, so the ids under test come from
    // an ingest this test can see the whole return value of.
    const OTHER: &[u8] =
        b"<html><title>Ridgeway Trust</title><body><p>Filed 2019.</p></body></html>";
    let path = f.dir.join("other.html");
    std::fs::write(&path, OTHER).unwrap();

    let ingested = f
        .session
        .with_case(|open| write::ingest_file(open, &ingest_request(&path)))
        .unwrap();
    assert!(
        !ingested.deduplicated,
        "distinct bytes were reported as a document the case already held"
    );

    let rows: Vec<(&str, i64)> = f
        .session
        .with_case(|open| {
            let mut found = Vec::new();
            for (table, id) in [
                ("source", &ingested.source_id),
                ("capture", &ingested.capture_id),
                ("artifact", &ingested.artifact_id),
                ("transform_run", &ingested.run_id),
            ] {
                let n: i64 = open
                    .conn
                    .query_row(
                        &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
                        [id],
                        |r| r.get(0),
                    )
                    .unwrap();
                found.push((table, n));
            }
            Ok(found)
        })
        .unwrap();
    assert_eq!(
        rows,
        vec![
            ("source", 1),
            ("capture", 1),
            ("artifact", 1),
            ("transform_run", 1)
        ],
        "the ingest returned an id for a row it did not write"
    );

    // The chain has to join up, not merely exist. A capture attributed to the
    // wrong source is provenance that points at another document.
    let (capture_source, capture_hash, artifact_hash): (String, String, String) = f
        .session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT c.source_id, c.content_hash, a.content_hash
                       FROM capture c JOIN artifact a ON a.id = ?2
                      WHERE c.id = ?1",
                    [&ingested.capture_id, &ingested.artifact_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap())
        })
        .unwrap();
    assert_eq!(capture_source, ingested.source_id);
    assert_eq!(capture_hash, ingested.content_hash);
    assert_eq!(artifact_hash, ingested.content_hash);

    let edges: i64 = f
        .session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM derivation
                      WHERE run_id = ?1 AND input_kind = 'capture' AND input_id = ?2
                        AND output_kind = 'artifact' AND output_id = ?3",
                    [
                        &ingested.run_id,
                        &ingested.capture_id,
                        &ingested.artifact_id,
                    ],
                    |r| r.get(0),
                )
                .unwrap())
        })
        .unwrap();
    assert_eq!(
        edges, 1,
        "the artifact exists with no recorded lineage back to the capture"
    );

    for (artifact, expected) in [(&f.artifact, PAGE), (&ingested.artifact_id, OTHER)] {
        let doc = f
            .session
            .with_case(|open| read::artifact_document(open, artifact))
            .unwrap();
        match doc.content {
            kokin_osint_lib::DocumentContent::Readable { text, .. } => {
                assert_eq!(text.as_bytes(), expected);
            }
            other => panic!("the document did not come back readable: {other:?}"),
        }
    }
}

/// The same bytes twice is an investigative fact, and the command says so.
#[test]
fn the_same_document_twice_is_reported_as_the_same_document() {
    let f = fixture("dedup");
    let again = f.dir.join("again.html");
    std::fs::write(&again, PAGE).unwrap();

    let second = f
        .session
        .with_case(|open| write::ingest_file(open, &ingest_request(&again)))
        .unwrap();

    assert!(
        second.deduplicated,
        "the case did not notice it already held these bytes"
    );
    let first_hash = f
        .session
        .with_case(|open| read::artifact_document(open, &f.artifact))
        .unwrap()
        .content_hash;
    assert_eq!(second.content_hash, first_hash);
}

#[test]
fn a_case_cannot_ingest_itself() {
    let f = fixture("selfingest");
    let inside = f
        .session
        .with_case(|open| Ok(open.paths.root.join("swallow.html")))
        .unwrap();
    std::fs::write(&inside, PAGE).unwrap();

    let err = f
        .session
        .with_case(|open| write::ingest_file(open, &ingest_request(&inside)))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::FileUnreadable);
}

/// An extraction reports what it could not read, in the same payload.
#[test]
fn an_extraction_reports_the_values_it_had_to_drop() {
    let f = fixture("oversize");

    // One href past max_value_bytes (8 KiB), so the extractor records the rest
    // of the page and drops this.
    let long = "A".repeat(9_000);
    let page = f.dir.join("long.html");
    std::fs::write(
        &page,
        format!("<html><title>Long</title><body><a href=\"https://x.example/{long}\">x</a><p>ordinary prose</p></body></html>"),
    )
    .unwrap();

    let artifact = f
        .session
        .with_case(|open| write::ingest_file(open, &ingest_request(&page)))
        .unwrap()
        .artifact_id;
    let extracted = f
        .session
        .with_case(|open| {
            write::extract(
                open,
                &ExtractRequest {
                    artifact_id: artifact,
                },
            )
        })
        .unwrap();

    assert!(
        extracted.skipped_oversize > 0,
        "the oversize value was not reported"
    );
    assert!(
        !extracted.complete,
        "a partly-read document reported itself as fully read"
    );

    // And the ordinary case is honestly complete, so `complete` is a measurement
    // rather than a constant.
    let plain = f
        .session
        .with_case(|open| {
            write::extract(
                open,
                &ExtractRequest {
                    artifact_id: f.artifact.clone(),
                },
            )
        })
        .unwrap();
    assert!(plain.complete);
}

/// Reading a document again withdraws what the earlier reading said.
#[test]
fn re_reading_a_document_withdraws_what_the_last_reading_said() {
    let f = fixture("rerun");
    let again = f
        .session
        .with_case(|open| {
            write::extract(
                open,
                &ExtractRequest {
                    artifact_id: f.artifact.clone(),
                },
            )
        })
        .unwrap();

    assert!(
        again.superseded > 0,
        "a second reading left the first one's observations standing"
    );
}

/// A document the extractor cannot read has not damaged anything.
#[test]
fn a_document_this_extractor_cannot_read_is_not_a_damaged_case() {
    let f = fixture("unsupported");
    let notes = f.dir.join("notes.txt");
    std::fs::write(&notes, b"plain text, not markup").unwrap();

    let artifact = f
        .session
        .with_case(|open| write::ingest_file(open, &ingest_request(&notes)))
        .unwrap()
        .artifact_id;

    let err = f
        .session
        .with_case(|open| {
            write::extract(
                open,
                &ExtractRequest {
                    artifact_id: artifact.clone(),
                },
            )
        })
        .unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::ExtractionFailed,
        "an unreadable document was reported as a storage failure"
    );

    // The bytes are untouched. One derivation did not happen; nothing was lost.
    let doc = f
        .session
        .with_case(|open| read::artifact_document(open, &artifact))
        .unwrap();
    assert!(matches!(
        doc.content,
        kokin_osint_lib::DocumentContent::Readable { .. }
    ));
}

// ---------------------------------------------------------------------------
// The index nobody has to remember
// ---------------------------------------------------------------------------

/// A written row is searchable without any command asking for it to be indexed.
#[test]
fn what_a_write_records_is_searchable_at_once() {
    let f = fixture("indexed");
    person(&f, "Wolfram Kestrel");

    let hits = f
        .session
        .with_case(|open| {
            read::search(
                open,
                &SearchRequest {
                    query: "kestrel".to_string(),
                    ..Default::default()
                },
            )
        })
        .unwrap();

    assert!(
        hits.hits.iter().any(|h| h.subject_kind == "entity"),
        "a newly written entity is not findable: {:?}",
        hits.hits
    );
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

#[test]
fn a_merge_returns_the_entity_the_interface_should_now_show() {
    let f = fixture("merge");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "Mercer, Alex");

    let merged = f
        .session
        .with_case(|open| {
            write::merge(
                open,
                &MergeRequest {
                    left: a.clone(),
                    right: b.clone(),
                    rationale: "same address in the filings".to_string(),
                    candidate_id: None,
                },
            )
        })
        .unwrap();

    assert!(merged.canonical_id == a || merged.canonical_id == b);
    let absorbed = if merged.canonical_id == a { &b } else { &a };

    let view = f
        .session
        .with_case(|open| read::entity(open, absorbed))
        .unwrap();
    assert!(view.redirected);
    assert_eq!(view.entity_id, merged.canonical_id);
}

#[test]
fn splitting_gives_an_entity_back_its_own_identity() {
    let f = fixture("split");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "Mercer, Alex");

    let merged = f
        .session
        .with_case(|open| {
            write::merge(
                open,
                &MergeRequest {
                    left: a.clone(),
                    right: b.clone(),
                    rationale: "same address".to_string(),
                    candidate_id: None,
                },
            )
        })
        .unwrap();
    let absorbed = if merged.canonical_id == a {
        b.clone()
    } else {
        a.clone()
    };

    let split = f
        .session
        .with_case(|open| {
            write::split(
                open,
                &SplitRequest {
                    entity: absorbed.clone(),
                    rationale: "different birth years".to_string(),
                },
            )
        })
        .unwrap();

    assert_eq!(split.canonical_id, absorbed);
    let view = f
        .session
        .with_case(|open| read::entity(open, &absorbed))
        .unwrap();
    assert!(!view.redirected);
}

/// Two states of the merge map that are not failures, and do not read like one.
#[test]
fn the_resolution_refusals_are_answers_rather_than_faults() {
    let f = fixture("refusals");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "Mercer, Alex");

    f.session
        .with_case(|open| {
            write::merge(
                open,
                &MergeRequest {
                    left: a.clone(),
                    right: b.clone(),
                    rationale: "same address".to_string(),
                    candidate_id: None,
                },
            )
        })
        .unwrap();

    let again = f
        .session
        .with_case(|open| {
            write::merge(
                open,
                &MergeRequest {
                    left: a.clone(),
                    right: b.clone(),
                    rationale: "same address".to_string(),
                    candidate_id: None,
                },
            )
        })
        .unwrap_err();
    assert_eq!(again.code, ErrorCode::AlreadyMerged);

    let canonical = f
        .session
        .with_case(|open| Ok(kokin_graph::resolution::canonical_id(&open.conn, &a)?))
        .unwrap();
    let never = f
        .session
        .with_case(|open| {
            write::split(
                open,
                &SplitRequest {
                    entity: canonical.clone(),
                    rationale: "nothing to undo".to_string(),
                },
            )
        })
        .unwrap_err();
    assert_eq!(never.code, ErrorCode::NotMerged);
}

/// A rejection records a decision without changing what resolves to what.
#[test]
fn a_rejection_is_recorded_and_changes_nothing_else() {
    let f = fixture("reject");
    let a = person(&f, "A. Mercer");
    let b = person(&f, "A. Merser");

    let decision = f
        .session
        .with_case(|open| {
            write::reject(
                open,
                &RejectRequest {
                    left: a.clone(),
                    right: b.clone(),
                    rationale: "different employers".to_string(),
                    candidate_id: None,
                },
            )
        })
        .unwrap();
    assert_eq!(decision.actor, write::ACTOR);

    let view = f.session.with_case(|open| read::entity(open, &a)).unwrap();
    assert!(!view.redirected, "a rejection moved an entity");
    assert!(
        view.history.iter().any(|d| d.action == "reject"),
        "the rejection is not in the entity's history"
    );
}

/// A write against a case nobody opened is refused, not attempted.
#[test]
fn a_write_needs_an_open_case() {
    let session = Session::default();
    let err = session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "person".to_string(),
                    display_name: "X".to_string(),
                    notes: String::new(),
                    evidence: vec![],
                },
            )
        })
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NoCaseOpen);
}
