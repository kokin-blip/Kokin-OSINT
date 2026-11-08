//! Offline contracts: encrypted persistence, review history, combined search and shredding.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_osint_lib::{faces, read, views::*, write, IngestFileRequest, Session};
use rusqlite::params;
fn detection(angle: f32, page: u16, time: Option<u64>) -> kokin_faces::Detection {
    let mut vector = vec![0.; 128];
    vector[0] = angle.cos();
    vector[1] = angle.sin();
    kokin_faces::Detection {
        bbox: [1., 2., 50., 60.],
        landmarks: [[3., 4.]; 5],
        image_size: [200, 200],
        detection_score: 0.95,
        vector,
        crop_png: b"synthetic crop contract fixture".to_vec(),
        page,
        timestamp_ms: time,
    }
}
fn output(d: Vec<kokin_faces::Detection>) -> kokin_faces::Output {
    kokin_faces::Output {
        version: 1,
        detections: d,
        preview_png: vec![],
        sampled_frames: 1,
        last_sample_ms: None,
        sampled_timestamps_ms: vec![],
        complete: true,
        limitations: vec!["Synthetic protocol fixture, not native inference".into()],
    }
}
fn request(media: &str, page: u16) -> kokin_faces::Request {
    kokin_faces::Request {
        version: 1,
        media_type: media.into(),
        page,
        start_ms: 0,
        end_ms: None,
        preview: false,
    }
}
#[test]
fn local_faces_review_combined_search_encryption_and_restore() {
    let dir = std::env::temp_dir().join(format!("kokin-faces-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let session = Session::default();
    session
        .create(&dir.join("source"), "face-test", "synthetic secret")
        .unwrap();
    let mut artifacts = Vec::new();
    for i in 0..3 {
        let path = dir.join(format!("image-{i}.png"));
        std::fs::write(&path, format!("synthetic source {i}")).unwrap();
        artifacts.push(
            session
                .with_case(|open| {
                    write::ingest_file(
                        open,
                        &IngestFileRequest {
                            path: path.to_string_lossy().into(),
                        },
                    )
                })
                .unwrap()
                .artifact_id,
        );
    }
    let selected=session.with_case(|open|{
  let tx=open.conn.transaction()?;for(i,a)in artifacts.iter().enumerate(){faces::commit(&tx,a,&request("image/png",0),&output(vec![detection(if i==2{1.5}else{0.05*i as f32},0,None)]))?;}tx.commit()?;
  assert_eq!(read::coverage(&open.conn)?.never_attempted,3,"Face analysis must not claim text coverage");
  let ids:Vec<String>=open.conn.prepare("SELECT id FROM face_detection ORDER BY rowid")?.query_map([],|r|r.get(0))?.collect::<Result<_,_>>()?;
  let query=FaceSearchRequest{version:1,face_id:ids[0].clone(),query:"".into(),text_mode:"lexical".into(),both:false,threshold:0.363,offset:0,limit:20};
  let related=SearchView{hits:vec![],total:0,coverage:read::coverage(&open.conn)?};let found=faces::search(open,&query,related.clone())?;assert_eq!(found.total,1);assert_eq!(found.matches[0].face.artifact_id,artifacts[1]);
  // A text observation maps to its artifact, rather than mistaking its own ID for an artifact.
  let run:String=open.conn.query_row("SELECT id FROM transform_run LIMIT 1",[],|r|r.get(0))?;
  open.conn.execute("INSERT INTO observation VALUES('topic-observation',?1,?2,'ocr.word','harbor project','{}',?3)",params![artifacts[1],run,kokin_store::now_utc_rfc3339()])?;
  let related=SearchView{hits:vec![HitView{subject_kind:"observation".into(),subject_id:"topic-observation".into(),title:"harbor".into(),snippet:"project".into(),facet:"ocr.word".into(),superseded:false,canonical_id:None}],total:1,coverage:read::coverage(&open.conn)?};let both=faces::search(open,&FaceSearchRequest{both:true,query:"harbor".into(),..query},related)?;assert_eq!(both.total,1);
  let person=write::create_entity(open,&NewEntityRequest{type_key:"person".into(),display_name:"Reviewed test person".into(),notes:"analyst fixture".into(),evidence:vec![GroundingInput{kind:"artifact".into(),id:artifacts[1].clone(),role:"context".into()}]})?.id;
  faces::review(&mut open.conn,&FaceReviewRequest{face_id:ids[1].clone(),entity_id:Some(person.clone()),group_id:None,decision:"accepted".into(),rationale:"manual visual review fixture".into()})?;
  assert_eq!(faces::face(&open.conn,&ids[1],true)?.entity_id,Some(person));
  assert!(faces::review(&mut open.conn,&FaceReviewRequest{face_id:ids[1].clone(),entity_id:None,group_id:None,decision:"accepted".into(),rationale:"".into()}).is_err());
  // Rerunning the page marks old detections stale but keeps their review.
  let tx=open.conn.transaction()?;faces::commit(&tx,&artifacts[1],&request("image/png",0),&output(vec![detection(0.05,0,None)]))?;tx.commit()?;
  assert!(faces::face(&open.conn,&ids[1],true)?.stale);assert!(faces::face(&open.conn,&ids[1],true)?.entity_id.is_some());
  assert!(open.conn.execute("DELETE FROM face_review",[]).is_err());
  kokin_export::package(open,&dir.join("faces.kokinpkg"),"export secret",&kokin_export::PackageOptions::default()).unwrap();
  Ok(ids[0].clone())
 }).unwrap();
    session.close().unwrap();
    let db = std::fs::read(dir.join("source/case.db")).unwrap();
    assert!(!db
        .windows(b"Reviewed test person".len())
        .any(|w| w == b"Reviewed test person"));
    kokin_export::import(
        &dir.join("faces.kokinpkg"),
        &dir.join("restored"),
        "export secret",
    )
    .unwrap();
    session
        .open(&dir.join("restored"), "export secret")
        .unwrap();
    session.with_case(|open|{
  let face=faces::face(&open.conn,&selected,true)?;assert!(face.crop_png.is_some());
  let hash:String=open.conn.query_row("SELECT content_hash FROM artifact WHERE id=?1",[&face.artifact_id],|r|r.get(0))?;
  open.conn.execute("UPDATE blob SET wrapped_key=NULL,wrapped_nonce=NULL,shredded_utc=?2 WHERE content_hash=?1",params![hash,kokin_store::now_utc_rfc3339()])?;
  let shredded=faces::face(&open.conn,&selected,true)?;assert!(shredded.stale&&shredded.crop_png.is_none());
  let vector:Option<Vec<u8>>=open.conn.query_row("SELECT vector FROM face_detection WHERE id=?1",[&selected],|r|r.get(0))?;assert!(vector.is_none());
  Ok(())
 }).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn search_separates_model_versions_and_runs_do_not_supersede_other_pages() {
    let dir = std::env::temp_dir().join(format!("kokin-faces-pages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let s = Session::default();
    s.create(&dir.join("case"), "pages", "secret").unwrap();
    let path = dir.join("test.pdf");
    std::fs::write(&path, b"%PDF-synthetic").unwrap();
    s.with_case(|open| {
        let a = write::ingest_file(
            open,
            &IngestFileRequest {
                path: path.to_string_lossy().into(),
            },
        )?
        .artifact_id;
        let tx = open.conn.transaction()?;
        faces::commit(
            &tx,
            &a,
            &request("application/pdf", 0),
            &output(vec![detection(0., 0, None)]),
        )?;
        faces::commit(
            &tx,
            &a,
            &request("application/pdf", 1),
            &output(vec![detection(0., 1, None)]),
        )?;
        tx.commit()?;
        let count: i64 = open.conn.query_row(
            "SELECT count(*) FROM face_detection WHERE stale=0",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 2);
        let tx = open.conn.transaction()?;
        faces::commit(&tx, &a, &request("application/pdf", 0), &output(vec![]))?;
        tx.commit()?;
        let count: i64 = open.conn.query_row(
            "SELECT count(*) FROM face_detection WHERE stale=0",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        let status = faces::status(open, false, false)?;
        assert!(!status.auto_analyze);
        assert_eq!(status.faces, 1);
        Ok(())
    })
    .unwrap();
    s.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn discovery_parsing_treats_results_as_text_and_requires_structure() {
    let hits=kokin_osint_lib::discovery::parse(r#"{"results":[{"title":"<script>untrusted</script>","url":"https://example.org","content":"a lead"},{"url":"javascript:alert(1)"}]}"#).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].snippet, "a lead");
    assert!(kokin_osint_lib::discovery::parse("{}").is_err());
}
#[test]
fn exact_search_10000_face_comparison_benchmark() {
    let query = vec![1. / (128f32).sqrt(); 128];
    let start = std::time::Instant::now();
    let vectors = vec![query.clone(); 10_000];
    let scores: Vec<_> = vectors
        .iter()
        .map(|v| kokin_faces::cosine(&query, v).unwrap())
        .collect();
    assert_eq!(scores.len(), 10_000);
    assert!(scores.iter().all(|s| *s > 0.999));
    println!("10,000 exact face comparisons: {:?}", start.elapsed());
}

#[test]
fn encrypted_10000_face_case_search_benchmark() {
    let dir = std::env::temp_dir().join(format!("kokin-faces-10000-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let s = Session::default();
    s.create(&dir.join("case"), "benchmark", "synthetic benchmark secret")
        .unwrap();
    let path = dir.join("source.png");
    std::fs::write(&path, b"synthetic benchmark evidence").unwrap();
    s.with_case(|open|{let a=write::ingest_file(open,&IngestFileRequest{path:path.to_string_lossy().into()})?.artifact_id;let tx=open.conn.transaction()?;faces::commit(&tx,&a,&request("image/png",0),&output(vec![detection(0.,0,None);10_000]))?;tx.commit()?;
 let id:String=open.conn.query_row("SELECT id FROM face_detection ORDER BY id LIMIT 1",[],|r|r.get(0))?;let query=FaceSearchRequest{version:1,face_id:id,query:"".into(),text_mode:"lexical".into(),both:false,threshold:0.363,offset:0,limit:20};let related=SearchView{hits:vec![],total:0,coverage:read::coverage(&open.conn)?};let started=std::time::Instant::now();let result=faces::search(open,&query,related)?;let elapsed=started.elapsed();assert_eq!(result.total,1);assert_eq!(result.matches[0].occurrences.len(),49);assert_eq!(result.matches[0].occurrence_count,9999);assert_eq!(result.coverage.faces,10_000);println!("Encrypted 10,000-face case: exact scan, rank, deduplication and bounded occurrence hydration in {elapsed:?}");Ok(())}).unwrap();
    s.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn overlapping_video_jobs_preserve_unsampled_sightings() {
    let dir = std::env::temp_dir().join(format!("kokin-faces-overlap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let session = Session::default();
    session
        .create(&dir.join("case"), "video-overlap", "synthetic secret")
        .unwrap();
    let path = dir.join("source.avi");
    std::fs::write(&path, b"synthetic frame-index persistence fixture").unwrap();
    session.with_case(|open|{let a=write::ingest_file(open,&IngestFileRequest{path:path.to_string_lossy().into()})?.artifact_id;
  for start in [0,1000] {let mut req=request("video/x-msvideo",0);req.start_ms=start;req.end_ms=Some(4000);let mut result=output(vec![detection(0.,0,Some(start)),detection(0.,0,Some(start+2000))]);result.sampled_frames=2;result.sampled_timestamps_ms=vec![start,start+2000];result.last_sample_ms=Some(start+2000);let tx=open.conn.transaction()?;faces::commit(&tx,&a,&req,&result)?;tx.commit()?;}
  let count:i64=open.conn.query_row("SELECT count(*) FROM face_detection WHERE stale=0",[],|r|r.get(0))?;assert_eq!(count,4);
  let mut req=request("video/x-msvideo",0);req.start_ms=2000;req.end_ms=Some(4000);let mut result=output(vec![]);result.sampled_timestamps_ms=vec![2000];result.last_sample_ms=Some(2000);let tx=open.conn.transaction()?;faces::commit(&tx,&a,&req,&result)?;tx.commit()?;
  let current:Vec<i64>=open.conn.prepare("SELECT json_extract(locator_json,'$.timestamp_ms') FROM face_detection WHERE stale=0 ORDER BY 1")?.query_map([],|r|r.get(0))?.collect::<Result<_,_>>()?;assert_eq!(current,vec![0,1000,3000]);Ok(())}).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
