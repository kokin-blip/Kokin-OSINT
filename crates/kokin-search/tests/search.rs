//! Search against the real pipeline.
//!
//! Every document searched here was projected by a trigger from a row that a
//! real ingest, a real extractor, or the real L2 API wrote. Hand-inserted
//! `search_document` rows would only prove that the query layer works against
//! whatever shape this file imagined, and the claim under test is that
//! increments 6, 7, 8 and 9 compose.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_blob::BlobStore;
use kokin_extract::{extract_artifact, Limits};
use kokin_graph::{add_identifier, create_entity, initialise, relate, Grounding, NewEntity};
use kokin_ingest::{ingest_file, IngestContext};
use kokin_keys::CaseMasterKey;
use kokin_search::{
    check, count, dangling_documents, facet_counts, rebuild, search, Query, SearchOptions,
};
use rusqlite::Connection;

static COUNTER: AtomicU32 = AtomicU32::new(0);

const PAGE: &str = concat!(
    "<html><head><title>Acme Holdings</title></head>\n",
    "<body><a href=\"mailto:press@ACME.example\">Press</a>\n",
    "<a href=\"mailto:sales@acme.example\">Sales</a>\n",
    "<a href=\"https://acme.example/about\">About</a></body></html>"
);

struct Case {
    conn: Connection,
    dir: PathBuf,
    blobs: BlobStore,
    cmk: CaseMasterKey,
    artifact: String,
    observations: Vec<String>,
}

impl Case {
    fn new(name: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("kokin-search-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let case_dir = dir.join("case.kokincase");
        let (mut conn, paths, _recovery) =
            kokin_store::create_case(&case_dir, "case-search", "passphrase").unwrap();
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
                job_id: "job-search",
            },
        )
        .unwrap()
        .artifact_id;

        let extracted =
            extract_artifact(&mut conn, &blobs, &cmk, &artifact, Limits::default()).unwrap();
        initialise(&conn).unwrap();

