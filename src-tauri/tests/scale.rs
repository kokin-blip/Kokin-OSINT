//! PoC P2 — does the storage layer hold at the size the spec asks for?
//!
//! **Ignored by default.** Run it deliberately:
//!
//! ```text
//! KOKIN_NETWORK=deny cargo test -p kokin-osint --test scale --release -- --ignored --nocapture
//! ```
//!
//! # What this answers
//!
//! ADR-0003 chose SQLite with FTS5 over a server database and a separate graph
//! store. R-003 is the risk that the choice does not survive contact with the
//! spec's stated target — *"cases with at least 100,000 observations remain
//! searchable"* — and it has been open since gate 3 with nothing measured.
//!
//! Three questions, which are the three things an analyst waits on:
//!
//! 1. **Full-text search** over 100k observations, through `kokin_search::search`
//!    rather than a hand-written query, so the BM25 ordering, the facet joins and
//!    the merge-map resolution are all in the measurement.
//! 2. **Bounded multi-hop expansion** — a 3-hop recursive CTE over 300k edges.
//!    This is what the graph view will issue on every expand, and ADR-0005 chose
//!    relational adjacency over a graph database on the assumption it is fast
//!    enough here.
//! 3. **`coverage()`**, which increment 16 made non-optional on every result list
//!    (D-029) and flagged as recomputed per search and never measured. If it is
//!    slow, every search in the product is slow, and the reason will not be
//!    obvious because the FTS query beside it will look fine.
//!
//! # What it does not answer
//!
//! **Vector KNN is not measured, because there is nothing to measure.**
//! `sqlite-vec` is named in R-003 and ADR-0009 and is not in this workspace — no
//! dependency, no lock entry, no embedding anywhere. That third of R-003 stays
//! open until embeddings land, and it is a different ADR (0009) with its own
//! exit condition, so it can close separately.
//!
//! # Why this is a test and not `poc/`
//!
//! `poc/` is excluded from the workspace so throwaway probe code cannot reach a
//! release build. This is not throwaway: it measures the real `kokin-store`,
//! `kokin-search` and `kokin-graph`, and it is worth re-running every time the
//! schema or an index changes. Outside the workspace it would carry its own
//! lockfile, rebuild SQLCipher from scratch, and silently stop compiling the
//! first time an API moved — a benchmark that no longer builds reports nothing
//! and looks like nothing is wrong. `#[ignore]` keeps it out of CI while the
//! compiler keeps it honest (D-036).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use kokin_search::{Query, SearchOptions};
use kokin_store::OpenCase;
use rusqlite::params;

/// The spec's stated target. Override with `KOKIN_SCALE_OBSERVATIONS` to sweep.
const OBSERVATIONS: usize = 100_000;
const ENTITIES: usize = 20_000;
const RELATIONSHIPS: usize = 300_000;
const ARTIFACTS: usize = 200;

/// The spec's interactive budget. Exceeding it on a [`Kind::Contract`] query is
/// the kill criterion for ADR-0003, not a note to file.
const BUDGET: Duration = Duration::from_millis(200);

/// What a measurement means, so that "over budget" and "must not be over
/// budget" are not silently the same statement.
///
/// The distinction exists because two of the slowest numbers here are queries
/// this product must never issue, and asserting on them would make the probe
/// fail for demonstrating exactly what it was written to demonstrate. Moving a
/// query between these categories is a decision with a paper trail, not a
/// threshold edit.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// The product's real query path. Must hold the budget.
    Contract,
    /// A slower form kept only as evidence for why the contract form is shaped
    /// the way it is. Never asserted.
    CounterExample,
    /// A real path that is genuinely over budget, with a named open risk and a
    /// ceiling set from measurement. Asserted against the ceiling so a
    /// regression is caught, and reported against the budget so the gap stays
    /// visible rather than becoming the new normal.
    TrackedGap {
        risk: &'static str,
        ceiling: Duration,
    },
}

const ITERATIONS: usize = 30;

/// Wall-clock cap per measured query. See `Latencies::measure`.
const SAMPLE_BUDGET: Duration = Duration::from_secs(20);

