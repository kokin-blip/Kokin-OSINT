//! Synthetic diagnostic corpus only. Never included without desktop-test.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_store::OpenCase;
use rusqlite::params;
const ARTIFACTS: usize = 200;
pub(crate) fn seed(
    open: &mut OpenCase,
    observations: usize,
    entities: usize,
    relationships: usize,
) {
    let now = "2026-08-14T00:00:00Z";
    let tx = open.conn.transaction().unwrap();

    tx.execute(
        "INSERT INTO source (id, kind, raw_locator, canonical_locator, first_seen_utc)
         VALUES ('src-p2','url','https://scale.example/','https://scale.example/', ?1)",
        params![now],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO transform_run (id, transform_name, transform_version, code_version,
                                    params_hash, started_utc, status)
         VALUES ('run-p2','p2.seed','1','p2','0', ?1, 'succeeded')",
        params![now],
    )
    .unwrap();

    // One capture per artifact would be more faithful, but coverage() walks
    // lineage from the artifact side, and a single capture is enough for the
    // derivation edges to resolve. Recorded here so the shortcut is visible
    // rather than discovered later as an unexplained difference from a real case.
    tx.execute(
        "INSERT INTO capture (id, source_id, requested_utc, capture_completeness,
                              connector, job_id,from_fixture)
         VALUES ('cap-p2','src-p2', ?1, 'static_html_only', 'p2.seed', 'p2',1)",
        params![now],
    )
    .unwrap();

    // Artifacts, each with a blob row so the coverage query has real lineage to
    // walk rather than a single artifact it can answer from one page.
    for a in 0..ARTIFACTS {
        let hash = format!("{a:064x}");
        tx.execute(
            "INSERT INTO blob (content_hash, size_bytes, stored_utc) VALUES (?1, 1024, ?2)",
            params![hash, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO artifact (id, content_hash, media_type, byte_length, created_utc)
             VALUES (?1, ?2, 'text/html', 1024, ?3)",
            params![format!("art-{a}"), hash, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO derivation (id, run_id, input_kind, input_id, output_kind, output_id)
             VALUES (?1,'run-p2','capture','cap-p2','artifact',?2)",
            params![format!("der-{a}"), format!("art-{a}")],
        )
        .unwrap();
    }

    // Observations. Roughly a fifth mention the search term, so the FTS query
    // returns a realistic working set rather than one row or every row.
    let mut insert = tx
        .prepare(
            "INSERT INTO observation (id, artifact_id, run_id, kind, value, locator_json, observed_utc)
             VALUES (?1, ?2, 'run-p2', ?3, ?4, '{\"xpath\":\"/html/body/p[1]\"}', ?5)",
        )
        .unwrap();
    for i in 0..observations {
        let value = if i % 5 == 0 {
            format!("A. Mercer director of Tungsten Holdings filing {i}")
        } else {
            format!("Ridgeway Trust registered office record number {i}")
        };
        insert
            .execute(params![
                format!("obs-{i}"),
                format!("art-{}", i % ARTIFACTS),
                if i % 3 == 0 { "person_name" } else { "text" },
                value,
                now
            ])
            .unwrap();
    }
    drop(insert);

    // Entities and the evidence every one of them must have.
    let mut entity_insert = tx
        .prepare(
            "INSERT INTO entity (id, type_key, display_name, notes, created_utc, updated_utc)
             VALUES (?1, 'person', ?2, '', ?3, ?3)",
        )
        .unwrap();
    let mut evidence_insert = tx
        .prepare(
            "INSERT INTO evidence_link
                (id, subject_kind, subject_id, evidence_kind, evidence_id, role, created_utc)
             VALUES (?1, ?2, ?3, 'observation', ?4, 'supports', ?5)",
        )
        .unwrap();

    // Evidence first, then the row it grounds. The trigger is explicit about the
    // order — "write the evidence_link before the entity" — because an L2 row
    // that exists for even one statement without its grounding is a row the
    // product's central invariant did not hold for.
    for e in 0..entities {
        evidence_insert
            .execute(params![
                format!("el-e-{e}"),
                "entity",
                format!("ent-{e}"),
                format!("obs-{}", e % observations),
                now
            ])
            .unwrap();
        entity_insert
            .execute(params![format!("ent-{e}"), format!("A. Mercer {e}"), now])
            .unwrap();
    }
    drop(entity_insert);

    // Relationships. Deliberately not uniform-random: a real investigation graph
    // has hubs, and a 3-hop expansion from a hub is the expensive case the graph
    // view will actually hit. Every tenth entity is wired to many others.
    let mut relate = tx
        .prepare(
            "INSERT INTO relationship (id, from_entity, to_entity, kind, created_utc)
             VALUES (?1, ?2, ?3, 'associated_with', ?4)",
        )
        .unwrap();
    for r in 0..relationships {
        let from = if r % 10 == 0 {
            r % 50 // a hub
        } else {
            r % entities
        };
        let to = (r * 7 + 13) % entities;
        if from == to {
            continue;
        }
        evidence_insert
            .execute(params![
                format!("el-r-{r}"),
                "relationship",
                format!("rel-{r}"),
                format!("obs-{}", r % observations),
                now
            ])
            .unwrap();
        relate
            .execute(params![
                format!("rel-{r}"),
                format!("ent-{from}"),
                format!("ent-{to}"),
                now
            ])
            .unwrap();
    }
    drop(relate);
    drop(evidence_insert);

    tx.commit().unwrap();

    // ANALYZE once after loading, which is what a real case gets over its life
    // and what the planner needs to choose the indexes that exist.
    open.conn.execute_batch("ANALYZE;").unwrap();
}

/// Add a bounded board topology atop the larger diagnostic corpus.
pub(crate) fn board(open: &mut OpenCase) -> Vec<String> {
    let now = "2026-10-12T00:00:00Z";
    let tx = open.conn.transaction().unwrap();
    for r in 0..1010usize {
        let from = if r >= 900 { 0 } else { r % 299 };
        let to = if r == 299 { from } else { (r + 1) % 299 };
        let id = format!("board-rel-{r}");
        tx.execute("INSERT INTO evidence_link(id,subject_kind,subject_id,evidence_kind,evidence_id,role,created_utc) VALUES(?1,'relationship',?2,'observation','obs-0','context',?3)",params![format!("board-el-{r}"),id,now]).unwrap();
        tx.execute("INSERT INTO relationship(id,from_entity,to_entity,kind,created_utc) VALUES(?1,?2,?3,'stress_path',?4)",params![id,format!("ent-{from}"),format!("ent-{to}"),now]).unwrap();
    }
    tx.commit().unwrap();
    kokin_graph::resolution::merge(
        &mut open.conn,
        "ent-298",
        "ent-297",
        "user:local",
        "Synthetic scale alias",
        None,
    )
    .unwrap();
    (0..300).map(|i| format!("ent-{i}")).collect()
}
