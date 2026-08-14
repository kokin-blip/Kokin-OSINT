//! The gate-5 acceptance test: all eleven reference-workflow steps, offline.
//!
//! The authoritative numbering is `docs/testing/reference-workflow.md`, and the
//! step function names here are what that file's table points at. If a step is
//! renumbered there and not here, `docs/testing/acceptance-plan.csv` is the
//! thing that goes stale, so the three move together or not at all.
//!
//! # Why this drives the command layer
//!
//! Where a Tauri command exists, this test calls it rather than the crate
//! underneath. That is the whole point: the crates have had 312 tests between
//! them and every one of those tests is free to reach past the boundary the
//! interface will actually be standing behind. A workflow that passes only when
//! driven from inside the workspace is not the workflow a user performs.
//!
//! Two steps have no command yet and are driven directly, and both are recorded
//! as gaps rather than smoothed over:
//!
//! - **Step 2** — `ingest_url` is not exposed, because `src-tauri` takes no
//!   `kokin-net` dependency and there is no live transport to hand it (increment
//!   17's known limitation 2).
//! - **Step 10** — `kokin-export` has no command layer at all (increment 18's
//!   known limitation 4).
//!
//! # Steps 1-10 run twice
//!
//! Step 11 is not a twelfth thing to do. `walk_the_workflow` is called once, and
//! then again into a fresh directory, and the two summaries must be equal. Under
//! `ReplayHttp` a fixture miss is an error rather than a live request, so a pass
//! is what establishes that steps 2 through 10 never touched the network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_export::{PackageOptions, Verdict};
use kokin_net::http::{Fixture, HttpRequest, HttpResponse, ReplayHttp};
use kokin_osint_lib::{
    read, write, AssessRequest, GroundingInput, NewEntityRequest, NewRelationshipRequest,
    SearchRequest, Session,
};
use kokin_store::OpenCase;

static COUNTER: AtomicU32 = AtomicU32::new(0);

const CASE_ID: &str = "acceptance";
const PASSPHRASE: &str = "correct horse battery staple";
const EXPORT_PASSPHRASE: &str = "a different phrase entirely";
const URL: &str = "https://tungsten.example/contact";

/// The page step 2 collects.
///
/// Committed at `tests/fixtures/acceptance/`, which is the directory the CI
/// secret-hygiene job greps, so a fixture that ever gains a cookie or a token
/// fails the build.
fn page() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/acceptance/tungsten-holdings.html");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the acceptance fixture is missing at {path:?}: {e}"))
}

/// Wrap the committed page into the form `ReplayHttp` serves.
///
/// **This fixture is synthetic, not recorded**, and that distinction is written
/// down in `docs/testing/reference-workflow.md` rather than left to be noticed.
/// Nothing in this workspace can record a fixture, because `HttpCapability` has
/// one implementation and it is the replaying one. `RecordingHttp` arrives with
/// `LiveHttp` in increment 26, and this comment comes out then.
fn install_fixture(dir: &Path) -> ReplayHttp {
    let http = ReplayHttp::new(dir);
    let request = HttpRequest::get(URL);
    let mut headers = BTreeMap::new();
    headers.insert(
        "content-type".to_string(),
        "text/html; charset=utf-8".to_string(),
    );

    http.store(&Fixture {
        key: request.fixture_key().unwrap(),
        request,
        response: HttpResponse {
            status: 200,
            headers,
            body: page(),
            from_fixture: true,
        },
        recorded_utc: "2026-08-14T00:00:00Z".to_string(),
        note: Some("synthetic: no transport exists to record from (increment 19)".to_string()),
    })
    .unwrap();
    http
}

/// What a walk of the workflow produced, for comparing one walk against another.
///
/// Ids are deliberately absent: they are random per run, so including them would
/// make two correct walks unequal. What must match is the *shape* of what the
/// case ended up holding.
#[derive(Debug, PartialEq, Eq)]
struct Summary {
    observations: i64,
    entities: i64,
    relationships: i64,
    evidence_links: i64,
    search_hits: usize,
    audit_events_intact: bool,
    export_verdict_ok: bool,
    document_bytes: usize,
}

