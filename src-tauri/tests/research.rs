//! Research persistence, source grounding, suppression and package contracts. No network.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_connectors::research::*;
use kokin_osint_lib::{read, research, QuoteView, Session};
use serde_json::json;
#[test]
fn research_is_grounded_encrypted_cached_and_restorable() {
    let root = std::env::temp_dir().join(format!("kokin-research-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let session = Session::default();
    session
        .create(
            &root.join("case"),
            "research",
            "synthetic research passphrase",
        )
        .unwrap();
    let request = ResearchRequest {
        version: 1,
        selectors: vec![
            Selector::parse(SelectorKind::Username, "synthetic-research-user").unwrap(),
        ],
        sources: vec!["github".into()],
        rescan: false,
        safe_mode: true,
        options: Default::default(),
    };
    let (plan, checks) =
        kokin_connectors::research::checks(&request, session.generation()).unwrap();
    let captured=session.with_case(|open|{
  kokin_store::research::begin(&open.conn,"research-run",&serde_json::to_value(&plan).unwrap(),1)?;
  let response=kokin_net::HttpResponse{status:200,headers:std::collections::BTreeMap::from([("content-type".into(),"application/json".into())]),body:json!({"login":"synthetic-research-user","id":123,"name":"Synthetic research name","bio":"<script>literal hostile field</script>","html_url":"https://github.com/synthetic-research-user"}).to_string(),body_bytes:None,redirect_chain:vec![],from_fixture:true};
  let r=research::publish_response(open,"research-run",&checks[0],&response)?;assert_eq!(r.outcome,CheckOutcome::Found);
  let proposal=research::review_candidate(open,&r.id)?;assert_eq!(proposal["proposed_type"],"social_account");assert_eq!(proposal["value"],"synthetic-research-user");assert_eq!(proposal["cited_observation_ids"].as_array().unwrap().len(),1);
  assert_eq!(open.conn.query_row("SELECT COUNT(*) FROM entity",[],|r|r.get::<_,i64>(0))?,0);
  let artifact=r.artifact_id.clone().unwrap();let oid:String=open.conn.query_row("SELECT o.id FROM observation o WHERE o.artifact_id=?1 AND o.value='Synthetic research name'",[&artifact],|r|r.get(0)).unwrap();
  assert!(matches!(read::observation(open,&oid)?.quote,QuoteView::StructuredExact{..}));
  assert_eq!(kokin_store::research::cached(&open.conn,&research::cache_key(&checks[0]))?.unwrap()["observed_utc"],r.observed_utc);
  assert!(research::authenticated_cache(open,&research::cache_key(&checks[0]))?.is_some());
  let hash:String=open.conn.query_row("SELECT content_hash FROM artifact WHERE id=?1",[&artifact],|r|r.get(0))?;
  let storage=kokin_blob::storage_name(&hash,&open.cmk);let file=open.paths.blobs().join(kokin_blob::relative_path(&storage));
  let encrypted=std::fs::read(&file).unwrap();let mut corrupt=encrypted.clone();let last=corrupt.len()-1;corrupt[last]^=1;std::fs::write(&file,corrupt).unwrap();
  assert!(research::authenticated_cache(open,&research::cache_key(&checks[0]))?.is_none());std::fs::write(file,encrypted).unwrap();
  // Per-source freshness can shorten reuse, but cannot extend the global ceiling.
  let key=research::cache_key(&checks[0]);
  assert!(kokin_store::research::cached_for(&open.conn,&key,0)?.is_none());
  kokin_store::research::begin(&open.conn,"freshness-age",&json!({}),1)?;
  let aged=serde_json::to_value(&r).unwrap();
  open.conn.execute("INSERT INTO research_result(id,run_id,cache_key,outcome,artifact_id,payload_json,created_epoch) VALUES('two-hour-original','freshness-age','two-hour-key','found',?1,?2,?3)",rusqlite::params![artifact,aged.to_string(),kokin_store::research::epoch()-2*3600])?;
  assert!(kokin_store::research::cached_for(&open.conn,"two-hour-key",3600)?.is_none());
  assert!(kokin_store::research::cached_for(&open.conn,"two-hour-key",8*3600)?.is_some());
  open.conn.execute("INSERT INTO research_result(id,run_id,cache_key,outcome,artifact_id,payload_json,created_epoch) VALUES('future-original','freshness-age','future-key','found',?1,?2,?3)",rusqlite::params![artifact,aged.to_string(),kokin_store::research::epoch()+3600])?;
  assert!(kokin_store::research::cached_for(&open.conn,"future-key",8*3600)?.is_none());
  // Reusing a cached answer must not turn its copy into a new eight-hour source.
  kokin_store::research::begin(&open.conn,"cache-age",&json!({}),2)?;
  let mut expired=serde_json::to_value(&r).unwrap();expired["id"]="expired-original".into();expired["run_id"]="cache-age".into();
  open.conn.execute("INSERT INTO research_result(id,run_id,cache_key,outcome,artifact_id,payload_json,created_epoch) VALUES('expired-original','cache-age','expired-key','found',?1,?2,?3)",rusqlite::params![artifact,expired.to_string(),kokin_store::research::epoch()-8*3600-1])?;
  expired["id"]="recent-copy".into();expired["from_cache"]=true.into();kokin_store::research::append(&open.conn,"cache-age","recent-copy","expired-key",&expired)?;
  assert!(kokin_store::research::cached(&open.conn,"expired-key")?.is_none());
  kokin_store::research::finish(&open.conn,"cache-age","completed")?;
  kokin_store::research::finish(&open.conn,"research-run","completed")?;
  let export=kokin_export::research::render(&open.conn,std::slice::from_ref(&r.id),"json").unwrap();let export:serde_json::Value=serde_json::from_slice(&export).unwrap();assert_eq!(export["results"][0]["artifact_id"],artifact);
  let destination=root.join("atomic.json");kokin_export::research::publish(&destination,b"first").unwrap();assert!(kokin_export::research::publish(&destination,b"second").is_err());assert_eq!(std::fs::read(destination).unwrap(),b"first");
  let doc=kokin_store::research::Document{id:"saved-results".into(),revision:0,kind:"collection".into(),name:"Reviewed leads".into(),payload:json!({"version":1,"result_ids":[r.id]}),archived:false,updated_utc:String::new()};
  kokin_store::research::save_document(&open.conn,&doc)?;
  kokin_export::package(open,&root.join("research.pkg"),"synthetic package secret",&Default::default()).unwrap();
  Ok(artifact)
 }).unwrap();
    session.close().unwrap();
    let bytes = std::fs::read(root.join("case/case.db")).unwrap();
    assert!(!bytes.windows(23).any(|w| w == b"Synthetic research name"));
    kokin_export::import(
        &root.join("research.pkg"),
        &root.join("restored"),
        "synthetic package secret",
    )
    .unwrap();
    session
        .open(&root.join("restored"), "synthetic package secret")
        .unwrap();
    session.with_case(|open|{
  assert_eq!(kokin_store::research::results(&open.conn,"research-run",0)?.total,1);
  assert_eq!(kokin_store::research::documents(&open.conn,"collection")?[0].name,"Reviewed leads");
  open.conn.execute("UPDATE blob SET wrapped_key=NULL,wrapped_nonce=NULL WHERE content_hash=(SELECT content_hash FROM artifact WHERE id=?1)",[&captured]).unwrap();
  assert!(kokin_store::research::cached(&open.conn,&research::cache_key(&checks[0]))?.is_none());
  let filtered=kokin_store::research::filtered_results(&open.conn,"research-run",0,&kokin_store::research::ResultFilter{outcome:"found".into(),sources:vec![],..Default::default()})?;
  assert_eq!(filtered.page.total,0);assert_eq!(filtered.outcome_counts["unknown"],1);
  let result=&kokin_store::research::results(&open.conn,"research-run",0)?.rows[0];assert_eq!(result["outcome"],"unknown");assert_eq!(result["fields"],json!({}));Ok(())
 }).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn image_request_provenance_keeps_a_digest_without_an_immutable_upload_copy() {
    let root = std::env::temp_dir().join(format!("kokin-image-provenance-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let session = Session::default();
    session
        .create(
            &root.join("case"),
            "image-provenance",
            "synthetic image passphrase",
        )
        .unwrap();
    session
        .with_case(|open| {
            let file = root.join("query.png");
            std::fs::write(&file, b"\x89PNG\r\n\x1a\nsynthetic-original").unwrap();
            let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
            let imported = kokin_ingest::ingest_file(
                &mut open.conn,
                &blobs,
                &open.cmk,
                &file,
                &kokin_ingest::IngestContext {
                    connector: "test",
                    job_id: "import",
                },
            )?;
            let request = ResearchRequest {
                version: 1,
                selectors: vec![
                    Selector::parse(SelectorKind::Image, &imported.artifact_id).unwrap()
                ],
                sources: vec!["lenso".into()],
                rescan: true,
                safe_mode: true,
                options: ProviderOptions {
                    paid_authorized: true,
                    ..Default::default()
                },
            };
            let (plan, mut checks) =
                kokin_connectors::research::checks(&request, session.generation()).unwrap();
            let uploaded = "UNIQUE_BASE64_QUERY_PAYLOAD";
            checks[0].request.body =
                Some(json!({"image":uploaded,"category":"similar","page":1}).to_string());
            kokin_store::research::begin(
                &open.conn,
                "image-response",
                &serde_json::to_value(plan).unwrap(),
                1,
            )?;
            let response = kokin_net::HttpResponse {
                status: 200,
                headers: std::collections::BTreeMap::from([(
                    "content-type".into(),
                    "application/json".into(),
                )]),
                body: json!({"results":[]}).to_string(),
                body_bytes: None,
                redirect_chain: vec![],
                from_fixture: true,
            };
            let result = research::publish_response(open, "image-response", &checks[0], &response)?;
            assert_eq!(result.outcome, CheckOutcome::NotFound);
            let metadata: String = open.conn.query_row(
                "SELECT request_json FROM capture WHERE job_id='image-response'",
                [],
                |r| r.get(0),
            )?;
            assert!(!metadata.contains(uploaded));
            let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
            assert!(metadata["body"].is_null());
            assert_eq!(
                metadata["body_capture"]["image_artifact_id"],
                imported.artifact_id
            );
            assert_eq!(
                metadata["body_capture"]["body_blake3"],
                kokin_store::blake3_hex(checks[0].request.body.as_ref().unwrap().as_bytes())
            );
            Ok(())
        })
        .unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