fn scaled(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Latencies, summarised the way a budget is actually judged.
///
/// p95 rather than a mean: a mean hides the tail, and the tail is what an
/// analyst notices. p99 alongside it because a p95 that passes with a p99 five
/// times larger is a different situation from one where they are close.
struct Latencies {
    label: String,
    kind: Kind,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    samples: usize,
    rows: usize,
}

impl Latencies {
    fn measure(label: &str, kind: Kind, mut run: impl FnMut() -> usize) -> Self {
        // One untimed pass: the first query of a session pays for page-cache
        // misses and SQLCipher key derivation on pages it has not touched, and
        // charging that to the query under test overstates every figure.
        let rows = run();

        // Capped by wall clock as well as iteration count. A query that takes 84
        // seconds does not need thirty samples to establish that it is too slow,
        // and the first version of this harness spent 43 of its 45 minutes
        // proving one number to four significant figures.
        let deadline = Instant::now() + SAMPLE_BUDGET;
        let mut samples: Vec<Duration> = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            let started = Instant::now();
            run();
            samples.push(started.elapsed());
            if Instant::now() >= deadline {
                break;
            }
        }
        samples.sort_unstable();

        let at = |q: usize| samples[((samples.len() - 1) * q) / 100];
        Self {
            label: label.to_string(),
            kind,
            p50: at(50),
            p95: at(95),
            p99: at(99),
            samples: samples.len(),
            rows,
        }
    }

    fn within_budget(&self) -> bool {
        self.p95 <= BUDGET
    }

    /// Whether this measurement should fail the probe.
    fn violates(&self) -> bool {
        match self.kind {
            Kind::Contract => self.p95 > BUDGET,
            Kind::CounterExample => false,
            Kind::TrackedGap { ceiling, .. } => self.p95 > ceiling,
        }
    }

    fn verdict(&self) -> String {
        match self.kind {
            Kind::Contract if self.within_budget() => "OK".to_string(),
            Kind::Contract => "OVER BUDGET".to_string(),
            Kind::CounterExample => "counter-example".to_string(),
            // Not "the risk is closed" - this query simply does not trigger it.
            // Whether R-014 closes is a question about the query that does.
            Kind::TrackedGap { risk, .. } if self.within_budget() => {
                format!("OK ({risk} not triggered here)")
            }
            Kind::TrackedGap { risk, .. } => format!("over budget, tracked by {risk}"),
        }
    }

    fn report(&self) {
        println!(
            "  {:<36} p50 {:>8.1}ms  p95 {:>8.1}ms  p99 {:>8.1}ms  rows {:<6} n={:<3} {}",
            self.label,
            self.p50.as_secs_f64() * 1000.0,
            self.p95.as_secs_f64() * 1000.0,
            self.p99.as_secs_f64() * 1000.0,
            self.rows,
            self.samples,
            self.verdict()
        );
    }
}

