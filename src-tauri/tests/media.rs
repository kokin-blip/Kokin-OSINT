//! Persistence and citation checks are deterministic and offline; native parsers
//! have a separate, explicit engine acceptance suite.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_osint_lib::{media, read, write, IngestFileRequest, QuoteView, Session};
use serde_json::json;
fn result() -> kokin_media::Result {
    kokin_media::Result {
        version: 1,
        engine: "synthetic-test-engine".into(),
        observations: vec![kokin_media::Observation {
            kind: "pdf.text".into(),
            value: "Synthetic located text".into(),
            locator: json!({"scheme":"pdf_char_range","page":0,"start":0,"end":22,"bounds":[0,0,100,20]}),
        }],
        thumbnail_png: b"\x89PNG\r\n\x1a\nsynthetic protocol fixture".to_vec(),
        perceptual_hash: None,
        pages: 1,
        gaps: vec!["Synthetic native text does not cover drawings".into()],
    }
}
#[test]
fn encrypted_media_snapshots_supersede_validate_and_survive_restore() {
    let dir = std::env::temp_dir().join(format!("kokin-media-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let session = Session::default();
    session
        .create(
            &dir.join("source.kokincase"),
            "media-test",
            "synthetic secret",
        )
        .unwrap();
    let source = dir.join("source.pdf");
    std::fs::write(&source, b"%PDF-1.7 synthetic persistence fixture").unwrap();
    let artifact = session
        .with_case(|open| {
            write::ingest_file(
                open,
                &IngestFileRequest {
                    path: source.to_string_lossy().into(),
                },
            )
        })
        .unwrap()
        .artifact_id;
    let observation = session
        .with_case(|open| {
            media::commit(open, &artifact, 0, false, result())?;
            let oid = open
                .conn
                .query_row(
                    "SELECT id FROM observation WHERE artifact_id=?1 ORDER BY rowid LIMIT 1",
                    [&artifact],
                    |r| r.get::<_, String>(0),
                )
                .unwrap();
            let view = read::observation(open, &oid)?;
            assert!(matches!(view.quote, QuoteView::DerivedExact { .. }));
            assert!(media::citation(
                open,
                &artifact,
                &json!({"snapshot_hash":"missing","snapshot_index":0}),
                "text"
            )
            .is_err());
            media::commit(open, &artifact, 0, false, result())?;
            let replaced = read::observation(open, &oid)?.superseded.unwrap();
            assert!(replaced.superseded_by.is_some());
            assert!(open
                .conn
                .execute("DELETE FROM media_derivative", [])
                .is_err());
            kokin_export::package(
                open,
                &dir.join("media.kokinpkg"),
                "export secret",
                &kokin_export::PackageOptions::default(),
            )
            .unwrap();
            Ok(oid)
        })
        .unwrap();
    session.close().unwrap();
    kokin_export::import(
        &dir.join("media.kokinpkg"),
        &dir.join("restored.kokincase"),
        "export secret",
    )
    .unwrap();
    session
        .open(&dir.join("restored.kokincase"), "export secret")
        .unwrap();
    session
        .with_case(|open| {
            assert!(matches!(
                read::observation(open, &observation)?.quote,
                QuoteView::DerivedExact { .. }
            ));
            assert_eq!(media::preview(open, &artifact, 0)?.unwrap().pages, 1);
            let count: i64 = open
                .conn
                .query_row("SELECT count(*) FROM media_derivative", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 2);
            Ok(())
        })
        .unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
