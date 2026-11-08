//! Case-scoped face jobs, exact comparisons and analyst review.
use crate::{views::*, CommandError, ErrorCode, Session};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Manager};
fn err(e: impl std::fmt::Display) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, e.to_string())
}
pub fn identity() -> Result<String, CommandError> {
    let r: serde_json::Value =
        serde_json::from_str(include_str!("../../resources/face-registry.json")).map_err(err)?;
    Ok(r["identity"]
        .as_str()
        .ok_or_else(|| err("face registry identity"))?
        .into())
}
fn id() -> Result<String, CommandError> {
    kokin_ai::persistence::id().map_err(err)
}
pub fn supported(media: &str) -> bool {
    crate::media::supported(media)
        || matches!(
            media,
            "video/mp4" | "video/quicktime" | "video/webm" | "video/x-matroska" | "video/x-msvideo"
        )
}
fn native_paths(app: &AppHandle) -> Result<(std::path::PathBuf, std::path::PathBuf), CommandError> {
    let binary = std::env::current_exe()
        .map_err(err)?
        .with_file_name("kokin-face-worker");
    let native = app.path().resource_dir().map_err(err)?.join("native");
    #[cfg(debug_assertions)]
    {
        if !binary.is_file()
            || !native
                .join("face-onnx/libonnxruntime.1.31.0.dylib")
                .is_file()
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            return Ok((
                root.join("../target/media-probe/MacOS/kokin-face-worker"),
                root.join("../target/media-probe/Resources/native"),
            ));
        }
    }
    Ok((binary, native))
}
pub fn status(
    open: &kokin_store::OpenCase,
    resources: bool,
    native: bool,
) -> Result<FaceStatusView, CommandError> {
    let eligible=open.conn.query_row("SELECT count(*) FROM artifact WHERE media_type IN ('image/jpeg','image/png','image/webp','image/tiff','application/pdf','video/mp4','video/quicktime','video/webm','video/x-matroska','video/x-msvideo')",[],|r|r.get(0))?;
    let analyzed = open.conn.query_row(
        "SELECT count(DISTINCT artifact_id) FROM face_run WHERE model_identity=?1",
        [identity()?],
        |r| r.get(0),
    )?;
    let faces=open.conn.query_row("SELECT count(*) FROM face_detection WHERE stale=0 AND vector IS NOT NULL AND model_identity=?1",[identity()?],|r|r.get(0))?;
    let auto_analyze = open.conn.query_row(
        "SELECT auto_analyze FROM face_preferences WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    Ok(FaceStatusView{eligible_artifacts:eligible,analyzed_artifacts:analyzed,faces,auto_analyze,resources_present:resources,native_available:native,model_identity:identity()?,limitation:"Case-local comparisons only. An analyzed PDF or video may have partial page/frame coverage. Similarity is not an identity probability; review all links.".into()})
}
#[tauri::command]
pub fn face_status(app: AppHandle, generation: u64) -> Result<FaceStatusView, CommandError> {
    let resources = crate::resources::selected(&app, "face-yunet")?.present
        && crate::resources::selected(&app, "face-sface")?.present;
    let (binary, native) = native_paths(&app)?;
    let native = cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && binary.is_file()
        && native
            .join("face-onnx/libonnxruntime.1.31.0.dylib")
            .is_file();
    app.state::<Session>()
        .with_generation(generation, |open| status(open, resources, native))
}
#[tauri::command]
pub fn face_auto_analyze(
    app: AppHandle,
    generation: u64,
    enabled: bool,
) -> Result<(), CommandError> {
    app.state::<Session>().with_generation(generation, |open| {
        let tx = open.conn.transaction()?;
        tx.execute(
            "UPDATE face_preferences SET auto_analyze=?1 WHERE singleton=1",
            [enabled],
        )?;
        audit(
            &tx,
            "faces.auto_analyze",
            None,
            &serde_json::json!({"enabled":enabled}),
        )?;
        tx.commit()?;
        Ok(())
    })
}
fn audit(
    conn: &rusqlite::Connection,
    action: &str,
    face: Option<&str>,
    payload: &serde_json::Value,
) -> Result<(), CommandError> {
    kokin_store::append_audit_event(
        conn,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action,
            subject_kind: face.map(|_| "face"),
            subject_id: face,
            payload_json: &payload.to_string(),
        },
    )?;
    Ok(())
}
pub fn run(
    app: &AppHandle,
    job: &str,
    generation: u64,
    request: FaceAnalysisRequest,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    if request.version != 1 || request.artifact_ids.is_empty() || request.artifact_ids.len() > 100 {
        return Err(err("select between 1 and 100 artifacts"));
    }
    let (binary, native) = native_paths(app)?;
    let models = crate::resources::root(app)?;
    for key in ["face-yunet", "face-sface"] {
        let r = crate::resources::selected(app, key)?;
        crate::resources::verify(
            std::path::Path::new(&r.destination),
            r.bytes,
            &r.sha256,
            cancel,
        )?;
    }
    let session = app.state::<Session>();
    let ops = app.state::<crate::operations::Operations>();
    let mut assembled = Vec::new();
    let mut seen = HashSet::new();
    for (index, artifact) in request.artifact_ids.iter().enumerate() {
        if !seen.insert(artifact) {
            continue;
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(err("face analysis cancelled before commit"));
        }
        ops.update(
            job,
            &format!(
                "Analyzing faces: evidence {} of {}",
                index + 1,
                request.artifact_ids.len()
            ),
            true,
        );
        let (media, hash, bytes) = session.with_generation(generation, |open| {
            let (media, hash): (String, String) = open.conn.query_row(
                "SELECT media_type,content_hash FROM artifact WHERE id=?1",
                [artifact],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if !supported(&media) {
                return Err(err("unsupported face media; source remains preserved"));
            }
            let max = if media.starts_with("video/") {
                kokin_faces::MAX_VIDEO
            } else {
                kokin_media::MAX_INPUT
            };
            let kokin_store::BlobAccess::Readable(reference) =
                kokin_store::blob_access(&open.conn, &hash)?
            else {
                return Err(err("source is unavailable"));
            };
            if reference.size > max as u64 {
                return Err(err("source exceeds face-processing size limit"));
            }
            let bytes = kokin_blob::BlobStore::new(open.paths.blobs())
                .get(&reference, &open.cmk)
                .map_err(err)?;
            Ok((media, hash, bytes))
        })?;
        let r = kokin_faces::Request {
            version: 1,
            media_type: media,
            page: request.page,
            start_ms: request.start_ms,
            end_ms: request.end_ms,
            preview: false,
        };
        r.validate().map_err(err)?;
        let result =
            kokin_media::face_worker::run(&binary, &native, &models, bytes, &r, cancel.clone())
                .map_err(err)?;
        if assembled
            .iter()
            .map(
                |a: &(String, String, kokin_faces::Request, kokin_faces::Output)| {
                    a.3.detections.len()
                },
            )
            .sum::<usize>()
            + result.detections.len()
            > kokin_faces::MAX_FACES
        {
            return Err(err("batch exceeds 10,000 faces; use smaller batches"));
        }
        assembled.push((artifact.clone(), hash, r, result));
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(err("face job cancelled"));
    }
    ops.begin_publish(job)?;
    session.with_generation(generation,|open|{
  let tx=open.conn.transaction()?;let mut total=0;
  for (artifact,hash,r,result) in &assembled{
   if !matches!(kokin_store::blob_access(&tx,hash)?,kokin_store::BlobAccess::Readable(_)){return Err(err("source became unavailable; prior index preserved"));}
   total+=commit(&tx,artifact,r,result)?;
  }
  audit(&tx,"faces.analyzed",None,&serde_json::json!({"artifacts":assembled.len(),"faces":total,"model":identity()?}))?;tx.commit()?;
  Ok(serde_json::json!({"artifacts":assembled.len(),"faces":total,"complete":assembled.iter().all(|a|a.3.complete),"limitations":assembled.iter().flat_map(|a|a.3.limitations.clone()).collect::<Vec<_>>()}))
 })
}
/// Commit only after all worker outputs have passed their processing bounds.
pub fn commit(
    conn: &rusqlite::Connection,
    artifact: &str,
    request: &kokin_faces::Request,
    result: &kokin_faces::Output,
) -> Result<usize, CommandError> {
    request.validate().map_err(err)?;
    if result.version != 1 || result.detections.len() > kokin_faces::MAX_FACES {
        return Err(err("invalid face output"));
    }
    let run = id()?;
    let now = kokin_store::now_utc_rfc3339();
    let model = identity()?;
    conn.execute("INSERT INTO transform_run VALUES(?1,'analyze.faces','1',?2,?3,?4,?4,'succeeded',NULL,NULL)",params![run,env!("CARGO_PKG_VERSION"),model,now])?;
    // face_run owns the artifact lineage. Generic run_subject feeds text
    // coverage, which a successful visual comparison must never mark complete.
    conn.execute(
        "INSERT INTO face_run VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            run,
            artifact,
            model,
            request.page,
            request.start_ms as i64,
            request.end_ms.map(|v| v as i64),
            result.sampled_frames as i64,
            result.complete,
            serde_json::to_string(&result.limitations).map_err(err)?,
            now
        ],
    )?;
    // Only replace frames actually decoded by this run. Overlapping ranges
    // with different sampling offsets must preserve earlier unsampled sightings.
    if request.media_type.starts_with("video/") {
        if result.sampled_timestamps_ms.len() != result.sampled_frames
            || result.sampled_frames > 1800
        {
            return Err(err("video sampling coverage mismatch"));
        }
        conn.execute("UPDATE face_detection SET stale=1 WHERE artifact_id=?1 AND stale=0 AND json_extract(locator_json,'$.timestamp_ms') IN(SELECT value FROM json_each(?2))",params![artifact,serde_json::to_string(&result.sampled_timestamps_ms).map_err(err)?])?;
    } else {
        conn.execute("UPDATE face_detection SET stale=1 WHERE artifact_id=?1 AND stale=0 AND json_extract(locator_json,'$.timestamp_ms') IS NULL AND json_extract(locator_json,'$.page')=?2",params![artifact,request.page])?;
    }
    for d in &result.detections {
        d.validate().map_err(err)?;
        let face = id()?;
        let locator = serde_json::json!({"scheme":"face_region","page":d.page,"timestamp_ms":d.timestamp_ms,"bbox":d.bbox,"landmarks":d.landmarks,"image_size":d.image_size,"preprocessing":"align112-rgb-v1"});
        conn.execute(
            "INSERT INTO face_detection VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,?9)",
            params![
                face,
                run,
                artifact,
                model,
                locator.to_string(),
                d.detection_score,
                kokin_faces::encode(&d.vector).map_err(err)?,
                d.crop_png,
                now
            ],
        )?;
    }
    Ok(result.detections.len())
}
pub fn face(conn: &rusqlite::Connection, id: &str, crop: bool) -> Result<FaceView, CommandError> {
    let mut view=conn.query_row("SELECT id,artifact_id,run_id,model_identity,locator_json,detector_response,CASE WHEN ?2 THEN crop_png ELSE NULL END,stale FROM face_detection WHERE id=?1",params![id,crop],|r|{
  let raw:String=r.get(4)?;let l:serde_json::Value=serde_json::from_str(&raw).map_err(|e|rusqlite::Error::FromSqlConversionFailure(4,rusqlite::types::Type::Text,Box::new(e)))?;
  let png:Option<Vec<u8>>=r.get(6)?;
  Ok(FaceView{id:r.get(0)?,artifact_id:r.get(1)?,run_id:r.get(2)?,model_identity:r.get(3)?,bbox:serde_json::from_value(l["bbox"].clone()).unwrap_or_default(),image_size:serde_json::from_value(l["image_size"].clone()).unwrap_or_default(),page:l["page"].as_u64().unwrap_or(0) as u16,timestamp_ms:l["timestamp_ms"].as_u64(),detection_score:r.get(5)?,crop_png:png.map(|p|base64::engine::general_purpose::STANDARD.encode(p)),stale:r.get(7)?,entity_id:None,entity_name:None})
 })?;
    let review:Option<(String,String)>=conn.query_row("SELECT e.id,e.display_name FROM face_review r JOIN entity e ON e.id=r.entity_id WHERE r.face_id=?1 AND r.group_id IS NULL AND r.decision='accepted' AND r.id=(SELECT max(id) FROM face_review WHERE face_id=?1 AND group_id IS NULL)",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((id, _name)) = review {
        let canonical = kokin_graph::resolution::canonical_id(conn, &id)?;
        let name = conn.query_row(
            "SELECT display_name FROM entity WHERE id=?1",
            [&canonical],
            |r| r.get(0),
        )?;
        view.entity_id = Some(canonical);
        view.entity_name = Some(name);
    }
    Ok(view)
}
#[tauri::command]
pub fn face_page(
    app: AppHandle,
    generation: u64,
    artifact_id: String,
    offset: usize,
    include_stale: bool,
    page_filter: Option<u16>,
) -> Result<FacePageView, CommandError> {
    if offset > 100_000 {
        return Err(err("face page offset limit"));
    }
    app.state::<Session>().with_generation(generation,|open|{
  let filter="(?1='' OR artifact_id=?1) AND (?2 OR stale=0) AND (?3 IS NULL OR (json_extract(locator_json,'$.page')=?3 AND json_extract(locator_json,'$.timestamp_ms') IS NULL))";
  let total=open.conn.query_row(&format!("SELECT count(*) FROM face_detection WHERE {filter}"),params![artifact_id,include_stale,page_filter],|r|r.get(0))?;
  let mut stmt=open.conn.prepare(&format!("SELECT id FROM face_detection WHERE {filter} ORDER BY artifact_id,json_extract(locator_json,'$.page'),json_extract(locator_json,'$.timestamp_ms'),id LIMIT 50 OFFSET ?4"))?;
  let ids=stmt.query_map(params![artifact_id,include_stale,page_filter,offset as i64],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
  let faces=ids.iter().map(|id|face(&open.conn,id,true)).collect::<Result<Vec<_>,_>>()?;
  let mut stmt=open.conn.prepare("SELECT limitations_json,complete,sampled_frames,page,start_ms,end_ms FROM face_run WHERE (?1='' OR artifact_id=?1) ORDER BY rowid DESC LIMIT 20")?;
  let limitations=stmt.query_map([artifact_id],|r|Ok(format!("Page {}, range {}–{} ms; {} sampled image(s); {}. {}",r.get::<_,u16>(3)?+1,r.get::<_,i64>(4)?,r.get::<_,Option<i64>>(5)?.map(|v|v.to_string()).unwrap_or_else(||"end".into()),r.get::<_,i64>(2)?,if r.get::<_,bool>(1)?{"requested range processed"}else{"partial: frame cap reached"},r.get::<_,String>(0)?)))?.collect::<Result<Vec<_>,_>>()?;
  Ok(FacePageView{faces,total,offset,limitations})
 })
}
#[tauri::command]
pub async fn face_search(
    app: AppHandle,
    generation: u64,
    request: FaceSearchRequest,
) -> Result<FaceSearchView, CommandError> {
    if (request.both && request.query.trim().is_empty())
        || request.version != 1
        || request.query.len() > 4096
        || request.limit == 0
        || request.limit > 50
        || request.offset > 100_000
        || !request.threshold.is_finite()
        || !(-1. ..=1.).contains(&request.threshold)
        || !matches!(
            request.text_mode.as_str(),
            "lexical" | "semantic" | "hybrid"
        )
    {
        return Err(err("invalid face search request"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let related_request = SearchRequest {
            query: request.query.clone(),
            limit: Some(1000),
            ..Default::default()
        };
        let mut text_truncated = false;
        let related = if request.query.trim().is_empty() {
            app.state::<Session>().with_generation(generation, |open| {
                Ok(SearchView {
                    hits: vec![],
                    total: 0,
                    coverage: crate::read::coverage(&open.conn)?,
                })
            })?
        } else if request.text_mode == "lexical" {
            app.state::<Session>().with_generation(generation, |open| {
                crate::read::search(open, &related_request)
            })?
        } else {
            // Embedding work must run outside the case lock, exactly as in semantic_search.
            let cancel = AtomicBool::new(false);
            let mut provider = crate::semantic::provider(&app, &cancel)?;
            use kokin_ai::embedding::EmbeddingProvider;
            let vector = provider.embed(&request.query, true).map_err(err)?;
            app.state::<Session>().with_generation(generation, |open| {
                let current: Option<String> = open
                    .conn
                    .query_row("SELECT model_identity FROM embedding_meta", [], |r| {
                        r.get(0)
                    })
                    .optional()?;
                if current.as_deref() != Some(provider.identity()) {
                    return Err(err("text embedding model changed; rebuild semantic index"));
                }
                let view = crate::semantic::search_vector(
                    open,
                    &SemanticRequest {
                        version: 1,
                        mode: request.text_mode.clone(),
                        search: related_request,
                    },
                    &vector,
                )?;
                text_truncated = view.truncated;
                Ok(view.results)
            })?
        };
        let coverage = face_status(app.clone(), generation)?;
        app.state::<Session>().with_generation(generation, |open| {
            let mut result = search(open, &request, related)?;
            result.coverage = coverage;
            result.truncated |= text_truncated;
            Ok(result)
        })
    })
    .await
    .map_err(|_| err("face search worker failed"))?
}
pub fn search(
    open: &kokin_store::OpenCase,
    request: &FaceSearchRequest,
    related: SearchView,
) -> Result<FaceSearchView, CommandError> {
    let selected = face(&open.conn, &request.face_id, false)?;
    if selected.stale || selected.model_identity != identity()? {
        return Err(err("query face is stale; analyze its source again"));
    }
    let bytes: Vec<u8> = open.conn.query_row(
        "SELECT vector FROM face_detection WHERE id=?1",
        [&request.face_id],
        |r| r.get(0),
    )?;
    let query = kokin_faces::decode(&bytes).map_err(err)?;
    let mut related_artifacts = HashSet::new();
    for hit in &related.hits {
        match hit.subject_kind.as_str() {
            "artifact" => {
                related_artifacts.insert(hit.subject_id.clone());
            }
            "observation" => {
                let artifact: String = open.conn.query_row(
                    "SELECT artifact_id FROM observation WHERE id=?1",
                    [&hit.subject_id],
                    |r| r.get(0),
                )?;
                related_artifacts.insert(artifact);
            }
            kind => {
                let mut subjects = vec![hit.subject_id.clone()];
                if kind == "entity" {
                    let canonical =
                        kokin_graph::resolution::canonical_id(&open.conn, &hit.subject_id)?;
                    subjects = kokin_graph::resolution::cluster(&open.conn, &canonical)?;
                    subjects.push(canonical);
                }
                for subject in subjects {
                    let mut links=open.conn.prepare("SELECT evidence_kind,evidence_id FROM evidence_link WHERE subject_kind=?1 AND subject_id=?2")?;
                    let evidence = links
                        .query_map(params![kind, subject], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    for (kind, id) in evidence {
                        if kind == "artifact" {
                            related_artifacts.insert(id);
                        } else if kind == "observation" {
                            let artifact: Option<String> = open
                                .conn
                                .query_row(
                                    "SELECT artifact_id FROM observation WHERE id=?1",
                                    [id],
                                    |r| r.get(0),
                                )
                                .optional()?;
                            if let Some(a) = artifact {
                                related_artifacts.insert(a);
                            }
                        } else {
                            let mut s=open.conn.prepare("SELECT a.id FROM artifact a JOIN capture c ON c.content_hash=a.content_hash WHERE c.id=?1")?;
                            for a in s.query_map([id], |r| r.get::<_, String>(0))? {
                                related_artifacts.insert(a?);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut stmt=open.conn.prepare("SELECT id,artifact_id,vector FROM face_detection WHERE stale=0 AND vector IS NOT NULL AND model_identity=?1 AND id!=?2 ORDER BY id LIMIT 100001")?;
    let mut rows = stmt.query(params![identity()?, request.face_id])?;
    let mut scored = Vec::new();
    let mut scanned = 0;
    while let Some(row) = rows.next()? {
        scanned += 1;
        if scanned > 100_000 {
            break;
        }
        let id: String = row.get(0)?;
        let artifact: String = row.get(1)?;
        if request.both && !related_artifacts.contains(&artifact) {
            continue;
        }
        let vector: Vec<u8> = row.get(2)?;
        let score = kokin_faces::cosine(&query, &kokin_faces::decode(&vector).map_err(err)?)
            .map_err(err)?;
        if score >= request.threshold {
            scored.push((id, artifact, score));
        }
    }
    scored.sort_by(|a, b| b.2.total_cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let mut by_artifact: HashMap<String, usize> = HashMap::new();
    let mut matches: Vec<(String, f32, Vec<String>)> = Vec::new();
    for (id, artifact, score) in scored {
        if let Some(index) = by_artifact.get(&artifact) {
            matches[*index].2.push(id);
        } else {
            by_artifact.insert(artifact, matches.len());
            matches.push((id, score, vec![]));
        }
    }
    let total = matches.len();
    let matches = matches
        .into_iter()
        .skip(request.offset)
        .take(request.limit)
        .map(|(id, similarity, others)| {
            Ok(FaceMatchView {
                face: face(&open.conn, &id, true)?,
                similarity,
                occurrence_count: others.len() + 1,
                occurrences: others
                    .iter()
                    .take(49)
                    .map(|id| face(&open.conn, id, false))
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    let truncated =
        scanned > 100_000 || (request.both && related.total > related.hits.len() as i64);
    Ok(FaceSearchView {
        matches,
        total,
        offset: request.offset,
        related,
        coverage: status(open, true, true)?,
        truncated,
    })
}
#[tauri::command]
pub fn face_review(
    app: AppHandle,
    generation: u64,
    request: FaceReviewRequest,
) -> Result<(), CommandError> {
    app.state::<Session>()
        .with_generation(generation, |open| review(&mut open.conn, &request))
}
pub fn review(conn: &mut rusqlite::Connection, r: &FaceReviewRequest) -> Result<(), CommandError> {
    if r.rationale.trim().is_empty()
        || r.rationale.len() > 8192
        || !matches!(
            r.decision.as_str(),
            "accepted" | "rejected" | "unlinked" | "excluded"
        )
    {
        return Err(err("select a review decision and provide a rationale"));
    }
    let face = face(conn, &r.face_id, false)?;
    if face.stale {
        return Err(err("stale face cannot receive a new review"));
    }
    if r.group_id.is_some() {
        if r.decision != "excluded" || r.entity_id.is_some() {
            return Err(err("group review must exclude a member"));
        }
        let member: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM face_group_member WHERE group_id=?1 AND face_id=?2)",
            params![r.group_id, r.face_id],
            |r| r.get(0),
        )?;
        if !member {
            return Err(err("face is not in that group"));
        }
    } else if r.decision == "accepted" {
        let person: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM entity WHERE id=?1 AND type_key='person')",
            [&r.entity_id],
            |r| r.get(0),
        )?;
        if !person {
            return Err(err("choose a person entity"));
        }
    } else if r.decision == "excluded" {
        return Err(err("exclusion requires a group"));
    }
    let tx = conn.transaction()?;
    tx.execute("INSERT INTO face_review(face_id,entity_id,group_id,decision,rationale,actor,created_utc) VALUES(?1,?2,?3,?4,?5,'user:local',?6)",params![r.face_id,r.entity_id,r.group_id,r.decision,r.rationale,kokin_store::now_utc_rfc3339()])?;
    audit(
        &tx,
        "faces.reviewed",
        Some(&r.face_id),
        &serde_json::json!({"decision":r.decision,"entity_id":r.entity_id,"group_id":r.group_id,"rationale":r.rationale}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn regroup(
    app: &AppHandle,
    job: &str,
    generation: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    let data=app.state::<Session>().with_generation(generation,|open|{let mut s=open.conn.prepare("SELECT id,vector FROM face_detection WHERE stale=0 AND vector IS NOT NULL AND model_identity=?1 ORDER BY id LIMIT 10001")?;let data=s.query_map([identity()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<Result<Vec<_>,_>>()?;Ok(data)})?;
    if data.len() > 10_000 {
        return Err(err("grouping is bounded to 10,000 faces"));
    }
    app.state::<crate::operations::Operations>().update(
        job,
        "Comparing suggested face groups",
        true,
    );
    let vectors = data
        .iter()
        .map(|(_, b)| kokin_faces::decode(b).map_err(err))
        .collect::<Result<Vec<_>, _>>()?;
    let groups = kokin_faces::groups_controlled(&vectors, kokin_faces::GROUP_THRESHOLD, || {
        cancel.load(Ordering::SeqCst)
    })
    .map_err(err)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(err("grouping cancelled"));
    }
    app.state::<crate::operations::Operations>()
        .begin_publish(job)?;
    app.state::<Session>().with_generation(generation, |open| {
        let tx = open.conn.transaction()?;
        for group in &groups {
            let member_ids: Vec<_> = group.iter().map(|i| data[*i].0.as_str()).collect();
            let gid = kokin_store::blake3_hex(
                format!("{}:{}", identity()?, member_ids.join(":")).as_bytes(),
            );
            tx.execute(
                "INSERT OR IGNORE INTO face_group VALUES(?1,?2,?3)",
                params![gid, identity()?, kokin_store::now_utc_rfc3339()],
            )?;
            for i in group {
                let current: bool = tx.query_row(
                    "SELECT stale=0 AND vector IS NOT NULL FROM face_detection WHERE id=?1",
                    [&data[*i].0],
                    |r| r.get(0),
                )?;
                if !current {
                    return Err(err("face index changed; retry grouping"));
                }
                tx.execute(
                    "INSERT OR IGNORE INTO face_group_member VALUES(?1,?2)",
                    params![gid, data[*i].0],
                )?;
            }
        }
        audit(
            &tx,
            "faces.grouped",
            None,
            &serde_json::json!({"groups":groups.len(),"threshold":kokin_faces::GROUP_THRESHOLD}),
        )?;
        tx.commit()?;
        Ok(serde_json::json!({"groups":groups.len()}))
    })
}
#[tauri::command]
pub fn face_groups(
    app: AppHandle,
    generation: u64,
    offset: usize,
) -> Result<Vec<FaceGroupView>, CommandError> {
    if offset > 10_000 {
        return Err(err("group offset bound"));
    }
    app.state::<Session>().with_generation(generation,|open|{
 let mut s=open.conn.prepare("SELECT g.id FROM face_group g WHERE g.model_identity=?1 AND NOT EXISTS(SELECT 1 FROM face_group_member m JOIN face_detection f ON f.id=m.face_id WHERE m.group_id=g.id AND f.stale=1) ORDER BY g.id LIMIT 20 OFFSET ?2")?;
 let ids=s.query_map(params![identity()?,offset as i64],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;let mut groups=Vec::new();for gid in ids{
 let page=group_members(&open.conn,&gid,0)?;
 groups.push(FaceGroupView{id:gid,faces:page.faces,member_total:page.total,member_offset:page.offset});}Ok(groups)
})
}

/// Render a selected source page/frame without writing decrypted media to disk.
#[derive(Default)]
pub struct FacePreviews {
    current: std::sync::Mutex<Option<Arc<AtomicBool>>>,
    serial: std::sync::Mutex<()>,
}
#[tauri::command]
pub async fn face_preview(
    app: AppHandle,
    generation: u64,
    face_id: String,
) -> Result<FacePreviewView, CommandError> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<FacePreviews>();
        let mut current = state.current.lock().map_err(|_| err("preview registry"))?;
        if let Some(prior) = current.replace(cancel.clone()) {
            prior.store(true, Ordering::SeqCst);
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let previews = app.state::<FacePreviews>();
        let _serial = previews
            .serial
            .lock()
            .map_err(|_| err("preview registry"))?;
        if cancel.load(Ordering::SeqCst) {
            return Err(err("source preview superseded by a newer selection"));
        }
        let (face, media, bytes) = app.state::<Session>().with_generation(generation, |open| {
            let face = face(&open.conn, &face_id, false)?;
            if face.stale {
                return Err(err("source detection is stale"));
            }
            let (media, hash): (String, String) = open.conn.query_row(
                "SELECT media_type,content_hash FROM artifact WHERE id=?1",
                [&face.artifact_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let kokin_store::BlobAccess::Readable(reference) =
                kokin_store::blob_access(&open.conn, &hash)?
            else {
                return Err(err("source was shredded"));
            };
            let bound = if media.starts_with("video/") {
                kokin_faces::MAX_VIDEO
            } else {
                kokin_media::MAX_INPUT
            };
            if reference.size > bound as u64 {
                return Err(err("source size exceeds preview bound"));
            }
            let bytes = kokin_blob::BlobStore::new(open.paths.blobs())
                .get(&reference, &open.cmk)
                .map_err(err)?;
            Ok((face, media, bytes))
        })?;
        let finished = Arc::new(AtomicBool::new(false));
        let watcher_app = app.clone();
        let stop = cancel.clone();
        let done = finished.clone();
        let watcher = std::thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                if watcher_app.state::<Session>().generation() != generation {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
        let result = (|| {
            let (binary, native) = native_paths(&app)?;
            let models = native.clone();
            let start = face.timestamp_ms.unwrap_or(0);
            let request = kokin_faces::Request {
                version: 1,
                media_type: media,
                page: face.page,
                start_ms: start,
                end_ms: face.timestamp_ms.map(|t| t + 1),
                preview: true,
            };
            let result = kokin_media::face_worker::run(
                &binary,
                &native,
                &models,
                bytes,
                &request,
                cancel.clone(),
            )
            .map_err(err)?;
            if result.preview_png.is_empty() || result.preview_png.len() > 8 * 1024 * 1024 {
                return Err(err("source preview unavailable or oversized"));
            }
            app.state::<Session>().with_generation(generation, |_| {
                Ok(FacePreviewView {
                    preview_png: base64::engine::general_purpose::STANDARD
                        .encode(result.preview_png),
                    face,
                })
            })
        })();
        finished.store(true, Ordering::SeqCst);
        let _ = watcher.join();
        result
    })
    .await
    .map_err(|_| err("source preview worker failed"))?
}

/// Creating a labelled person and recording the face review share one transaction.
#[tauri::command]
pub fn face_person_create(
    app: AppHandle,
    generation: u64,
    request: FacePersonRequest,
) -> Result<String, CommandError> {
    app.state::<Session>().with_generation(generation,|open|{
  if request.name.trim().is_empty()||request.name.len()>256||request.rationale.trim().is_empty()||request.rationale.len()>8192{return Err(err("name and review rationale are required"));}
  let source=face(&open.conn,&request.face_id,false)?;if source.stale{return Err(err("face is stale; analyze again"));}
  kokin_graph::initialise(&open.conn)?;let person=id()?;let now=kokin_store::now_utc_rfc3339();let tx=open.conn.transaction()?;
  tx.execute("INSERT INTO evidence_link VALUES(?1,'entity',?2,'artifact',?3,'context',?4)",params![id()?,person,source.artifact_id,now])?;
  tx.execute("INSERT INTO entity VALUES(?1,'person',?2,?3,?4,?4)",params![person,request.name.trim(),request.rationale,now])?;
  tx.execute("INSERT INTO face_review(face_id,entity_id,group_id,decision,rationale,actor,created_utc) VALUES(?1,?2,NULL,'accepted',?3,'user:local',?4)",params![request.face_id,person,request.rationale,now])?;
  kokin_store::append_audit_event(&tx,&kokin_store::NewAuditEvent{actor:"user:local",action:"entity.created",subject_kind:Some("entity"),subject_id:Some(&person),payload_json:&serde_json::json!({"source":"reviewed_face","face_id":request.face_id}).to_string()})?;
  audit(&tx,"faces.reviewed",Some(&request.face_id),&serde_json::json!({"entity_id":person,"decision":"accepted","rationale":request.rationale}))?;tx.commit()?;Ok(person)
 })
}

#[tauri::command]
pub fn person_faces(
    app: AppHandle,
    generation: u64,
    entity_id: String,
    offset: usize,
) -> Result<FacePageView, CommandError> {
    if offset > 100_000 {
        return Err(err("face offset limit"));
    }
    app.state::<Session>().with_generation(generation,|open|{
  let canonical=kokin_graph::resolution::canonical_id(&open.conn,&entity_id)?;let mut members=kokin_graph::resolution::cluster(&open.conn,&canonical)?;members.push(canonical);
  let mut ids=Vec::new();for member in members{let mut s=open.conn.prepare("SELECT r.face_id FROM face_review r JOIN face_detection f ON f.id=r.face_id WHERE r.entity_id=?1 AND r.decision='accepted' AND r.group_id IS NULL AND f.stale=0 AND r.id=(SELECT max(id) FROM face_review WHERE face_id=r.face_id AND group_id IS NULL) ORDER BY r.face_id")?;ids.extend(s.query_map([member],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?);}
  ids.sort();ids.dedup();let total=ids.len() as i64;let faces=ids.into_iter().skip(offset).take(50).map(|id|face(&open.conn,&id,true)).collect::<Result<Vec<_>,_>>()?;
  Ok(FacePageView{faces,total,offset,limitations:vec!["Analyst-reviewed person links. Stale detections are omitted; historical reviews remain in the case.".into()]})
 })
}

fn group_members(
    conn: &rusqlite::Connection,
    group_id: &str,
    offset: usize,
) -> Result<FacePageView, CommandError> {
    if offset > 10_000 {
        return Err(err("group member offset bound"));
    }
    let filter="FROM face_group_member m JOIN face_detection f ON f.id=m.face_id WHERE m.group_id=?1 AND f.stale=0 AND NOT EXISTS(SELECT 1 FROM face_review r WHERE r.group_id=m.group_id AND r.face_id=m.face_id AND r.decision='excluded')";
    let total = conn.query_row(&format!("SELECT count(*) {filter}"), [group_id], |r| {
        r.get(0)
    })?;
    let mut s = conn.prepare(&format!(
        "SELECT m.face_id {filter} ORDER BY m.face_id LIMIT 12 OFFSET ?2"
    ))?;
    let ids = s
        .query_map(params![group_id, offset as i64], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FacePageView {
        faces: ids
            .iter()
            .map(|id| face(conn, id, true))
            .collect::<Result<Vec<_>, _>>()?,
        total,
        offset,
        limitations: vec!["Suggested group; membership does not establish identity.".into()],
    })
}
#[tauri::command]
pub fn face_group_members(
    app: AppHandle,
    generation: u64,
    group_id: String,
    offset: usize,
) -> Result<FaceGroupMembersView, CommandError> {
    app.state::<Session>().with_generation(generation, |open| {
        Ok(FaceGroupMembersView {
            page: group_members(&open.conn, &group_id, offset)?,
            group_id,
        })
    })
}

#[tauri::command]
pub fn face_read(
    app: AppHandle,
    generation: u64,
    face_id: String,
) -> Result<FaceView, CommandError> {
    app.state::<Session>()
        .with_generation(generation, |open| face(&open.conn, &face_id, true))
}
