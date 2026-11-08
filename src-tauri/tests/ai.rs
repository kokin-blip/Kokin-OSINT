#![allow(clippy::unwrap_used)]
use kokin_ai::{
    persistence,
    vector::{self, SqliteVectorIndex, VectorIndex},
    Suggestion,
};
use kokin_osint_lib::{write, IngestFileRequest, Session};
#[test]
fn suggestions_reviews_vectors_are_encrypted_restorable_and_never_analytical_writes() {
    let dir = std::env::temp_dir().join(format!("kokin-ai-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let case = Session::default();
    case.create(&dir.join("case"), "ai", "synthetic credential")
        .unwrap();
    let file = dir.join("source.html");
    std::fs::write(
        &file,
        "<html><p>Mercer works at Tungsten Holdings.</p></html>",
    )
    .unwrap();
    let oid = case
        .with_case(|open| {
            let artifact = write::ingest_file(
                open,
                &IngestFileRequest {
                    path: file.to_string_lossy().into(),
                },
            )?
            .artifact_id;
            kokin_extract::extract_artifact(
                &mut open.conn,
                &kokin_blob::BlobStore::new(open.paths.blobs()),
                &open.cmk,
                &artifact,
                Default::default(),
            )
            .unwrap();
            let oid: String = open
                .conn
                .query_row(
                    "SELECT id FROM observation WHERE kind='page.text'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            Ok(oid)
        })
        .unwrap();
    let sid=case.with_case(|open|{
  let suggestion=Suggestion{kind:"entity_candidate".into(),text:"Mercer is a candidate person described by this source, not independently verified.".into(),cited_observation_ids:vec![oid.clone()],proposed_name:Some("Mercer".into()),proposed_type:Some("person".into()),relationship_kind:None};
  let sid=persistence::save(&mut open.conn,"synthetic-test-model",&"a".repeat(64),"synthetic-prompt","synthetic-context",&[suggestion]).unwrap()[0].clone();
  persistence::review(&mut open.conn,&sid,"reviewed","Human checked the source; candidate still needs a record decision.").unwrap();assert!(open.conn.execute("DELETE FROM ai_suggestion",[]).is_err());assert!(open.conn.execute("UPDATE ai_review SET actor='ai:test'",[]).is_err());
  let mut v=vec![0.0;384];v[0]=1.0;open.conn.execute("INSERT INTO observation_embedding VALUES(?1,'synthetic-test-model','hash',?2)",rusqlite::params![oid,vector::encode(&v).unwrap()]).unwrap();open.conn.execute("INSERT INTO embedding_meta VALUES(1,'synthetic-test-model',384,'synthetic-time',0)",[]).unwrap();vector::rebuild_projection(&open.conn).unwrap();let nearest=SqliteVectorIndex{conn:&open.conn}.nearest(&v,10).unwrap();assert_eq!(nearest[0].0,oid);
  assert_eq!(open.conn.query_row("SELECT count(*) FROM entity",[],|r|r.get::<_,i64>(0)).unwrap(),0);assert_eq!(open.conn.query_row("SELECT count(*) FROM relationship",[],|r|r.get::<_,i64>(0)).unwrap(),0);
  kokin_export::package(open,&dir.join("case.pkg"),"synthetic package secret",&Default::default()).unwrap();Ok(sid)
 }).unwrap();
    case.close().unwrap();
    kokin_export::import(
        &dir.join("case.pkg"),
        &dir.join("restored"),
        "synthetic package secret",
    )
    .unwrap();
    case.open(&dir.join("restored"), "synthetic package secret")
        .unwrap();
    case.with_case(|open| {
        assert_eq!(
            persistence::get(&open.conn, &sid)
                .unwrap()
                .cited_observation_ids,
            vec![oid]
        );
        assert_eq!(
            persistence::list(&open.conn, 0).unwrap().0[0]
                .review
                .as_deref(),
            Some("reviewed")
        );
        assert_eq!(
            open.conn
                .query_row("SELECT count(*) FROM observation_embedding", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        Ok(())
    })
    .unwrap();
    case.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