#[test]
#[ignore = "PoC P2: builds a 100k-observation case; run deliberately with --release"]
fn the_storage_layer_holds_at_a_hundred_thousand_observations() {
    let observations = scaled("KOKIN_SCALE_OBSERVATIONS", OBSERVATIONS);
    let entities = scaled("KOKIN_SCALE_ENTITIES", ENTITIES);
    let relationships = scaled("KOKIN_SCALE_RELATIONSHIPS", RELATIONSHIPS);

    let mut dir = std::env::temp_dir();
    dir.push(format!("kokin-p2-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let case = dir.join("scale.kokincase");

    println!(
        "\nP2 — building a case with {observations} observations, \
         {entities} entities, {relationships} relationships"
    );

    let (mut open, _recovery) =
        kokin_store::create_case(&case, "p2-scale", "correct horse battery staple").unwrap();
    kokin_graph::initialise(&open.conn).unwrap();

    let built = Instant::now();
    seed(&mut open, observations, entities, relationships);
    println!("  seeded in {:.1}s", built.elapsed().as_secs_f64());
    println!(
        "  case on disk: {:.1} MiB\n",
        directory_bytes(&case) as f64 / (1024.0 * 1024.0)
    );

    let mut results = Vec::new();

    // 1. Full-text search, through the real entry point.
    results.push(Latencies::measure(
        "FTS5 search (common term)",
        Kind::TrackedGap {
            risk: "R-014",
            ceiling: Duration::from_millis(350),
        },
        || {
            kokin_search::search(
                &open.conn,
                &Query::parse("mercer"),
                &SearchOptions::default(),
            )
            .unwrap()
            .len()
        },
    ));
    results.push(Latencies::measure(
        "FTS5 search (two terms)",
        Kind::TrackedGap {
            risk: "R-014",
            ceiling: Duration::from_millis(350),
        },
        || {
            kokin_search::search(
                &open.conn,
                &Query::parse("mercer holdings"),
                &SearchOptions::default(),
            )
            .unwrap()
            .len()
        },
    ));
    results.push(Latencies::measure(
        "FTS5 search (no merge resolve)",
        Kind::Contract,
        || {
            kokin_search::search(
                &open.conn,
                &Query::parse("mercer"),
                &SearchOptions {
                    resolve_merged: false,
                    ..SearchOptions::default()
                },
            )
            .unwrap()
            .len()
        },
    ));
    results.push(Latencies::measure(
        "FTS5 typeahead (prefix)",
        Kind::TrackedGap {
            risk: "R-014",
            ceiling: Duration::from_millis(350),
        },
        || {
            kokin_search::search(
                &open.conn,
                &Query::parse_for_typeahead("merc"),
                &SearchOptions::default(),
            )
            .unwrap()
            .len()
        },
    ));

    // 2. Bounded 3-hop expansion — what the graph view issues on every expand.
    let seed_entity: String = open
        .conn
        .query_row("SELECT id FROM entity LIMIT 1", [], |r| r.get(0))
        .unwrap();
    results.push(Latencies::measure(
        "3-hop expansion (IN, no index)",
        Kind::CounterExample,
        || three_hop_naive(&open, &seed_entity),
    ));
    results.push(Latencies::measure(
        "3-hop expansion (indexed)",
        Kind::CounterExample,
        || three_hop_indexed(&open, &seed_entity),
    ));
    results.push(Latencies::measure(
        "3-hop expansion (bounded per hop)",
        Kind::Contract,
        || three_hop_bounded(&open, &seed_entity, 2000, 64),
    ));

    // 3. The caveat that rides along with every result list (D-029).
    results.push(Latencies::measure("coverage()", Kind::Contract, || {
        let c = kokin_search::coverage::coverage(&open.conn).unwrap();
        c.artifacts as usize
    }));

    println!("Results (budget: p95 <= {}ms)\n", BUDGET.as_millis());
    for r in &results {
        r.report();
    }

    let violations: Vec<&Latencies> = results.iter().filter(|r| r.violates()).collect();
    let over_budget: Vec<&Latencies> = results
        .iter()
        .filter(|r| r.kind != Kind::CounterExample && !r.within_budget())
        .collect();

    println!();
    if !over_budget.is_empty() {
        println!("Over the {}ms budget on a real path:", BUDGET.as_millis());
        for r in &over_budget {
            println!(
                "  - {} at p95 {:.1}ms",
                r.label,
                r.p95.as_secs_f64() * 1000.0
            );
        }
        println!();
    }
    if violations.is_empty() {
        println!(
            "VERDICT: every contract query holds, and every tracked gap is within the 
             ceiling measured for it. ADR-0003 and ADR-0005 stand."
        );
    } else {
        println!(
            "VERDICT: {} measurement(s) exceeded what they are held to:",
            violations.len()
        );
        for r in &violations {
            println!(
                "  - {} at p95 {:.1}ms",
                r.label,
                r.p95.as_secs_f64() * 1000.0
            );
        }
    }

    drop(open);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        violations.is_empty(),
        "{} measurement(s) exceeded what they are held to. A contract query over          {}ms is R-003 materialising; a tracked gap over its ceiling is a          regression against a number already recorded in docs/benchmarks/p2-scale.md.          Neither is a flaky test, and neither is fixed by raising the number here.",
        violations.len(),
        BUDGET.as_millis()
    );
}

/// A 3-hop expansion written the obvious way, which is the slow way.
///
/// `reach.id IN (r.from_entity, r.to_entity)` reads naturally and matches an
/// undirected edge in one branch. It also cannot use either index: SQLite has
/// `relationship_from` and `relationship_to`, and an `IN` across two columns is
/// not a lookup on either, so every recursion step full-scans 300k rows.
///
/// Kept and measured on purpose. The point of the pair is that ADR-0005's
/// premise is not what fails here — the query is.
fn three_hop_naive(open: &OpenCase, from: &str) -> usize {
    let mut statement = open
        .conn
        .prepare_cached(
            "WITH RECURSIVE reach(id, depth) AS (
                 SELECT ?1, 0
                 UNION
                 SELECT CASE WHEN r.from_entity = reach.id THEN r.to_entity
                             ELSE r.from_entity END,
                        reach.depth + 1
                   FROM relationship r
                   JOIN reach ON reach.id IN (r.from_entity, r.to_entity)
                  WHERE reach.depth < 3
             )
             SELECT id FROM reach LIMIT 2000",
        )
        .unwrap();
    let rows = statement
        .query_map([from], |r| r.get::<_, String>(0))
        .unwrap();
    rows.count()
}

