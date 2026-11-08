//! Synthetic encrypted-disk acceptance. No credentials, uploads or network.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_connectors::research::*;
use kokin_osint_lib::{research, research_import, Session};
use serde_json::json;
#[test]
fn hybrid_imports_provider_cards_and_encrypted_disk_pages() {
    let root = std::env::temp_dir().join(format!("kokin-phase2-finish-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let session = Session::default();
    session
        .create(
            &root.join("case"),
            "Phase2 synthetic",
            "synthetic finish passphrase",
        )
        .unwrap();
    let parent=session.with_case(|open| {
        let rows=kokin_connectors::research::parse_bulk("Case@example.org\nOther@example.org",SelectorKind::Email).unwrap();
        let doc=kokin_store::research::Document{id:"private-import".into(),revision:0,kind:"saved_search".into(),name:"Synthetic private import marker".into(),payload:json!({"version":1,"purpose":"import_session","rows":rows}),archived:false,updated_utc:String::new()};
        research_import::validate(open,&doc)?;
        let doc=kokin_store::research::save_document(&open.conn,&doc)?;
        let parent=ImportParent{id:doc.id.clone(),revision:doc.revision};
        let selector=Selector::parse(SelectorKind::Email,"Case@example.org").unwrap();
        research_import::validate_parent(open,&parent,std::slice::from_ref(&selector))?;
        assert!(research_import::validate_parent(open,&parent,&[Selector::parse(SelectorKind::Email,"Wrong@example.org").unwrap()]).is_err());
        let request=ResearchRequest{version:1,selectors:vec![selector.clone()],sources:vec!["osint_industries".into()],rescan:false,safe_mode:true,options:ProviderOptions{paid_authorized:true,..Default::default()}};
        let (mut plan,checks)=checks(&request,session.generation()).unwrap();plan.parent_import=Some(parent.clone());
        kokin_store::research::begin(&open.conn,"hybrid-fixture",&serde_json::to_value(&plan).unwrap(),1)?;
        let body=json!([{"module":"synthetic_service","status":"found","query":"Case@example.org","spec_format":[{"registered":{"type":"bool","value":true},"username":{"type":"str","value":"synthetic_user"},"name":{"type":"str","value":"Synthetic Jane"},"verified":{"type":"bool","value":false},"private":{"type":"bool","value":true},"age":{"type":"str","value":"32"},"creation_date":{"type":"str","value":"2020-04-12T12:00:00Z"}},{"registered":{"type":"bool","value":true},"username":{"type":"str","value":"second_user"},"verified":{"type":"bool","value":true},"private":{"type":"bool","value":false}}]}]);
        let response=kokin_net::HttpResponse{status:200,headers:std::collections::BTreeMap::from([("content-type".into(),"application/json".into())]),body:body.to_string(),body_bytes:None,redirect_chain:vec![],from_fixture:true};
        let result=research::publish_response(open,"hybrid-fixture",&checks[0],&response)?;
        assert_eq!(result.outcome,CheckOutcome::Found);assert_eq!(result.fields["cards"][0]["fields"]["age"],32);
        let filter=kokin_store::research::ResultFilter{module:"synthetic_service".into(),verified:"false".into(),private:"true".into(),min_age:Some(30),date_from:"2020-01-01".into(),..Default::default()};
        assert_eq!(kokin_store::research::filtered_results(&open.conn,"hybrid-fixture",0,&filter)?.page.total,1);
        let proposal=research::review_card_candidate(open,&result.id,0)?;assert_eq!(proposal["value"],"synthetic_user");assert_eq!(proposal["proposed_type"],"social_account");
        assert_eq!(open.conn.query_row("SELECT COUNT(*) FROM entity",[],|r|r.get::<_,i64>(0))?,0);
        let local=research::local_search(open,vec![Selector::parse(SelectorKind::Name,"Synthetic Jane").unwrap()],0)?;
        assert_eq!(local["external_requests"],0);assert!(local["queries"][0]["total"].as_i64().unwrap()>0);
        let unsupported=research::local_search(open,vec![Selector::parse(SelectorKind::Image,"synthetic-image-id").unwrap()],0)?;
        assert_eq!(unsupported["queries"][0]["status"],"unsupported");
        kokin_store::research::finish(&open.conn,"hybrid-fixture","completed")?;
        let state=research_import::state(open,&doc.id,0)?;assert_eq!(state["submitted_keys"],json!(["email:Case@example.org"]));assert_eq!(state["runs"]["rows"][0]["status"],"completed");
        let edited=kokin_store::research::save_document(&open.conn,&doc)?;
        assert!(research_import::validate_parent(open,&parent,std::slice::from_ref(&selector)).is_err());
        let parent=ImportParent{id:edited.id,revision:edited.revision};
        // 10k synthetic projections across ten bounded runs on real SQLCipher disk.
        let started=std::time::Instant::now();
        let tx=open.conn.unchecked_transaction()?;
        for run in 0..10 {
            tx.execute("INSERT INTO research_run(id,plan_json,status,created_utc,expected,completed) VALUES(?1,'{}','completed','2026-10-20T00:00:00Z',1000,1000)",[format!("disk-{run}")])?;
            for n in 0..1000 {
                let id=format!("disk-{run}-{n}");
                let payload=json!({"id":id,"run_id":format!("disk-{run}"),"source":"synthetic_disk","outcome":"unknown","artifact_id":null,"fields":{"cards":[{"module":"synthetic","fields":{"verified":false,"age":32,"name":format!("Synthetic {n}")}}]}});
                tx.execute("INSERT INTO research_result(id,run_id,cache_key,outcome,payload_json,created_epoch) VALUES(?1,?2,'synthetic','unknown',?3,0)",rusqlite::params![id,format!("disk-{run}"),payload.to_string()])?;
            }
        }
        tx.commit()?;
        let write_ms=started.elapsed().as_millis();let started=std::time::Instant::now();
        let filter=kokin_store::research::ResultFilter{verified:"false".into(),pattern:"Synthetic*".into(),..Default::default()};
        let page=kokin_store::research::filtered_results(&open.conn,"disk-9",950,&filter)?;
        assert_eq!(page.page.total,1000);assert_eq!(page.page.rows.len(),50);
        let filter_ms=started.elapsed().as_millis();
        let receipt=json!({"date":"2026-10-20","architecture":std::env::consts::ARCH,"scope":"SQLCipher encrypted disk; 10000 synthetic result projections across ten runs; page/filter scans one 1000-result run; network/native model excluded","insert_10000_ms":write_ms,"filter_and_page_ms":filter_ms,"page_size":50,"matching":1000});
        eprintln!("phase2_encrypted_disk={receipt}");
        std::fs::write(root.join("benchmark.json"),receipt.to_string()).unwrap();
        let path=open.paths.database();let bytes=std::fs::read(path).unwrap();assert!(!bytes.windows(31).any(|w|w==b"Synthetic private import marker"));
        kokin_export::package(open,&root.join("phase2.pkg"),"synthetic package passphrase",&Default::default()).unwrap();
        Ok(parent)
    }).unwrap();
    session.close().unwrap();
    session
        .open(&root.join("case"), "synthetic finish passphrase")
        .unwrap();
    session
        .with_case(|open| {
            let state = research_import::state(open, &parent.id, 0)?;
            assert_eq!(state["document"]["revision"], parent.revision);
            assert_eq!(state["runs"]["total"], 1);
            assert_eq!(
                state["document"]["payload"]["rows"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            Ok(())
        })
        .unwrap();
    session.close().unwrap();
    kokin_export::import(
        &root.join("phase2.pkg"),
        &root.join("restored"),
        "synthetic package passphrase",
    )
    .unwrap();
    session
        .open(&root.join("restored"), "synthetic package passphrase")
        .unwrap();
    session.with_case(|open| {
        let state=research_import::state(open,&parent.id,0)?;assert_eq!(state["document"]["revision"],parent.revision);assert_eq!(state["runs"]["rows"][0]["completed"],1);
        let result=kokin_store::research::results(&open.conn,"hybrid-fixture",0)?.rows.remove(0);
        let id=result["id"].as_str().unwrap();let artifact=result["artifact_id"].as_str().unwrap();
        open.conn.execute("UPDATE blob SET wrapped_key=NULL,wrapped_nonce=NULL WHERE content_hash=(SELECT content_hash FROM artifact WHERE id=?1)",[artifact])?;
        assert!(research::review_card_candidate(open,id,0).is_err());
        let filter=kokin_store::research::ResultFilter{verified:"false".into(),..Default::default()};assert_eq!(kokin_store::research::filtered_results(&open.conn,"hybrid-fixture",0,&filter)?.page.total,0);
        Ok(())
    }).unwrap();
    session.close().unwrap();
    if let Ok(destination) = std::env::var("KOKIN_PHASE2_BENCHMARK") {
        std::fs::copy(root.join("benchmark.json"), destination).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}
