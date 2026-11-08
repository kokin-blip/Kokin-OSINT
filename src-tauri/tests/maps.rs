#![allow(clippy::unwrap_used)]
use kokin_osint_lib::views::{LocationRequest, MapIllustrationRequest};
use kokin_osint_lib::{maps, read, reports, write, IngestFileRequest, Session};
use serde_json::json;
#[test]
fn grounded_locations_cameras_and_cited_illustrations_survive_restore_without_basemap() {
    let dir = std::env::temp_dir().join(format!("kokin-map-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let case = Session::default();
    case.create(&dir.join("case"), "maps", "synthetic credential")
        .unwrap();
    let source = dir.join("source.html");
    std::fs::write(&source,"<html><p>The synthetic source describes Mercer in Phoenix, with uncertain location.</p></html>").unwrap();
    let (lid,mid)=case.with_case(|open|{
  let artifact=write::ingest_file(open,&IngestFileRequest{path:source.to_string_lossy().into()})?.artifact_id;
  kokin_extract::extract_artifact(&mut open.conn,&kokin_blob::BlobStore::new(open.paths.blobs()),&open.cmk,&artifact,Default::default()).unwrap();
  let oid:String=open.conn.query_row("SELECT id FROM observation WHERE kind='page.text'",[],|r|r.get(0))?;
  let mut request=LocationRequest{observation_id:oid.clone(),entity_id:None,label:"Phoenix region".into(),longitude:-112.07,latitude:33.45,uncertainty:"Approximate city center, not exact premises".into(),rationale:"Synthetic text names the city; analyst entered coordinates manually".into()};
  let lid=maps::create_location(open,&request)?;request.longitude=f64::NAN;assert!(maps::create_location(open,&request).is_err());request.longitude = -112.07;request.observation_id="missing".into();assert!(maps::create_location(open,&request).is_err());
  let state=maps::state_read(&open.conn)?;let saved=maps::state_save(&mut open.conn,state.clone())?;assert_eq!(saved.revision,1);assert!(maps::state_save(&mut open.conn,state).is_err());
  let mut png=std::io::Cursor::new(Vec::new());image::DynamicImage::new_rgb8(32,32).write_to(&mut png,image::ImageFormat::Png).unwrap();use base64::Engine;
  let illustration=maps::save_illustration(open,&MapIllustrationRequest{image:format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png.into_inner())),archive_id:"a".repeat(64),attribution:"Synthetic map fixture <script>bad()</script>".into(),location_ids:vec![lid.clone()]})?;let mid=illustration["illustration_id"].as_str().unwrap().to_owned();
  let payload=json!({"version":1,"summary":"Synthetic map report","findings":[{"heading":"Location","text":"Approximate analyst-entered coordinates","citations":[{"kind":"observation","id":oid,"role":"context"}]}],"limitations":"No authenticity inferred","boards":[],"map_illustrations":[mid]});
  kokin_store::workspace::save(&open.conn,&kokin_store::workspace::SaveWorkspace{id:"report".into(),kind:"report".into(),name:"Map report".into(),expected_revision:0,payload,archived:false}).unwrap();
  let text=reports::render(open,"report","html",1,&std::sync::atomic::AtomicBool::new(false))?;assert!(text.contains("data:image/png;base64,"));assert!(!text.contains("<script>"));assert!(text.contains("Approximate city center"));
  assert_eq!(read::subject(open,"location",&lid)?.evidence[0].evidence_id,oid);assert_eq!(read::subject(open,"map_illustration",&mid)?.evidence[0].evidence_id,oid);
  assert!(open.conn.execute("DELETE FROM grounded_location",[]).is_err());kokin_export::package(open,&dir.join("case.pkg"),"package secret",&Default::default()).unwrap();Ok((lid,mid))
 }).unwrap();
    case.close().unwrap();
    kokin_export::import(
        &dir.join("case.pkg"),
        &dir.join("restored"),
        "package secret",
    )
    .unwrap();
    case.open(&dir.join("restored"), "package secret").unwrap();
    case.with_case(|open| {
        assert_eq!(maps::state_read(&open.conn)?.revision, 1);
        assert_eq!(maps::illustration(open, &mid)?.2.len(), 1);
        assert!(read::subject(open, "location", &lid).is_ok());
        assert!(reports::render(
            open,
            "report",
            "html",
            1,
            &std::sync::atomic::AtomicBool::new(false)
        )?
        .contains("data:image/png;base64,"));
        Ok(())
    })
    .unwrap();
    case.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