/// The same expansion, as the graph view must actually issue it.
///
/// Two branches instead of one `IN`, so each is an index lookup:
/// `relationship_from` for edges out, `relationship_to` for edges in. The
/// undirectedness that the `CASE` expressed is now expressed by the union of two
/// directed traversals, which is the same set and a different plan.
///
/// Bounded at 2000 rows because ADR-0005's contract is that storage scales to
/// 100k and *rendering* is capped at roughly 2k nodes.
fn three_hop_indexed(open: &OpenCase, from: &str) -> usize {
    let mut statement = open
        .conn
        .prepare_cached(
            "WITH RECURSIVE reach(id, depth) AS (
                 SELECT ?1, 0
                 UNION
                 SELECT r.to_entity, reach.depth + 1
                   FROM relationship r JOIN reach ON r.from_entity = reach.id
                  WHERE reach.depth < 3
                 UNION
                 SELECT r.from_entity, reach.depth + 1
                   FROM relationship r JOIN reach ON r.to_entity = reach.id
                  WHERE reach.depth < 3
             )
             SELECT id FROM reach LIMIT 2000",
        )
        .unwrap();
    let rows = statement
        .query_map([from], |r| r.get::<_, String>(0))
        .unwrap();
    rows.count()
}

/// The expansion a graph view can actually afford: bounded *during* traversal.
///
/// Both CTE forms above compute the entire 3-hop closure and then `LIMIT` it.
/// From a hub that closure is most of the graph, so the limit describes what is
/// returned and not what was computed — which is precisely the "uncontrolled
/// graph explosion" the spec names as a thing to prevent.
///
/// This walks hop by hop in the host, stopping the moment the render budget is
/// reached, and capping how many neighbours any single node may contribute. A
/// hub therefore costs a bounded amount rather than an unbounded one. The shape
/// is not a workaround: it is also what makes expansion cancellable and
/// progressive, both of which the spec requires and neither of which a
/// single recursive CTE can offer.
fn three_hop_bounded(open: &OpenCase, from: &str, budget: usize, per_node: usize) -> usize {
    use std::collections::HashSet;

    let mut out = open
        .conn
        .prepare_cached("SELECT to_entity FROM relationship WHERE from_entity = ?1 LIMIT ?2")
        .unwrap();
    let mut inbound = open
        .conn
        .prepare_cached("SELECT from_entity FROM relationship WHERE to_entity = ?1 LIMIT ?2")
        .unwrap();

    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(from.to_string());
    let mut frontier = vec![from.to_string()];

    for _hop in 0..3 {
        let mut next = Vec::new();
        for node in &frontier {
            for statement in [&mut out, &mut inbound] {
                let rows = statement
                    .query_map(rusqlite::params![node, per_node as i64], |r| {
                        r.get::<_, String>(0)
                    })
                    .unwrap();
                for row in rows {
                    let id = row.unwrap();
                    if seen.insert(id.clone()) {
                        next.push(id);
                        if seen.len() >= budget {
                            return seen.len();
                        }
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    seen.len()
}

fn directory_bytes(root: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Fill the case through the real schema, so every trigger fires.
///
/// Not a bulk load around the side: inserting an observation fires the trigger
/// into `search_document`, which fires the trigger into `search_index`. A seed
/// that bypassed those would measure an FTS index that the product's own write
/// path never built, which is the one thing this probe must not do.
fn seed(open: &mut OpenCase, observations: usize, entities: usize, relationships: usize) {
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
                              connector, job_id)
         VALUES ('cap-p2','src-p2', ?1, 'static_html_only', 'p2.seed', 'p2')",
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
