//! Pure persistence/preview contracts; native parser acceptance is separate.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_osint_lib::{media, research_preview, write, IngestFileRequest, Session};
use serde_json::json;
#[test]
fn evidence_candidates_authenticate_snapshots_preserve_origins_and_refuse_stale_publication() {
    let root = std::env::temp_dir().join(format!("kokin-research-preview-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let session = Session::default();
    session
        .create(&root.join("case"), "preview", "synthetic preview secret")
        .unwrap();
    let path = root.join("source.pdf");
    std::fs::write(&path, b"%PDF-1.7 synthetic preview fixture").unwrap();
    let artifact = session
        .with_case(|open| {
            Ok(write::ingest_file(
                open,
                &IngestFileRequest {
                    path: path.to_string_lossy().into(),
                },
            )?
            .artifact_id)
        })
        .unwrap();
    let result = || kokin_media::Result {
        version: 1,
        engine: "synthetic-preview".into(),
        observations: vec![kokin_media::Observation {
            kind: "pdf.text".into(),
            value: "Review Case@EXAMPLE.org and 192.0.2.1".into(),
            locator: json!({"scheme":"pdf_char_range","page":0,"start":0,"end":35,"bounds":[0,0,100,20]}),
        }],
        thumbnail_png: b"synthetic preview".to_vec(),
        perceptual_hash: None,
        pages: 1,
        gaps: vec!["Synthetic text fixture; not native engine acceptance".into()],
    };
    session
        .with_case(|open| {
            media::commit(open, &artifact, 0, false, result())?;
            Ok(())
        })
        .unwrap();
    let input = session
        .with_case(|open| research_preview::prepare(open, &artifact))
        .unwrap();
    let output = research_preview::process(input).unwrap();
    assert!(output.preview.rows.iter().any(|r| r
        .selector
        .as_ref()
        .is_some_and(|s| s.normalized == "Case@example.org")));
    assert!(output
        .preview
        .rows
        .iter()
        .all(|r| r.origin_ids.len() == 1 && r.origin_ids[0] != "local-paste"));
    session
        .with_case(|open| {
            research_preview::validate_publication(open, &output)?;
            assert_eq!(
                open.conn
                    .query_row("SELECT COUNT(*) FROM entity", [], |r| r.get::<_, i64>(0))?,
                0
            );
            media::commit(open, &artifact, 0, false, result())?;
            assert!(research_preview::validate_publication(open, &output).is_err());
            Ok(())
        })
        .unwrap();
    let input = session
        .with_case(|open| research_preview::prepare(open, &artifact))
        .unwrap();
    let output = research_preview::process(input).unwrap();
    session.with_case(|open|{open.conn.execute("UPDATE blob SET wrapped_key=NULL,wrapped_nonce=NULL WHERE content_hash=(SELECT content_hash FROM artifact WHERE id=?1)",[&artifact])?;assert!(research_preview::validate_publication(open,&output).is_err());assert!(research_preview::prepare(open,&artifact).is_err());Ok(())}).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