        Case {
            conn,
            dir,
            blobs,
            cmk,
            artifact,
            observations: extracted.observation_ids,
        }
    }

    fn observation(&self) -> String {
        self.observations[0].clone()
    }

    fn entity(&mut self, type_key: &str, name: &str, notes: &str) -> String {
        let observation = self.observation();
        let evidence = [Grounding::supporting_observation(&observation)];
        create_entity(
            &mut self.conn,
            NewEntity {
                type_key,
                display_name: name,
                notes,
            },
            &evidence,
        )
        .unwrap()
    }

    /// Run the extractor again, as an upgrade would.
    fn re_extract(&mut self) -> usize {
        extract_artifact(
            &mut self.conn,
            &self.blobs,
            &self.cmk,
            &self.artifact,
            Limits::default(),
        )
        .unwrap()
        .superseded
    }

    fn find(&self, raw: &str) -> Vec<kokin_search::Hit> {
        search(&self.conn, &Query::parse(raw), &SearchOptions::default()).unwrap()
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The point of the whole increment: a hit is not a piece of text, it is a
/// route back to the row that text came from, and from there to the evidence.
#[test]
fn a_hit_routes_back_to_the_row_it_came_from() {
    let case = Case::new("route");

    let hits = case.find("acme");
    assert!(!hits.is_empty(), "nothing found for a term on the page");

    let observation = hits
        .iter()
        .find(|h| h.subject_kind == "observation")
        .expect("no observation matched");

    // The id it hands back must be a real observation with a real locator.
    let (kind, locator): (String, String) = case
        .conn
        .query_row(
            "SELECT kind, locator_json FROM observation WHERE id = ?1",
            [&observation.subject_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();

    assert_eq!(
        kind, observation.facet,
        "facet must be the observation kind"
    );
    assert!(
        locator.contains(':') || locator.contains('{'),
        "the hit routes to an observation with no locator: {locator}"
    );
}

#[test]
fn entities_and_identifiers_are_findable_by_what_they_are_called() {
    let mut case = Case::new("l2find");
    let entity = case.entity("organisation", "Acme Holdings", "a shell company");

    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];
    add_identifier(
        &mut case.conn,
        &entity,
        "email_address",
        "Press@ACME.example",
        &evidence,
    )
    .unwrap();

    let kinds: Vec<String> = case
        .find("acme")
        .into_iter()
        .map(|h| h.subject_kind)
        .collect();
    assert!(kinds.contains(&"entity".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"identifier".to_string()), "{kinds:?}");

    // Captured shouting, searched quietly. The normalised form is indexed
    // alongside the raw one precisely so this works.
    let hits = case.find("press@acme.example");
    assert!(
        hits.iter().any(|h| h.subject_kind == "identifier"),
        "an identifier captured as Press@ACME.example was not found in lower case"
    );
}

/// An email address is a phrase, not three loose words. Without that, every
/// address at a domain matches every other, and an analyst searching for one
/// person's address gets the whole company.
#[test]
fn one_address_does_not_match_another_at_the_same_domain() {
    let case = Case::new("phrase");

    let titles: Vec<String> = case
        .find("press@acme.example")
        .into_iter()
        .map(|h| h.title.to_lowercase())
        .collect();

    assert!(
        titles.iter().any(|t| t.contains("press@acme.example")),
        "the address itself was not found: {titles:?}"
    );
    assert!(
        !titles.iter().any(|t| t.contains("sales@acme.example")),
        "searching one address matched a different address at the same domain: {titles:?}"
    );
}

/// Rerunning an extractor withdraws its earlier observations (ADR-0006). If
/// the index did not know that, every rerun would silently double every result
/// list, and the duplicates would look like corroboration — two independent
/// sightings of the same fact, which is exactly the wrong conclusion.
#[test]
fn a_rerun_does_not_double_every_result() {
    let mut case = Case::new("rerun");

    let before = case.find("acme").len();
    assert!(before > 0);

    let superseded = case.re_extract();
    assert!(
        superseded > 0,
        "the rerun superseded nothing; this test is not exercising supersession"
    );

    let after = case.find("acme").len();
    assert_eq!(
        before, after,
        "a rerun doubled the result list: {before} -> {after}"
    );

    // The withdrawn ones are still there to be asked for, and arrive labelled.
    let with_history = search(
        &case.conn,
        &Query::parse("acme"),
        &SearchOptions {
            include_superseded: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        with_history.len() > after,
        "withdrawn observations were deleted rather than flagged"
    );
    assert!(
        with_history.iter().any(|h| h.superseded),
        "a withdrawn observation came back unlabelled"
    );
    assert!(
        !case.find("acme").iter().any(|h| h.superseded),
        "a withdrawn observation leaked into the default view"
    );
}

/// L2 is mutable, so the projection has to keep up. A rename that leaves the
/// old name searchable is worse than one that is slow to appear: it means the
/// case answers to a name it no longer holds.
#[test]
fn renaming_an_entity_changes_what_finds_it() {
    let mut case = Case::new("rename");
    let entity = case.entity("organisation", "Ampersand Trading", "");

    assert!(!case.find("ampersand").is_empty());

    case.conn
        .execute(
            "UPDATE entity SET display_name = 'Bellwether Trading', updated_utc = ?2 WHERE id = ?1",
            rusqlite::params![entity, kokin_store::now_utc_rfc3339()],
        )
        .unwrap();

    assert!(
        case.find("ampersand").is_empty(),
        "the old name is still searchable after a rename"
    );
    assert!(
        !case.find("bellwether").is_empty(),
        "the new name is not searchable after a rename"
    );
}

/// A relationship is indexed by its kind and not by the names of the entities
/// it joins — see the note above migration 5. Renaming an endpoint must not be
/// able to leave a stale name searchable through the edge.
#[test]
fn a_relationship_carries_only_the_text_it_owns() {
    let mut case = Case::new("edge");
    let from = case.entity("organisation", "Ampersand Trading", "");
    let to = case.entity("person", "Someone Else", "");

    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];
    relate(
        &mut case.conn,
        &from,
        &to,
        "mentioned_alongside",
        (None, None),
        &evidence,
    )
    .unwrap();

    let edges: Vec<_> = case
        .find("mentioned_alongside")
        .into_iter()
        .filter(|h| h.subject_kind == "relationship")
        .collect();
    assert_eq!(edges.len(), 1, "a relationship is not findable by its kind");

    // Renaming an endpoint touches the entity's document and nothing else.
    case.conn
        .execute(
            "UPDATE entity SET display_name = 'Renamed Ltd', updated_utc = ?2 WHERE id = ?1",
            rusqlite::params![from, kokin_store::now_utc_rfc3339()],
        )
        .unwrap();

    let stale: i64 = case
        .conn
        .query_row(
            "SELECT COUNT(*) FROM search_document
              WHERE subject_kind = 'relationship' AND body LIKE '%Ampersand%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        stale, 0,
        "a relationship document holds an endpoint's old name"
    );
}

/// Names in an OSINT case are transliterated inconsistently by the sources
/// that publish them. A case that holds "Müller" and cannot answer "Muller"
/// reports an absence that is not there.
#[test]
fn a_diacritic_does_not_hide_a_name() {
    let mut case = Case::new("fold");
    case.entity("person", "Jürgen Müller", "");

    assert!(
        !case.find("muller").is_empty(),
        "a folded search did not find the accented name"
    );
    assert!(
        !case.find("Müller").is_empty(),
        "the accented spelling did not find itself"
    );
}

#[test]
fn results_can_be_narrowed_by_kind_and_by_facet() {
    let mut case = Case::new("facets");
    case.entity("organisation", "Acme Holdings", "");

    let only_entities = search(
        &case.conn,
        &Query::parse("acme"),
        &SearchOptions {
            kinds: vec!["entity".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!only_entities.is_empty());
    assert!(only_entities.iter().all(|h| h.subject_kind == "entity"));

    let only_emails = search(
        &case.conn,
        &Query::parse("acme"),
        &SearchOptions {
            facets: vec!["email.address".into()],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!only_emails.is_empty());
    assert!(only_emails.iter().all(|h| h.facet == "email.address"));

    // The summary an analyst reads before the results themselves.
    let counts =
        facet_counts(&case.conn, &Query::parse("acme"), &SearchOptions::default()).unwrap();
    let total: i64 = counts.iter().map(|(_, _, n)| n).sum();
    assert_eq!(
        total,
        count(&case.conn, &Query::parse("acme"), &SearchOptions::default()).unwrap(),
        "the facet breakdown does not add up to the result count"
    );
}

/// The index is a projection: dropping its contents and rebuilding from
/// `search_document` must produce the same answers. If it does not, then
/// something is knowable only from the index, which is the one thing it is not
/// allowed to be.
#[test]
fn the_index_can_be_rebuilt_from_the_case() {
    let mut case = Case::new("rebuild");
    case.entity("organisation", "Acme Holdings", "a shell company");

    let before: Vec<String> = case
        .find("acme")
        .into_iter()
        .map(|h| format!("{}:{}", h.subject_kind, h.subject_id))
        .collect();
    assert!(!before.is_empty());

    rebuild(&case.conn).unwrap();

    let after: Vec<String> = case
        .find("acme")
        .into_iter()
        .map(|h| format!("{}:{}", h.subject_kind, h.subject_id))
        .collect();
    assert_eq!(before, after, "a rebuilt index answers differently");
}

/// After a real workload: FTS5 agrees with the text it indexed, and every
/// document routes to a row that exists. A hit that opens nothing is a result
/// an analyst cannot check.
#[test]
fn the_index_agrees_with_the_case_after_a_workload() {
    let mut case = Case::new("integrity");
    let entity = case.entity("organisation", "Acme Holdings", "notes here");
    let observation = case.observation();
    let evidence = [Grounding::supporting_observation(&observation)];
    add_identifier(&mut case.conn, &entity, "domain", "acme.example", &evidence).unwrap();
    case.re_extract();

    check(&case.conn).unwrap();
    assert_eq!(
        dangling_documents(&case.conn).unwrap(),
        Vec::<(String, String)>::new(),
        "documents point at rows that do not exist"
    );
}

#[test]
fn deleting_an_l2_row_removes_it_from_the_index() {
    let mut case = Case::new("delete");
    let entity = case.entity("organisation", "Ampersand Trading", "");
    assert!(!case.find("ampersand").is_empty());

    // The grounding rule requires the row to go before its last evidence.
    case.conn
        .execute("DELETE FROM entity WHERE id = ?1", [&entity])
        .unwrap();

    assert!(
        case.find("ampersand").is_empty(),
        "a deleted entity is still searchable"
    );
    assert_eq!(
        dangling_documents(&case.conn).unwrap(),
        Vec::<(String, String)>::new()
    );
}

/// End to end, through the real database: none of these may raise an error or
/// return everything. A search box that errors on `C++` is broken; one that
/// returns the whole case for `-acme` is worse.
#[test]
fn hostile_queries_return_nothing_rather_than_erroring() {
    let case = Case::new("hostile");

    for raw in [
        "",
        "   ",
        "-",
        "---",
        "-acme",
        "\"unclosed",
        "title : acme",
        "acme OR *",
        "NEAR(a b, 2)",
        "((((",
        "C++",
        "O'Brien",
        "%_%",
        "\\",
    ] {
        let hits = search(&case.conn, &Query::parse(raw), &SearchOptions::default());
        let hits = hits.unwrap_or_else(|e| panic!("query {raw:?} errored: {e}"));

        let everything: i64 = case
            .conn
            .query_row("SELECT COUNT(*) FROM search_document", [], |r| r.get(0))
            .unwrap();
        assert!(
            (hits.len() as i64) < everything,
            "query {raw:?} returned the entire case"
        );
    }
}

/// Ranking is text similarity and nothing more, but it still has to be useful:
/// a row that IS the thing you searched for should beat a row that merely
/// mentions it.
#[test]
fn a_name_match_outranks_a_passing_mention() {
    let mut case = Case::new("rank");
    case.entity("organisation", "Bellwether Trading", "acme acme acme acme");
    case.entity("organisation", "Acme Holdings", "");

    let hits = search(
        &case.conn,
        &Query::parse("acme"),
        &SearchOptions {
            kinds: vec!["entity".into()],
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(hits.len(), 2);
    assert_eq!(
        hits[0].title,
        "Acme Holdings",
        "the entity named Acme ranked below one that merely mentions it: {:?}",
        hits.iter()
            .map(|h| (&h.title, h.relevance))
            .collect::<Vec<_>>()
    );
}

/// The scale ADR-0003 commits to: 100,000 observations still searchable, with
/// interactive queries under 200 ms at p95.
///
/// Ignored by default because it takes minutes and would make the suite
/// useless as a fast check. Run it deliberately:
///
/// ```text
/// cargo test -p kokin-search --release --test search -- --ignored --nocapture
/// ```
///
/// Measured results belong in `docs/increments/009-search.md` next to the
/// hardware they were measured on, because a latency figure without a machine
/// attached to it is not a measurement.
///
/// The observations are written straight to SQL rather than through the
/// extractor. That is deliberate and it is a real limitation of this test: it
/// measures the read path at scale, and says nothing about what it costs to
/// build an index that size.
#[test]
#[ignore = "scale measurement; minutes, not seconds"]
fn a_hundred_thousand_observations_stay_searchable() {
    const N: usize = 100_000;

    let mut case = Case::new("scale");
    let run: String = case
        .conn
        .query_row("SELECT id FROM transform_run LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let artifact = case.artifact.clone();

    let started = std::time::Instant::now();
    let tx = case.conn.transaction().unwrap();
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO observation (id, artifact_id, run_id, kind, value, locator_json, observed_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, '{\"scale\":true}', '2026-08-12T00:00:00Z')",
            )
            .unwrap();
        for i in 0..N {
            // Varied enough that the index is not one term repeated, and
            // sparse enough that a realistic query matches a small subset.
            let value = format!(
                "Record {i} concerning Bellwether Trading and contact person{i}@example{}.test \
                 with reference lorem ipsum dolor sit amet",
                i % 1000
            );
            stmt.execute(rusqlite::params![
                format!("obs-scale-{i}"),
                artifact,
                run,
                "page.text",
                value
            ])
            .unwrap();
        }
    }
    tx.commit().unwrap();
    let build = started.elapsed();

    let documents: i64 = case
        .conn
        .query_row("SELECT COUNT(*) FROM search_document", [], |r| r.get(0))
        .unwrap();
    assert!(
        documents >= N as i64,
        "the projection did not keep up: {documents} documents for {N} observations"
    );

    // The two regimes turned out to be so far apart that averaging them into
    // one number would have hidden both. Measured separately on purpose.
    let selective = [
        "person42000@example0.test",
        "person99999@example999.test",
        "Record 12345",
    ];
    // Terms this corpus puts in EVERY document. That is pathological as a
    // proportion, but it is not an unrealistic *shape*: the central subject of
    // an investigation really does appear in most of its documents, so this is
    // the right worst case to hold ourselves to.
    let broad = ["bellwether", "\"lorem ipsum\"", "reference dolor"];

    let measure = |queries: &[&str]| -> (std::time::Duration, std::time::Duration) {
        let mut timings = Vec::new();
        for _ in 0..20 {
            for raw in queries {
                let started = std::time::Instant::now();
                let hits =
                    search(&case.conn, &Query::parse(raw), &SearchOptions::default()).unwrap();
                timings.push(started.elapsed());
                assert!(!hits.is_empty(), "query {raw:?} found nothing at scale");
            }
        }
        timings.sort();
        (
            timings[timings.len() / 2],
            timings[timings.len() * 95 / 100],
        )
    };

    let (selective_p50, selective_p95) = measure(&selective);
    let (broad_p50, broad_p95) = measure(&broad);

    println!(
        "\n{N} observations | index built in {build:?}\n\
         selective queries  p50 {selective_p50:?}  p95 {selective_p95:?}\n\
         broad queries      p50 {broad_p50:?}  p95 {broad_p95:?}\n"
    );

    // The budget ADR-0003 commits to, met with several orders of magnitude to
    // spare for any query that narrows the case at all.
    assert!(
        selective_p95 < std::time::Duration::from_millis(200),
        "selective p95 {selective_p95:?} exceeds the 200 ms ADR-0003 commits to"
    );

    // Broad queries do NOT meet that budget, and this assertion does not
    // pretend otherwise: measured p95 is ~240 ms, because ranking requires
    // BM25 over every matching document and there are 100,000 of them.
    // Loosening the number above to cover this case would have converted a
    // measurement into a claim. See docs/increments/009-search.md.
    //
    // What this guards is a regression: 500 ms would mean something changed by
    // a factor, not by noise.
    assert!(
        broad_p95 < std::time::Duration::from_millis(500),
        "broad p95 {broad_p95:?} is far worse than the ~240 ms measured; \
         something regressed rather than drifted"
    );
}

#[test]
fn a_snippet_shows_why_a_result_matched() {
    let mut case = Case::new("snippet");
    case.entity(
        "organisation",
        "Bellwether Trading",
        "a long note that mentions acme somewhere in the middle of it",
    );

    let hit = search(
        &case.conn,
        &Query::parse("acme"),
        &SearchOptions {
            kinds: vec!["entity".into()],
            ..Default::default()
        },
    )
    .unwrap()
    .remove(0);

    assert!(
        hit.snippet.contains("[acme]"),
        "the snippet does not mark the match: {:?}",
        hit.snippet
    );
}
