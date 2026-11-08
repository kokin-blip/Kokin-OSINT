//! Raw structured evidence, located citations and package compatibility, offline.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_net::HttpResponse;
use kokin_osint_lib::{read, QuoteView, Session};
#[test]
fn source_json_is_grounded_superseded_and_restorable_with_errors_preserved() {
    let root = std::env::temp_dir().join(format!("kokin-source-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let session = Session::default();
    session
        .create(&root.join("case"), "sources", "synthetic case secret")
        .unwrap();
    let artifact=session.with_case(|open|{
  let blobs=kokin_blob::BlobStore::new(open.paths.blobs());
  let plan=kokin_connectors::dns("example.org","A").unwrap();
  let response=HttpResponse{status:200,headers:std::collections::BTreeMap::from([("content-type".into(),"application/dns-json".into())]),body:String::new(),body_bytes:Some(br#"{"Status":0,"Answer":[{"name":"example.org","data":"192.0.2.1"}],"hostile":"<script>literal</script>"}"#.to_vec()),redirect_chain:vec![],from_fixture:true};
  let captured=kokin_ingest::ingest_response(&mut open.conn,&blobs,&open.cmk,&plan.requests[0],&response,&kokin_ingest::IngestContext{connector:"dns",job_id:"fixture-test"},"structured_response_only")?;
  kokin_extract::json::extract(&mut open.conn,&blobs,&open.cmk,&captured.artifact_id).unwrap();
  let oid:String=open.conn.query_row("SELECT id FROM observation WHERE value='192.0.2.1'",[],|r|r.get(0)).unwrap();
  let view=read::observation(open,&oid)?;assert_eq!(view.locator.scheme,"json_pointer");assert!(matches!(view.quote,QuoteView::StructuredExact{..}));
  kokin_extract::json::extract(&mut open.conn,&blobs,&open.cmk,&captured.artifact_id).unwrap();assert!(read::observation(open,&oid)?.superseded.unwrap().superseded_by.is_some());
  // Non-success and malformed bodies still have genuine source bytes.
  let mut failed=response;failed.status=503;failed.body_bytes=Some(b"service temporarily unavailable".to_vec());
  let error=kokin_ingest::ingest_response(&mut open.conn,&blobs,&open.cmk,&plan.requests[0],&failed,&kokin_ingest::IngestContext{connector:"dns",job_id:"error-test"},"structured_response_only")?;
  assert!(kokin_extract::json::extract(&mut open.conn,&blobs,&open.cmk,&error.artifact_id).is_err());
  assert_eq!(read::artifact_document(open,&error.artifact_id)?.byte_length,31);
  kokin_export::package(open,&root.join("sources.pkg"),"synthetic export secret",&Default::default()).unwrap();
  Ok((captured.artifact_id,oid))
 }).unwrap();
    session.close().unwrap();
    kokin_export::import(
        &root.join("sources.pkg"),
        &root.join("restored"),
        "synthetic export secret",
    )
    .unwrap();
    session
        .open(&root.join("restored"), "synthetic export secret")
        .unwrap();
    session
        .with_case(|open| {
            assert!(matches!(
                read::observation(open, &artifact.1)?.quote,
                QuoteView::StructuredExact { .. }
            ));
            assert_eq!(
                read::artifact_document(open, &artifact.0)?.media_type,
                "application/dns-json"
            );
            Ok(())
        })
        .unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
