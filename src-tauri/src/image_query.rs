//! Local prepared images and bounded duplicate-candidate search. No network/model.
use crate::views::{ImagePrepareRequest, ImageSearchRequest};
use crate::{CommandError, ErrorCode, Session};
use kokin_media::image_query::Rect;
use kokin_store::OpenCase;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::Manager;
fn invalid(v: impl ToString) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, v.to_string())
}
fn readable(open: &OpenCase, id: &str) -> Result<kokin_blob::BlobRef, CommandError> {
    let hash: String =
        open.conn
            .query_row("SELECT content_hash FROM artifact WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
    match kokin_store::blob_access(&open.conn, &hash)? {
        kokin_store::BlobAccess::Readable(r) => Ok(r),
        _ => Err(invalid("source image is shredded or unavailable")),
    }
}
fn hash(outcome: &kokin_blob::PutOutcome) -> String {
    match outcome {
        kokin_blob::PutOutcome::Stored(r) => r.content_hash.clone(),
        kokin_blob::PutOutcome::AlreadyPresent { content_hash, .. } => content_hash.clone(),
    }
}
fn face_rect(locator: &Value) -> Result<Rect, CommandError> {
    let size = locator["image_size"]
        .as_array()
        .ok_or_else(|| invalid("face image dimensions unavailable"))?;
    let bounds = locator["bbox"]
        .as_array()
        .ok_or_else(|| invalid("face bounds unavailable"))?;
    if size.len() != 2 || bounds.len() != 4 {
        return Err(invalid("face geometry invalid"));
    }
    let w = size[0].as_f64().unwrap_or(0.);
    let h = size[1].as_f64().unwrap_or(0.);
    let b = bounds
        .iter()
        .map(|v| v.as_f64().unwrap_or(f64::NAN))
        .collect::<Vec<_>>();
    if w <= 0. || h <= 0. || !b.iter().all(|v| v.is_finite()) || b[2] <= 0. || b[3] <= 0. {
        return Err(invalid("face geometry invalid"));
    }
    let left = (b[0] / w * 1000.).floor().clamp(0., 999.) as u16;
    let top = (b[1] / h * 1000.).floor().clamp(0., 999.) as u16;
    let right = ((b[0] + b[2]) / w * 1000.)
        .ceil()
        .clamp(f64::from(left + 1), 1000.) as u16;
    let bottom = ((b[1] + b[3]) / h * 1000.)
        .ceil()
        .clamp(f64::from(top + 1), 1000.) as u16;
    Ok(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}
pub fn prepare_request(
    open: &OpenCase,
    input: &ImagePrepareRequest,
) -> Result<(ImagePrepareRequest, Vec<String>), CommandError> {
    if input.version != 1 || input.artifact_id.len() > 128 {
        return Err(invalid("unsupported image preparation request"));
    }
    input.recipe.validate().map_err(invalid)?;
    readable(open, &input.artifact_id)?;
    let mut request = input.clone();
    let mut selected_found = false;
    let mut ids = vec![];
    if request.obscure_other_faces && request.selected_face_id.is_none() {
        return Err(invalid(
            "select a current detected face before obscuring others",
        ));
    }
    if request.selected_face_id.is_some() {
        let mut query=open.conn.prepare("SELECT id,locator_json FROM face_detection WHERE artifact_id=?1 AND stale=0 AND CAST(json_extract(locator_json,'$.page') AS INTEGER)=?2 AND json_extract(locator_json,'$.timestamp_ms') IS NULL ORDER BY rowid LIMIT 66")?;
        for row in query.query_map(rusqlite::params![request.artifact_id, request.page], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, raw) = row?;
            let locator: Value = serde_json::from_str(&raw).map_err(invalid)?;
            if Some(&id) == request.selected_face_id.as_ref() {
                selected_found = true;
                ids.push(id);
            } else if request.obscure_other_faces {
                request.recipe.obscure.push(face_rect(&locator)?);
                ids.push(id);
            }
        }
        if !selected_found {
            return Err(invalid(
                "selected face is missing, stale, outside this page or unavailable",
            ));
        }
    }
    request.recipe.validate().map_err(invalid)?;
    Ok((request, ids))
}
pub fn run(
    app: &tauri::AppHandle,
    job: &str,
    generation: u64,
    input: ImagePrepareRequest,
    cancel: &Arc<AtomicBool>,
) -> Result<Value, CommandError> {
    let session = app.state::<Session>();
    let (request, face_ids, reference, key, root) =
        session.with_generation(generation, |open| {
            let (request, ids) = prepare_request(open, &input)?;
            let reference = readable(open, &input.artifact_id)?;
            if reference.size > kokin_media::MAX_INPUT as u64 {
                return Err(invalid("image source exceeds 64 MiB"));
            }
            Ok((
                request,
                ids,
                reference,
                open.cmk.clone(),
                open.paths.blobs(),
            ))
        })?;
    let bytes = kokin_blob::BlobStore::new(root)
        .get(&reference, &key)
        .map_err(invalid)?;
    let media = session.with_generation(generation, |o| {
        Ok(o.conn.query_row(
            "SELECT media_type FROM artifact WHERE id=?1",
            [&request.artifact_id],
            |r| r.get::<_, String>(0),
        )?)
    })?;
    if !crate::media::supported(&media) {
        return Err(invalid(
            "this source format has no reviewed native query decoder",
        ));
    }
    let resources = app.path().resource_dir().map_err(invalid)?;
    let binary = std::env::current_exe()
        .map_err(invalid)?
        .with_file_name("kokin-media-worker");
    let native = resources.join("native");
    #[cfg(debug_assertions)]
    let (binary, native) = if binary.exists() && native.exists() {
        (binary, native)
    } else {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/media-probe");
        (
            root.join("MacOS/kokin-media-worker"),
            root.join("Resources/native"),
        )
    };
    let worker = kokin_media::Request {
        version: 1,
        media_type: media,
        page: request.page,
        ocr: false,
        image_recipe: Some(request.recipe.clone()),
    };
    let output = kokin_media::worker::run(&binary, &native, None, bytes, &worker, cancel.clone())
        .map_err(invalid)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(invalid("image preparation cancelled before publication"));
    }
    app.state::<crate::operations::Operations>()
        .begin_publish(job)?;
    session.with_generation(generation,|o|{
  let now=readable(o,&request.artifact_id)?;if now.content_hash!=reference.content_hash{return Err(invalid("source changed before publication"));}
  for id in &face_ids{let current:bool=o.conn.query_row("SELECT EXISTS(SELECT 1 FROM face_detection WHERE id=?1 AND artifact_id=?2 AND stale=0)",rusqlite::params![id,request.artifact_id],|r|r.get(0))?;if !current{return Err(invalid("face analysis changed; review the query again"));}}
  commit(o,&request,&face_ids,output)
 })
}
pub fn commit(
    open: &mut OpenCase,
    request: &ImagePrepareRequest,
    face_ids: &[String],
    mut output: kokin_media::Result,
) -> Result<Value, CommandError> {
    readable(open, &request.artifact_id)?;
    request.recipe.validate().map_err(invalid)?;
    if !output.engine.contains("query-prepare-1")
        || !output.thumbnail_png.starts_with(b"\x89PNG\r\n\x1a\n")
        || output.thumbnail_png.len() > kokin_media::MAX_OUTPUT
    {
        return Err(invalid("worker did not return a bounded prepared PNG"));
    }
    let recorded = output
        .observations
        .iter()
        .find(|o| o.kind == "image.query_recipe")
        .and_then(|o| serde_json::from_str::<Value>(&o.value).ok());
    let recipe = serde_json::to_value(&request.recipe).map_err(invalid)?;
    if recorded.as_ref() != Some(&recipe) {
        return Err(invalid(
            "worker transformation recipe differs from the requested recipe",
        ));
    }
    // A prepared PNG is one page even when its source was a multi-page PDF.
    // Source-page coordinates belong to preparation lineage, not the new PNG.
    output.pages = 1;
    for observation in &mut output.observations {
        observation.locator["source_page"] = request.page.into();
        observation.locator["page"] = 0.into();
    }
    let store = kokin_blob::BlobStore::new(open.paths.blobs());
    let put = crate::media::put_derivative(open, &store, &output.thumbnail_png)?;
    let content = hash(&put);
    let now = kokin_store::now_utc_rfc3339();
    let run = crate::operations::random_id()?;
    let artifact = crate::operations::random_id()?;
    let tx = open.conn.transaction()?;
    if let kokin_blob::PutOutcome::Stored(reference) = put {
        tx.execute(
            "INSERT INTO blob VALUES(?1,?2,?3,?4,?5,NULL)",
            rusqlite::params![
                reference.content_hash,
                reference.size as i64,
                reference.wrapped_key.ciphertext,
                reference.wrapped_key.nonce.as_slice(),
                now
            ],
        )?;
    }
    tx.execute("INSERT INTO transform_run VALUES(?1,'image.query.prepare','1',?2,?3,?4,?4,'succeeded',NULL,NULL)",rusqlite::params![run,env!("CARGO_PKG_VERSION"),kokin_store::blake3_hex(recipe.to_string().as_bytes()),now])?;
    tx.execute(
        "INSERT INTO artifact VALUES(?1,?2,'image/png',?3,?4)",
        rusqlite::params![artifact, content, output.thumbnail_png.len() as i64, now],
    )?;
    tx.execute(
        "INSERT INTO run_subject VALUES(?1,'artifact',?2)",
        rusqlite::params![run, request.artifact_id],
    )?;
    tx.execute(
        "INSERT INTO derivation VALUES(?1,?2,'artifact',?3,'artifact',?4)",
        rusqlite::params![
            crate::operations::random_id()?,
            run,
            request.artifact_id,
            artifact
        ],
    )?;
    let provenance = json!({"version":1,"recipe":recipe,"selected_face_id":request.selected_face_id,"obscured_face_ids":face_ids.iter().filter(|id|Some(*id)!=request.selected_face_id.as_ref()).collect::<Vec<_>>(),"coordinates":"orientation_corrected_source_thousandths","order":"orientation,opaque_obscure,crop,rotate,mirror,brightness,contrast,RGB_offsets"});
    tx.execute(
        "INSERT INTO image_query VALUES(?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            artifact,
            request.artifact_id,
            run,
            request.page,
            provenance.to_string(),
            output.engine,
            now
        ],
    )?;
    kokin_store::audit::append(
        &tx,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action: "image.query_prepared",
            subject_kind: Some("artifact"),
            subject_id: Some(&artifact),
            payload_json: &json!({"source_artifact_id":request.artifact_id,"run_id":run})
                .to_string(),
        },
    )?;
    tx.commit()?;
    // Keep the same authenticated preview/snapshot representation used by the
    // existing media inspector; it remains a separate versioned L1 transform.
    crate::media::commit(open, &artifact, 0, false, output)?;
    Ok(
        json!({"artifact_id":artifact,"source_artifact_id":request.artifact_id,"run_id":run,"derived":true,"uploaded":false,"recipe":provenance}),
    )
}
#[tauri::command]
pub fn image_query_info(
    session: tauri::State<'_, Session>,
    generation: u64,
    artifact_id: String,
) -> Result<Value, CommandError> {
    session.with_generation(generation,|o|{
 readable(o,&artifact_id)?;
 let mut s=o.conn.prepare("SELECT source_artifact_id,page,recipe_json,engine,created_utc FROM image_query WHERE artifact_id=?1")?;
 let value=s.query_row([&artifact_id],|r|Ok(json!({"artifact_id":artifact_id,"source_artifact_id":r.get::<_,String>(0)?,"page":r.get::<_,u16>(1)?,"recipe":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(Value::Null),"engine":r.get::<_,String>(3)?,"created_utc":r.get::<_,String>(4)?}))).optional()?;
 Ok(value.unwrap_or(Value::Null))
})
}
use rusqlite::OptionalExtension;

