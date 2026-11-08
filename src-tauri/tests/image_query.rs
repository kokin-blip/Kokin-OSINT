//! Synthetic storage/lineage acceptance; pixel/decoder behavior has separate native tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use kokin_osint_lib::{image_query, media, write, IngestFileRequest, Session};
use kokin_osint_lib::{ClipboardImageRequest, ImagePrepareRequest, ImageSearchRequest};
use serde_json::json;
fn png(color: u8) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        8,
        4,
        image::Rgba([color, 20, 30, 255]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    bytes.into_inner()
}
fn output(recipe: &kokin_media::image_query::Recipe, color: u8) -> kokin_media::Result {
    kokin_media::Result {
        version: 1,
        engine: "synthetic-protocol-fixture;query-prepare-1".into(),
        observations: vec![kokin_media::Observation {
            kind: "image.query_recipe".into(),
            value: serde_json::to_string(recipe).unwrap(),
            locator: json!({"scheme":"query_recipe","version":1,"page":0}),
        }],
        thumbnail_png: png(color),
        perceptual_hash: Some("0000000000000000".into()),
        pages: 1,
        gaps: vec!["Synthetic protocol fixture, not a native pixel evaluation".into()],
    }
}
#[test]
fn prepared_queries_duplicates_package_and_descendant_shredding() {
    let root = std::env::temp_dir().join(format!("kokin-query-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let session = Session::default();
    session
        .create(
            &root.join("case"),
            "image-query",
            "synthetic image passphrase",
        )
        .unwrap();
    let (original, derived, nested) = session
        .with_case(|open| {
            let path = root.join("original.png");
            let bytes = png(10);
            std::fs::write(&path, &bytes).unwrap();
            let source = write::ingest_file(
                open,
                &IngestFileRequest {
                    path: path.to_string_lossy().into_owned(),
                },
            )?
            .artifact_id;
            let duplicate = write::ingest_file(
                open,
                &IngestFileRequest {
                    path: path.to_string_lossy().into_owned(),
                },
            )?
            .artifact_id;
            let exact = image_query::process_search(image_query::prepare_search(
                open,
                ImageSearchRequest {
                    artifact_id: source.clone(),
                    mode: "exact".into(),
                    ..Default::default()
                },
            )?)?;
            assert_eq!(exact["total"], 1);
            assert_eq!(exact["rows"][0]["artifact_id"], duplicate);
            assert_eq!(exact["external_requests"], 0);
            assert!(exact["rows"][0]["artifact_created_utc"].is_string());
            assert!(exact["rows"][0].get("captured_utc").is_none());
            assert!(image_query::prepare_search(
                open,
                ImageSearchRequest {
                    artifact_id: source.clone(),
                    mode: "similar".into(),
                    ..Default::default()
                }
            )
            .is_err());
            let recipe = kokin_media::image_query::Recipe {
                version: 1,
                quarter_turns: 1,
                ..Default::default()
            };
            let request = ImagePrepareRequest {
                version: 1,
                artifact_id: source.clone(),
                page: 0,
                recipe: recipe.clone(),
                selected_face_id: None,
                obscure_other_faces: false,
            };
            // Synthetic face protocol fixtures exercise authoritative source/page
            // selection and opaque exclusions independently from native inference.
            let detections=[([0.,0.,2.,2.]),([4.,0.,4.,4.])].into_iter().map(|bbox| {
                let mut vector=vec![0.;128];vector[0]=1.;
                kokin_faces::Detection{bbox,landmarks:[[0.,0.];5],image_size:[8,4],detection_score:0.95,vector,crop_png:b"synthetic face crop".to_vec(),page:0,timestamp_ms:None}
            }).collect();
            let tx=open.conn.transaction()?;
            kokin_osint_lib::faces::commit(&tx,&source,&kokin_faces::Request{version:1,media_type:"image/png".into(),page:0,start_ms:0,end_ms:None,preview:false},&kokin_faces::Output{version:1,detections,preview_png:vec![],sampled_frames:1,last_sample_ms:None,sampled_timestamps_ms:vec![],complete:true,limitations:vec!["Synthetic face protocol fixture".into()]})?;tx.commit()?;
            let face_ids:Vec<String>=open.conn.prepare("SELECT id FROM face_detection WHERE artifact_id=?1 ORDER BY rowid")?.query_map([&source],|r|r.get(0))?.collect::<Result<_,_>>()?;
            let face_request=ImagePrepareRequest{selected_face_id:Some(face_ids[0].clone()),obscure_other_faces:true,..request.clone()};
            let (reviewed,ids)=image_query::prepare_request(open,&face_request)?;
            assert_eq!(ids.len(),2);assert_eq!(reviewed.recipe.obscure,vec![kokin_media::image_query::Rect{x:500,y:0,width:500,height:1000}]);
            assert!(image_query::prepare_request(open,&ImagePrepareRequest{page:1,..face_request.clone()}).is_err());
            assert!(image_query::prepare_request(open,&ImagePrepareRequest{artifact_id:duplicate.clone(),..face_request.clone()}).is_err());
            open.conn.execute("UPDATE face_detection SET stale=1 WHERE id=?1",[&face_ids[0]])?;
            assert!(image_query::prepare_request(open,&face_request).is_err());
            let mut wrong = output(&recipe, 50);
            wrong.observations[0].value = "{}".into();
            assert!(image_query::commit(open, &request, &[], wrong).is_err());
            let mut prepared_output=output(&recipe,50);prepared_output.pages=7;
            let result = image_query::commit(open, &request, &[], prepared_output)?;
            let derived = result["artifact_id"].as_str().unwrap().to_owned();
            let request2 = ImagePrepareRequest {
                artifact_id: derived.clone(),
                ..request
            };
            let result = image_query::commit(open, &request2, &[], output(&recipe, 100))?;
            let nested = result["artifact_id"].as_str().unwrap().to_owned();
            assert_eq!(media::preview(open, &derived, 0)?.unwrap().pages,1);
            let oid:String=open.conn.query_row("SELECT id FROM observation WHERE artifact_id=?1 AND kind='image.query_recipe' LIMIT 1",[&derived],|r|r.get(0))?;
            let observed=kokin_osint_lib::read::observation(open,&oid)?;
            assert!(matches!(observed.quote,kokin_osint_lib::views::QuoteView::DerivedExact{..}));
            assert_eq!(
                open.conn
                    .query_row("SELECT COUNT(*) FROM image_query", [], |r| r
                        .get::<_, i64>(0))?,
                2
            );
            let count: i64 = open
                .conn
                .query_row("SELECT COUNT(*) FROM entity", [], |r| r.get(0))?;
            assert_eq!(count, 0);
            let near = image_query::process_search(image_query::prepare_search(
                open,
                ImageSearchRequest {
                    artifact_id: nested.clone(),
                    max_distance: 0,
                    ..Default::default()
                },
            )?)?;
            assert_eq!(near["total"], 1);
            assert_eq!(near["rows"][0]["artifact_id"], derived);
            assert_eq!(near["rows"][0]["publication_utc"], serde_json::Value::Null);
            let filtered = image_query::process_search(image_query::prepare_search(
                open,
                ImageSearchRequest {
                    artifact_id: nested.clone(),
                    domain: "example.org".into(),
                    ..Default::default()
                },
            )?)?;
            assert_eq!(filtered["total"], 0);
            let copied: Vec<u8> = kokin_blob::BlobStore::new(open.paths.blobs())
                .get(
                    &match kokin_store::blob_access(
                        &open.conn,
                        &open.conn.query_row(
                            "SELECT content_hash FROM artifact WHERE id=?1",
                            [&source],
                            |r| r.get::<_, String>(0),
                        )?,
                    )? {
                        kokin_store::BlobAccess::Readable(r) => r,
                        _ => panic!("source lost"),
                    },
                    &open.cmk,
                )
                .unwrap();
            assert_eq!(copied, bytes);
            kokin_export::package(
                open,
                &root.join("query.pkg"),
                "synthetic package passphrase",
                &Default::default(),
            )
            .unwrap();
            Ok((source, derived, nested))
        })
        .unwrap();
    session.close().unwrap();
    kokin_export::import(
        &root.join("query.pkg"),
        &root.join("restored"),
        "synthetic package passphrase",
    )
    .unwrap();
    session
        .open(&root.join("restored"), "synthetic package passphrase")
        .unwrap();
    session.with_case(|open|{
  assert_eq!(open.conn.query_row("SELECT COUNT(*) FROM image_query",[],|r|r.get::<_,i64>(0))?,2);
  open.conn.execute("UPDATE blob SET wrapped_key=NULL,wrapped_nonce=NULL,shredded_utc='2026-10-20T00:00:00Z' WHERE content_hash=(SELECT content_hash FROM artifact WHERE id=?1)",[&original])?;
  for id in [&derived,&nested]{assert!(image_query::prepare_search(open,ImageSearchRequest{artifact_id:id.clone(),..Default::default()}).is_err());assert!(media::preview(open,id,0).is_err());}
  assert_eq!(open.conn.query_row("SELECT COUNT(*) FROM blob WHERE wrapped_key IS NOT NULL",[],|r|r.get::<_,i64>(0))?,0);Ok(())
 }).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn clipboard_input_records_origin_and_rejects_encoding_or_type_confusion() {
    use base64::Engine;
    let root = std::env::temp_dir().join(format!("kokin-clipboard-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let session = Session::default();
    session
        .create(
            &root.join("case"),
            "clipboard",
            "synthetic clipboard passphrase",
        )
        .unwrap();
    session
        .with_case(|open| {
            assert!(image_query::import_clipboard(
                open,
                "clip",
                ClipboardImageRequest {
                    version: 1,
                    media_type: "image/png".into(),
                    base64: "not-base64".into()
                }
            )
            .is_err());
            let data = base64::engine::general_purpose::STANDARD.encode(png(1));
            let result = image_query::import_clipboard(
                open,
                "clip",
                ClipboardImageRequest {
                    version: 1,
                    media_type: "image/png".into(),
                    base64: data.clone(),
                },
            )?;
            assert_eq!(result["uploaded"], false);
            assert!(image_query::import_clipboard(
                open,
                "clip",
                ClipboardImageRequest {
                    version: 1,
                    media_type: "image/jpeg".into(),
                    base64: data
                }
            )
            .is_err());
            assert_eq!(
                open.conn
                    .query_row("SELECT kind FROM source LIMIT 1", [], |r| r
                        .get::<_, String>(0))?,
                "clipboard"
            );
            assert!(open.conn.query_row(
                "SELECT http_status IS NULL FROM capture LIMIT 1",
                [],
                |r| r.get::<_, bool>(0)
            )?);
            Ok(())
        })
        .unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn encrypted_ten_thousand_image_candidates_are_bounded_filtered_and_paged() {
    let root = std::env::temp_dir().join(format!("kokin-image-scale-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let session = Session::default();
    session
        .create(
            &root.join("case"),
            "image-scale",
            "synthetic image scale passphrase",
        )
        .unwrap();
    session.with_case(|open| {
        let path=root.join("source.png");std::fs::write(&path,png(21)).unwrap();
        let source=write::ingest_file(open,&IngestFileRequest{path:path.to_string_lossy().into_owned()})?.artifact_id;
        let start=std::time::Instant::now();
        let tx=open.conn.transaction()?;
        for n in 0..10001 {
            tx.execute("INSERT INTO artifact SELECT ?1,content_hash,media_type,byte_length,created_utc FROM artifact WHERE id=?2",rusqlite::params![format!("image-scale-{n:05}"),source])?;
        }
        tx.commit()?;
        let mut measurements=vec![];
        for budget in [500,5000,20000] {
            let start=std::time::Instant::now();
            let result=image_query::process_search(image_query::prepare_search(open,ImageSearchRequest{artifact_id:source.clone(),mode:"exact".into(),scan_limit:budget,..Default::default()})?)?;
            assert_eq!(result["scanned"],budget.min(10001));assert_eq!(result["rows"].as_array().unwrap().len(),50);
            assert_eq!(result["total"],budget.min(10001));assert_eq!(result["scan_truncated"],budget<10001);assert_eq!(result["external_requests"],0);
            measurements.push(json!({"budget":budget,"scanned":result["scanned"],"milliseconds":start.elapsed().as_millis()}));
        }
        let next=image_query::process_search(image_query::prepare_search(open,ImageSearchRequest{artifact_id:source.clone(),mode:"exact".into(),scan_limit:20000,offset:50,sort:"oldest".into(),..Default::default()})?)?;
        assert_eq!(next["rows"][0]["artifact_id"],"image-scale-00050");
        for request in [ImageSearchRequest{min_bytes:Some(10000),..Default::default()},ImageSearchRequest{title:"not established".into(),..Default::default()},ImageSearchRequest{date_from:"2099-01-01".into(),..Default::default()}] {
            let result=image_query::process_search(image_query::prepare_search(open,ImageSearchRequest{artifact_id:source.clone(),mode:"exact".into(),scan_limit:500,..request})?)?;
            assert_eq!(result["total"],0);
        }
        for request in [ImageSearchRequest{scan_limit:25000,..Default::default()},ImageSearchRequest{date_from:"2026-02-30".into(),..Default::default()},ImageSearchRequest{min_bytes:Some(50),max_bytes:Some(1),..Default::default()}] {
            assert!(image_query::prepare_search(open,ImageSearchRequest{artifact_id:source.clone(),..request}).is_err());
        }
        eprintln!("IMAGE_QUERY_BENCHMARK {}",json!({"synthetic":true,"storage":"encrypted disk case","platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"eligible_artifacts":10002,"fixture":"10001 captured artifact references to one synthetic PNG; exact hash mode; no decoder/model benchmark","measurements":measurements,"total_milliseconds":start.elapsed().as_millis()}));
        Ok(())
    }).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