#[test]
fn the_eleven_step_reference_workflow_completes_offline() {
    let first = walk_the_workflow("first");

    // Step 11. Not a repetition for its own sake: the second walk proves the
    // first was reproducible and that neither touched the network, because a
    // fixture miss under ReplayHttp is a failure rather than a request.
    let second = walk_the_workflow("replay");

    assert_eq!(
        first, second,
        "the same workflow over the same fixture produced two different cases"
    );
    assert!(first.audit_events_intact);
    assert!(first.export_verdict_ok);
}

fn walk_the_workflow(label: &str) -> Summary {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "kokin-acceptance-{label}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let http = install_fixture(&dir.join("fixtures"));
    let case = dir.join("case.kokincase");
    let session = Session::default();

    // -----------------------------------------------------------------------
    // Step 1 — create an encrypted local case
    // -----------------------------------------------------------------------
    let created = session.create(&case, CASE_ID, PASSPHRASE).unwrap();
    assert_eq!(created.case.case_id, CASE_ID);
    assert!(
        !created.recovery_key.is_empty(),
        "the recovery key is returned exactly once and this was the once"
    );
    assert!(case.join("header.json").exists());
    assert!(
        !std::fs::read(case.join("case.db"))
            .unwrap()
            .starts_with(b"SQLite format 3\0"),
        "the case database is not encrypted"
    );

    // -----------------------------------------------------------------------
    // Step 2 — ingest one URL, preserving the source it came from
    // Step 3 — preserve integrity metadata
    // -----------------------------------------------------------------------
    let ingested = session
        .with_case(|open| {
            Ok(kokin_ingest::ingest_url(
                &mut open.conn,
                &kokin_blob::BlobStore::new(open.paths.blobs()),
                &http,
                &open.cmk,
                URL,
                &kokin_ingest::IngestContext {
                    connector: "http_fetch",
                    job_id: "acceptance",
                },
            )
            .unwrap())
        })
        .unwrap();

    assert!(
        ingested.from_fixture,
        "a replayed capture must say so rather than claim it was fetched"
    );

    let (canonical, completeness, capture_source): (String, String, String) = session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT s.canonical_locator, c.capture_completeness, c.source_id
                       FROM capture c JOIN source s ON s.id = c.source_id
                      WHERE c.id = ?1",
                    [&ingested.capture_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap())
        })
        .unwrap();
    assert!(canonical.contains("tungsten.example"), "{canonical}");
    assert_eq!(capture_source, ingested.source_id);
    // A partial snapshot must never be mistaken for a full one.
    assert!(!completeness.is_empty());

    // Step 3 proper: the integrity metadata is checkable after the fact, not
    // merely present. Recompute the hash from the bytes the case stored.
    let hash_of_page = blake3::hash(page().as_bytes()).to_hex().to_string();
    assert_eq!(
        ingested.content_hash, hash_of_page,
        "the stored content hash does not describe the collected bytes"
    );

    // -----------------------------------------------------------------------
    // Step 4 — extract normalised observations
    // -----------------------------------------------------------------------
    let extracted = session
        .with_case(|open| {
            write::extract(
                open,
                &kokin_osint_lib::ExtractRequest {
                    artifact_id: ingested.artifact_id.clone(),
                },
            )
        })
        .unwrap();
    assert!(
        extracted.observations > 0,
        "the page produced no observations"
    );

    let (observation, locator): (String, String) = session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT id, locator_json FROM observation
                      WHERE artifact_id = ?1 ORDER BY rowid LIMIT 1",
                    [&ingested.artifact_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap())
        })
        .unwrap();
    assert!(
        locator.trim() != "" && locator.trim() != "{}",
        "an observation with no locator cannot be traced back into the document"
    );

    // -----------------------------------------------------------------------
    // Step 5 — create candidate entities and a relationship
    // -----------------------------------------------------------------------
    let evidence = vec![GroundingInput {
        kind: "observation".to_string(),
        id: observation.clone(),
        role: "supports".to_string(),
    }];

    let person = session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "person".to_string(),
                    display_name: "A. Mercer".to_string(),
                    notes: String::new(),
                    evidence: evidence.clone(),
                },
            )
        })
        .unwrap()
        .id;

    let org = session
        .with_case(|open| {
            write::create_entity(
                open,
                &NewEntityRequest {
                    type_key: "organisation".to_string(),
                    display_name: "Tungsten Holdings Ltd".to_string(),
                    notes: String::new(),
                    evidence: evidence.clone(),
                },
            )
        })
        .unwrap()
        .id;

    let relationship = session
        .with_case(|open| {
            write::relate(
                open,
                &NewRelationshipRequest {
                    from_entity: person.clone(),
                    to_entity: org.clone(),
                    kind: "affiliated_with".to_string(),
                    started_utc: None,
                    ended_utc: None,
                    evidence: evidence.clone(),
                },
            )
        })
        .unwrap()
        .id;

    // -----------------------------------------------------------------------
    // Step 6 — the relationship opens its supporting evidence
    //
    // The acceptance criterion is "graph edges open their supporting evidence",
    // and it fails if a relationship exists whose evidence cannot be reached
    // *from the relationship*. Driven through kokin_graph rather than a command
    // because the command layer has no relationship read yet (increment 16,
    // known limitation 6).
    // -----------------------------------------------------------------------
    let backing = session
        .with_case(|open| {
            Ok(kokin_graph::evidence_for(
                &open.conn,
                kokin_graph::Subject {
                    kind: kokin_graph::SubjectKind::Relationship,
                    id: &relationship,
                },
            )
            .unwrap())
        })
        .unwrap();
    assert!(
        !backing.is_empty(),
        "the relationship is presented as fact with no reachable evidence"
    );
    assert!(
        backing.iter().any(|e| e.evidence_id == observation),
        "the evidence behind the edge is not the observation it was created from"
    );

    // And the evidence leads somewhere a person can read: the document the
    // observation came from, still decryptable and byte-identical.
    let document = session
        .with_case(|open| read::artifact_document(open, &ingested.artifact_id))
        .unwrap();
    let document_bytes = match document.content {
        kokin_osint_lib::DocumentContent::Readable { ref text, .. } => {
            assert_eq!(text.as_str(), page(), "the document changed in storage");
            text.len()
        }
        ref other => panic!("the evidence behind the edge is unreadable: {other:?}"),
    };

    // -----------------------------------------------------------------------
    // Step 7 — a person accepts it
    // -----------------------------------------------------------------------
    // How strong the evidence is, and separately whether a person has signed off
    // on it. Two dimensions rather than one, because ADR-0007 exists to stop
    // "the source says so" and "I checked it" collapsing into a single number.
    for (dimension, value, factor) in [
        (
            "relationship_confidence",
            "stated_by_source",
            "the organisation's own contact page names them as a director",
        ),
        (
            "review_status",
            "accepted",
            "followed the locator to the artifact and confirmed the value",
        ),
    ] {
        let recorded = session
            .with_case(|open| {
                write::assess(
                    open,
                    &AssessRequest {
                        subject_kind: "relationship".to_string(),
                        subject_id: relationship.clone(),
                        dimension: dimension.to_string(),
                        value_key: value.to_string(),
                        contributing_factors: vec![factor.to_string()],
                    },
                )
            })
            .unwrap();
        assert_eq!(
            recorded.actor, "user:local",
            "an acceptance must be attributable to the person who made it (D-032)"
        );
    }

    // The review_status scale does not merely permit an audit event for
    // `accepted` — it defines the value as requiring one, and marks it
    // machine_assignable: false. An acceptance nobody can attribute is not an
    // acceptance, so the step is not passed until the chain says who.
    let attributed: i64 = session
        .with_case(|open| {
            Ok(open
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM audit_event
                      WHERE subject_id = ?1 AND actor = 'user:local'",
                    [&relationship],
                    |r| r.get(0),
                )
                .unwrap())
        })
        .unwrap();
    assert!(
        attributed > 0,
        "the acceptance left no attributable trace in the audit chain"
    );

    // -----------------------------------------------------------------------
    // Step 8 — the case is searchable, and its history is readable
    // -----------------------------------------------------------------------
    let found = session
        .with_case(|open| {
            read::search(
                open,
                &SearchRequest {
                    query: "Mercer".to_string(),
                    limit: Some(50),
                    offset: 0,
                    kinds: Vec::new(),
                    facets: Vec::new(),
                    include_superseded: false,
                    resolve_merged: None,
                    typeahead: false,
                },
            )
        })
        .unwrap();
    assert!(!found.hits.is_empty(), "the case cannot find what it holds");
    // D-029: the caveat travels with the results, and is not an Option the
    // interface can forget to fetch.
    assert!(found.coverage.artifacts > 0);

    let audit_events_intact = session
        .with_case(|open| {
            Ok(matches!(
                kokin_store::verify_chain(&open.conn).unwrap(),
                kokin_store::ChainStatus::Intact { .. }
            ))
        })
        .unwrap();
    assert!(audit_events_intact, "the activity history does not verify");

    let before = summarise(
        &session,
        &found.hits.len(),
        audit_events_intact,
        document_bytes,
    );

    // -----------------------------------------------------------------------
    // Step 9 — close and reopen with no data loss
    // -----------------------------------------------------------------------
    session.close().unwrap();
    session.open(&case, PASSPHRASE).unwrap();

    let after = summarise(
        &session,
        &found.hits.len(),
        audit_events_intact,
        document_bytes,
    );
    assert_eq!(
        before.observations, after.observations,
        "observations were lost across a close and reopen"
    );
    assert_eq!(before.entities, after.entities);
    assert_eq!(before.relationships, after.relationships);
    assert_eq!(before.evidence_links, after.evidence_links);

    // The entity is still readable by the id held from before the close.
    let entity = session
        .with_case(|open| read::entity(open, &person))
        .unwrap();
    assert_eq!(entity.display_name, "A. Mercer");
    assert_eq!(
        entity.confidence.len(),
        7,
        "all seven dimensions are always present (D-031)"
    );

    // -----------------------------------------------------------------------
    // Step 10 — export and restore
    // -----------------------------------------------------------------------
    let package_path = dir.join("case.kokinpkg");
    session
        .with_case(|open| {
            kokin_export::package(
                open,
                &package_path,
                EXPORT_PASSPHRASE,
                &PackageOptions::default(),
            )
            .unwrap();
            Ok(())
        })
        .unwrap();

    // Verified with no passphrase: integrity is checkable by someone who cannot
    // open the case.
    let verified = kokin_export::verify(&package_path).unwrap();
    let export_verdict_ok = verified.verdict == Verdict::Ok;
    assert!(
        export_verdict_ok,
        "the package did not verify: {:?}",
        verified.problems
    );

    let restored_root = dir.join("restored.kokincase");
    kokin_export::import(&package_path, &restored_root, EXPORT_PASSPHRASE).unwrap();

    session.close().unwrap();
    session.open(&restored_root, EXPORT_PASSPHRASE).unwrap();
    let restored = session
        .with_case(|open| read::entity(open, &person))
        .unwrap();
    assert_eq!(
        restored.display_name, "A. Mercer",
        "the restored case lost the entity"
    );
    assert!(
        !restored.evidence.is_empty(),
        "the restored case kept the entity and lost what it rests on"
    );
    session.close().unwrap();

    let _ = std::fs::remove_dir_all(&dir);

    Summary {
        search_hits: found.hits.len(),
        audit_events_intact,
        export_verdict_ok,
        document_bytes,
        ..after
    }
}

fn summarise(
    session: &Session,
    search_hits: &usize,
    audit_events_intact: bool,
    document_bytes: usize,
) -> Summary {
    let (observations, entities, relationships, evidence_links) = session
        .with_case(|open: &mut OpenCase| {
            Ok(open
                .conn
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM observation),
                            (SELECT COUNT(*) FROM entity),
                            (SELECT COUNT(*) FROM relationship),
                            (SELECT COUNT(*) FROM evidence_link)",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap())
        })
        .unwrap();

    Summary {
        observations,
        entities,
        relationships,
        evidence_links,
        search_hits: *search_hits,
        audit_events_intact,
        export_verdict_ok: true,
        document_bytes,
    }
}