pub struct SearchInput {
    request: ImageSearchRequest,
    source: kokin_blob::BlobRef,
    snapshot: Option<kokin_blob::BlobRef>,
    key: kokin_keys::CaseMasterKey,
    root: std::path::PathBuf,
    rows: Vec<Value>,
    truncated: bool,
    eligible: i64,
}
pub fn prepare_search(
    open: &OpenCase,
    request: ImageSearchRequest,
) -> Result<SearchInput, CommandError> {
    if request.version != 1
        || !matches!(request.mode.as_str(), "duplicates" | "exact")
        || request.max_distance > 16
        || ![500, 5000, 20000].contains(&request.scan_limit)
        || request.offset > 20000
        || !matches!(
            request.sort.as_str(),
            "best" | "worst" | "newest" | "oldest" | "diverse"
        )
        || request.domain.len() > 253
        || request.title.len() > 200
        || request
            .min_bytes
            .zip(request.max_bytes)
            .is_some_and(|(a, b)| a > b)
    {
        return Err(invalid("unsupported local image filter or budget"));
    }
    for date in [&request.date_from, &request.date_to] {
        if !date.is_empty() {
            let valid: bool = open.conn.query_row(
                "SELECT length(?1)=10 AND date(?1,'+0 days')=?1",
                [date],
                |r| r.get(0),
            )?;
            if !valid {
                return Err(invalid("use a valid YYYY-MM-DD index date"));
            }
        }
    }
    if !request.date_from.is_empty()
        && !request.date_to.is_empty()
        && request.date_from > request.date_to
    {
        return Err(invalid("index date range is reversed"));
    }
    if !request.domain.is_empty() {
        kokin_connectors::research::Selector::parse(
            kokin_connectors::research::SelectorKind::Domain,
            &request.domain,
        )
        .map_err(invalid)?;
    }
    let media: String = open.conn.query_row(
        "SELECT media_type FROM artifact WHERE id=?1",
        [&request.artifact_id],
        |r| r.get(0),
    )?;
    if !(media.starts_with("image/") || media == "application/pdf") {
        return Err(invalid(
            "select captured image or PDF evidence for local image search",
        ));
    }
    let source = readable(open, &request.artifact_id)?;
    if source.size > kokin_media::MAX_INPUT as u64 {
        return Err(invalid("query source exceeds 64 MiB"));
    }
    let snapshot = if request.mode == "duplicates" {
        let hash:String=open.conn.query_row("SELECT snapshot_hash FROM media_derivative WHERE artifact_id=?1 AND page=?2 ORDER BY rowid DESC LIMIT 1",rusqlite::params![request.artifact_id,request.page],|r|r.get(0)).map_err(|_|invalid("process this image/page locally before perceptual duplicate search"))?;
        match kokin_store::blob_access(&open.conn, &hash)? {
            kokin_store::BlobAccess::Readable(r) if r.size <= kokin_media::MAX_OUTPUT as u64 => {
                Some(r)
            }
            _ => return Err(invalid("query fingerprint snapshot unavailable")),
        }
    } else {
        None
    };
    let eligible:i64=open.conn.query_row("SELECT COUNT(*) FROM artifact a JOIN blob b ON b.content_hash=a.content_hash WHERE (a.media_type LIKE 'image/%' OR a.media_type='application/pdf') AND b.wrapped_key IS NOT NULL",[],|r|r.get(0))?;
    let sql = if request.mode == "exact" {
        "SELECT a.id,a.content_hash,a.media_type,a.byte_length,a.created_utc,0,NULL,a.created_utc,(SELECT s.canonical_locator FROM capture c JOIN source s ON s.id=c.source_id WHERE c.content_hash=a.content_hash ORDER BY c.rowid DESC LIMIT 1),(SELECT value FROM observation o WHERE o.artifact_id=a.id AND o.kind='page.title' AND NOT EXISTS(SELECT 1 FROM observation_supersession s WHERE s.superseded_id=o.id) LIMIT 1) FROM artifact a JOIN blob b ON b.content_hash=a.content_hash WHERE a.id<>?1 AND (a.media_type LIKE 'image/%' OR a.media_type='application/pdf') AND b.wrapped_key IS NOT NULL ORDER BY a.rowid DESC LIMIT ?2"
    } else {
        "SELECT a.id,a.content_hash,a.media_type,a.byte_length,a.created_utc,m.page,m.perceptual_hash,m.created_utc,(SELECT s.canonical_locator FROM capture c JOIN source s ON s.id=c.source_id WHERE c.content_hash=a.content_hash ORDER BY c.rowid DESC LIMIT 1),(SELECT value FROM observation o WHERE o.artifact_id=a.id AND o.kind='page.title' AND NOT EXISTS(SELECT 1 FROM observation_supersession s WHERE s.superseded_id=o.id) LIMIT 1) FROM media_derivative m JOIN artifact a ON a.id=m.artifact_id JOIN blob b ON b.content_hash=a.content_hash JOIN blob mb ON mb.content_hash=m.snapshot_hash WHERE a.id<>?1 AND b.wrapped_key IS NOT NULL AND mb.wrapped_key IS NOT NULL AND m.perceptual_hash IS NOT NULL AND NOT EXISTS(SELECT 1 FROM media_derivative newer WHERE newer.artifact_id=m.artifact_id AND newer.page=m.page AND newer.rowid>m.rowid) ORDER BY m.rowid DESC LIMIT ?2"
    };
    let mut query = open.conn.prepare(sql)?;
    let mut rows = vec![];
    let mut bytes = 0;
    let mut truncated = false;
    for row in query.query_map(rusqlite::params![request.artifact_id,(request.scan_limit+1) as i64],|r|Ok(json!({"artifact_id":r.get::<_,String>(0)?,"content_hash":r.get::<_,String>(1)?,"media_type":r.get::<_,String>(2)?,"byte_length":r.get::<_,i64>(3)?,"artifact_created_utc":r.get::<_,String>(4)?,"page":r.get::<_,u16>(5)?,"perceptual_hash":r.get::<_,Option<String>>(6)?,"indexed_utc":r.get::<_,String>(7)?,"source_url":r.get::<_,Option<String>>(8)?,"title":r.get::<_,Option<String>>(9)?,"publication_utc":null,"language":null})))? {
  let row=row?;bytes+=row.to_string().len();if rows.len()==request.scan_limit||bytes>32*1024*1024{truncated=true;break;}rows.push(row);
 }
    Ok(SearchInput {
        request,
        source,
        snapshot,
        key: open.cmk.clone(),
        root: open.paths.blobs(),
        rows,
        truncated,
        eligible,
    })
}
pub fn process_search(input: SearchInput) -> Result<Value, CommandError> {
    let store = kokin_blob::BlobStore::new(&input.root);
    store
        .read_to(&input.source, &input.key, &mut std::io::sink())
        .map_err(invalid)?;
    let query_hash = if let Some(snapshot) = &input.snapshot {
        let bytes = store.get(snapshot, &input.key).map_err(invalid)?;
        let snapshot: kokin_media::Result = serde_json::from_slice(&bytes).map_err(invalid)?;
        Some(
            u64::from_str_radix(
                snapshot
                    .perceptual_hash
                    .as_deref()
                    .ok_or_else(|| invalid("query has no perceptual fingerprint"))?,
                16,
            )
            .map_err(invalid)?,
        )
    } else {
        None
    };
    let request = &input.request;
    let scanned = input.rows.len();
    let mut matches = vec![];
    let mut groups = BTreeMap::<String, usize>::new();
    let domain_filter = if request.domain.is_empty() {
        String::new()
    } else {
        kokin_connectors::research::Selector::parse(
            kokin_connectors::research::SelectorKind::Domain,
            &request.domain,
        )
        .map_err(invalid)?
        .normalized
    };
    for mut row in input.rows {
        let exact = row["content_hash"].as_str() == Some(&input.source.content_hash);
        let distance = if exact {
            Some(0)
        } else {
            query_hash
                .zip(
                    row["perceptual_hash"]
                        .as_str()
                        .and_then(|v| u64::from_str_radix(v, 16).ok()),
                )
                .map(|(a, b)| (a ^ b).count_ones())
        };
        if !exact && (request.mode == "exact" || distance.is_none_or(|d| d > request.max_distance))
        {
            continue;
        }
        let domain = row["source_url"]
            .as_str()
            .and_then(|v| tauri::Url::parse(v).ok())
            .filter(|v| matches!(v.scheme(), "https" | "http"))
            .and_then(|v| v.host_str().map(str::to_owned));
        if !domain_filter.is_empty() && domain.as_deref() != Some(&domain_filter) {
            continue;
        }
        let date = row["indexed_utc"].as_str().unwrap_or("");
        let day = date.get(..10).unwrap_or("");
        if !request.date_from.is_empty() && (day.is_empty() || day < request.date_from.as_str())
            || !request.date_to.is_empty() && (day.is_empty() || day > request.date_to.as_str())
        {
            continue;
        }
        if !request.title.is_empty()
            && !row["title"]
                .as_str()
                .is_some_and(|v| v.to_lowercase().contains(&request.title.to_lowercase()))
        {
            continue;
        }
        let size = row["byte_length"].as_u64().unwrap_or(0);
        if request.min_bytes.is_some_and(|s| size < s)
            || request.max_bytes.is_some_and(|s| size > s)
        {
            continue;
        }
        let group = domain
            .clone()
            .unwrap_or_else(|| "local or unattributed".into());
        *groups.entry(group.clone()).or_default() += 1;
        row["domain"] = domain.into();
        row["group"] = group.into();
        row["exact_bytes"] = exact.into();
        row["distance"] = distance.into();
        row["perceptual_hash"] = Value::Null;
        row["signal"] = if exact {
            "Identical captured file bytes"
        } else {
            "Stored perceptual-hash candidate; inspect the source"
        }
        .into();
        matches.push(row);
    }
    matches.sort_by(|a, b| {
        let order = match request.sort.as_str() {
            "newest" => b["indexed_utc"].as_str().cmp(&a["indexed_utc"].as_str()),
            "oldest" => a["indexed_utc"].as_str().cmp(&b["indexed_utc"].as_str()),
            "worst" => b["distance"].as_u64().cmp(&a["distance"].as_u64()),
            _ => a["distance"].as_u64().cmp(&b["distance"].as_u64()),
        };
        order
            .then(a["artifact_id"].as_str().cmp(&b["artifact_id"].as_str()))
            .then(a["page"].as_u64().cmp(&b["page"].as_u64()))
    });
    if request.sort == "diverse" {
        let mut by_group = BTreeMap::<String, std::collections::VecDeque<Value>>::new();
        for row in matches {
            by_group
                .entry(row["group"].as_str().unwrap_or("").into())
                .or_default()
                .push_back(row);
        }
        matches = vec![];
        loop {
            let mut found = false;
            for queue in by_group.values_mut() {
                if let Some(row) = queue.pop_front() {
                    found = true;
                    matches.push(row);
                }
            }
            if !found {
                break;
            }
        }
    }
    let total = matches.len();
    let rows = matches
        .into_iter()
        .skip(request.offset)
        .take(50)
        .collect::<Vec<_>>();
    Ok(
        json!({"version":1,"scope":"current encrypted case only","delivery":"native","external_requests":0,"rows":rows,"total":total,"offset":request.offset,"limit":50,"scanned":scanned,"scan_limit":request.scan_limit,"scan_truncated":input.truncated,"eligible_artifacts":input.eligible,"groups":groups,"coverage":"Counts cover only the bounded scanned corpus. Unprocessed pages/images are absent from perceptual search. Candidate fingerprints are stored L1 signals; inspect each source for current integrity. Index dates are not publication dates. Similarity is not identity, confidence or geographic certainty."}),
    )
}
#[tauri::command]
pub async fn image_query_search(
    app: tauri::AppHandle,
    generation: u64,
    request: ImageSearchRequest,
) -> Result<Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let input = app
            .state::<Session>()
            .with_generation(generation, |o| prepare_search(o, request.clone()))?;
        let result = process_search(input)?;
        app.state::<Session>().with_generation(generation, |o| {
            readable(o, &request.artifact_id)?;
            for row in result["rows"].as_array().into_iter().flatten() {
                readable(
                    o,
                    row["artifact_id"]
                        .as_str()
                        .ok_or_else(|| invalid("invalid result reference"))?,
                )?;
            }
            Ok(result)
        })
    })
    .await
    .map_err(|_| invalid("local image search worker failed"))?
}

pub fn import_clipboard(
    open: &mut OpenCase,
    job: &str,
    input: crate::views::ClipboardImageRequest,
) -> Result<Value, CommandError> {
    use base64::Engine;
    if input.version != 1 || input.base64.len() > 14 * 1024 * 1024 {
        return Err(invalid("clipboard image exceeds 10 MiB"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(input.base64)
        .map_err(|_| invalid("clipboard image encoding invalid"))?;
    let signature = match input.media_type.as_str() {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        _ => false,
    };
    if !signature || bytes.len() > 10 * 1024 * 1024 {
        return Err(invalid(
            "clipboard bytes do not match a reviewed PNG/JPEG/WEBP image",
        ));
    }
    let captured = kokin_ingest::ingest_clipboard_image(
        &mut open.conn,
        &kokin_blob::BlobStore::new(open.paths.blobs()),
        &open.cmk,
        &bytes,
        &input.media_type,
        &kokin_ingest::IngestContext {
            connector: "clipboard_image",
            job_id: job,
        },
    )?;
    Ok(
        json!({"artifact_id":captured.artifact_id,"origin":"clipboard image bytes only","uploaded":false,"byte_length":captured.size_bytes}),
    )
}
