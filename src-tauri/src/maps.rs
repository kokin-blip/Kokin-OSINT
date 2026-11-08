//! Local archive capabilities and case-encrypted, source-grounded locations.
use crate::{views::*, CommandError, ErrorCode, Session};
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};
use tauri::{AppHandle, Manager};
struct Selection {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
    expires: Instant,
}
pub struct MapResources {
    selections: Mutex<HashMap<String, Selection>>,
    worlds: Mutex<HashMap<String, WorldSelection>>,
}
impl Default for MapResources {
    fn default() -> Self {
        Self {
            selections: Mutex::new(HashMap::new()),
            worlds: Mutex::new(HashMap::new()),
        }
    }
}
fn err(v: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, v)
}
fn root(app: &AppHandle) -> Result<PathBuf, CommandError> {
    Ok(crate::resources::root(app)?.join("maps"))
}
fn path(app: &AppHandle, id: &str) -> Result<PathBuf, CommandError> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(err("unknown archive capability"));
    }
    Ok(root(app)?.join(format!("{id}.pmtiles")))
}
fn info(app: &AppHandle, id: &str) -> Result<MapArchiveView, CommandError> {
    let file = path(app, id)?;
    let h = kokin_maps::validate(&file).map_err(|e| err(e.to_string()))?;
    let mut f = std::fs::File::open(&file).map_err(|_| err("archive unavailable"))?;
    let metadata = if h.metadata.1 > 0 {
        let bytes = kokin_maps::range(&mut f, h.bytes, h.metadata.0, h.metadata.1)
            .map_err(|e| err(e.to_string()))?;
        let bytes = kokin_maps::decompress(&bytes, h.internal_compression)
            .map_err(|e| err(e.to_string()))?;
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    let credit = std::fs::read_to_string(file.with_extension("credits.txt")).unwrap_or_else(|_| {
        "Imported archive: credits unavailable; verify the resource before illustration export."
            .into()
    });
    let layers = metadata["vector_layers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["id"].as_str())
        .filter(|id| {
            id.len() <= 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .take(64)
        .map(str::to_owned)
        .collect();
    Ok(MapArchiveView {
        id: id.into(),
        bytes: h.bytes,
        min_zoom: h.min_zoom,
        max_zoom: h.max_zoom,
        tile_type: h.tile_type,
        bounds: h.bounds.to_vec(),
        attribution: credit,
        layers,
    })
}
#[tauri::command]
pub async fn map_archives(app: AppHandle) -> Result<Vec<MapArchiveView>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = root(&app)?;
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for entry in std::fs::read_dir(directory)
            .map_err(|_| err("map resource folder"))?
            .take(256)
        {
            let entry = entry.map_err(|_| err("map entry"))?;
            if entry.path().extension().is_some_and(|e| e == "pmtiles") {
                if let Some(id) = entry.path().file_stem().and_then(|v| v.to_str()) {
                    if let Ok(view) = info(&app, id) {
                        rows.push(view);
                    }
                }
            }
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(rows)
    })
    .await
    .map_err(|_| err("archive catalogue failed"))?
}
#[tauri::command]
pub async fn map_select(app: AppHandle) -> Result<Option<MapSelectionView>, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(feature = "desktop-test")]
        let queued = app
            .state::<crate::operations::Operations>()
            .test_state
            .lock()
            .map_err(|_| err("test selection"))?
            .selections
            .pop_front();
        #[cfg(feature = "desktop-test")]
        let selected = if let Some((purpose, path)) = queued {
            if purpose != "map_archive" {
                return Err(err("wrong fixture selection purpose"));
            }
            Some(path)
        } else {
            rfd::FileDialog::new()
                .set_title("Choose a local PMTiles v3 basemap")
                .add_filter("PMTiles", &["pmtiles"])
                .pick_file()
        };
        #[cfg(not(feature = "desktop-test"))]
        let selected = rfd::FileDialog::new()
            .set_title("Choose a local PMTiles v3 basemap")
            .add_filter("PMTiles", &["pmtiles"])
            .pick_file();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let selected =
            std::fs::canonicalize(selected).map_err(|_| err("selected archive unavailable"))?;
        let h = kokin_maps::validate(&selected).map_err(|e| err(e.to_string()))?;
        let meta = std::fs::metadata(&selected).map_err(|_| err("archive metadata"))?;
        let token = kokin_ai::persistence::id().map_err(|_| err("selection id"))?;
        let state = app.state::<MapResources>();
        let mut pending = state
            .selections
            .lock()
            .map_err(|_| err("selection registry"))?;
        pending.retain(|_, s| s.expires > Instant::now());
        if pending.len() >= 8 {
            return Err(err("too many pending map selections"));
        }
        pending.insert(
            token.clone(),
            Selection {
                path: selected.clone(),
                size: meta.len(),
                modified: meta.modified().map_err(|_| err("archive identity"))?,
                expires: Instant::now() + Duration::from_secs(300),
            },
        );
        Ok(Some(MapSelectionView {
            selection: token,
            filename: selected
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            bytes: h.bytes,
            min_zoom: h.min_zoom,
            max_zoom: h.max_zoom,
            destination: root(&app)?.to_string_lossy().into_owned(),
        }))
    })
    .await
    .map_err(|_| err("map selection failed"))?
}
#[tauri::command]
pub async fn map_import(app: AppHandle, request: MapImportRequest) -> Result<String, CommandError> {
    if !request.consent || request.attribution.trim().is_empty() || request.attribution.len() > 4096
    {
        return Err(err(
            "review size/destination and enter the archive's required credits before import",
        ));
    }
    let (operation, cancel) = app.state::<crate::resources::ResourceJobs>().begin(
        "map-import",
        app.state::<MapResources>()
            .selections
            .lock()
            .map_err(|_| err("map selection registry"))?
            .get(&request.selection)
            .ok_or_else(|| err("selection expired"))?
            .size,
    )?;
    let output_id = operation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = (|| -> Result<MapArchiveView, CommandError> {
            let selected = app
                .state::<MapResources>()
                .selections
                .lock()
                .map_err(|_| err("selection registry"))?
                .remove(&request.selection)
                .ok_or_else(|| err("selection expired"))?;
            if selected.expires < Instant::now() {
                return Err(err("selection expired"));
            }
            let identity = |p: &Path| -> Result<(u64, SystemTime), CommandError> {
                let meta = std::fs::metadata(p).map_err(|_| err("selected archive disappeared"))?;
                Ok((
                    meta.len(),
                    meta.modified().map_err(|_| err("archive metadata"))?,
                ))
            };
            if identity(&selected.path)? != (selected.size, selected.modified) {
                return Err(err("selected archive changed; choose it again"));
            }
            let directory = root(&app)?;
            std::fs::create_dir_all(&directory)
                .map_err(|_| err("map resource folder unavailable"))?;
            let job = kokin_ai::persistence::id().map_err(|_| err("import id"))?;
            let stage = directory.join(format!(".incoming-{job}"));
            let outcome = (|| {
                let mut input = std::fs::File::open(&selected.path)
                    .map_err(|_| err("selected archive unreadable"))?;
                let mut output = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&stage)
                    .map_err(|_| err("map staging unavailable"))?;
                let mut hash = Sha256::new();
                let mut block = [0; 65536];
                let mut count = 0;
                loop {
                    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(err("map import cancelled before publication"));
                    }
                    let n = input
                        .read(&mut block)
                        .map_err(|_| err("archive read interrupted"))?;
                    if n == 0 {
                        break;
                    }
                    count += n as u64;
                    if count > selected.size {
                        return Err(err("selected archive changed"));
                    }
                    output
                        .write_all(&block[..n])
                        .map_err(|_| err("archive write failed; check storage"))?;
                    hash.update(&block[..n]);
                    app.state::<crate::resources::ResourceJobs>()
                        .progress(&operation, count);
                }
                output
                    .flush()
                    .and_then(|_| output.sync_all())
                    .map_err(|_| err("map publication flush failed"))?;
                drop(output);
                if count != selected.size
                    || identity(&selected.path)? != (selected.size, selected.modified)
                {
                    return Err(err("selected archive changed during import"));
                }
                kokin_maps::validate(&stage).map_err(|e| err(e.to_string()))?;
                let id = format!("{:x}", hash.finalize());
                let destination = path(&app, &id)?;
                app.state::<crate::resources::ResourceJobs>()
                    .publish(&operation)?;
                if !destination.exists() {
                    let credits = destination.with_extension("credits.txt");
                    let mut credit = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&credits)
                        .map_err(|_| err("credit destination exists; no archive replaced"))?;
                    let publication = credit
                        .write_all(request.attribution.as_bytes())
                        .and_then(|_| credit.sync_all())
                        .and_then(|_| std::fs::hard_link(&stage, &destination));
                    if publication.is_err() {
                        std::fs::remove_file(&credits).map_err(|_| err("archive publication and credits cleanup failed; import not complete"))?;
                        return Err(err("archive publication failed; no archive replaced"));
                    }
                }
                info(&app, &id)
            })();
            if stage.exists() {
                std::fs::remove_file(&stage)
                    .map_err(|_| err("map staging cleanup failed; import not reported complete"))?;
            }
            outcome
        })();
        app.state::<crate::resources::ResourceJobs>()
            .finish(&operation, outcome.map(|view| view.id));
    });
    Ok(output_id)
}

#[tauri::command]
pub async fn map_range(
    app: AppHandle,
    id: String,
    offset: u64,
    length: u32,
) -> Result<String, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let file = path(&app, &id)?;
        let h = kokin_maps::validate(&file).map_err(|e| err(e.to_string()))?;
        let mut f = std::fs::File::open(file).map_err(|_| err("archive missing"))?;
        let bytes = kokin_maps::range(&mut f, h.bytes, offset, u64::from(length))
            .map_err(|e| err(e.to_string()))?;
        if offset >= h.leaves.0 && offset < h.leaves.0 + h.leaves.1 {
            let decoded = kokin_maps::decompress(&bytes, h.internal_compression)
                .map_err(|e| err(e.to_string()))?;
            kokin_maps::directory(&decoded, &h).map_err(|e| err(e.to_string()))?;
        }
        use base64::Engine;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    })
    .await
    .map_err(|_| err("local map read failed"))?
}
#[tauri::command]
pub async fn location_page(
    app: AppHandle,
    generation: u64,
    offset: usize,
) -> Result<LocationPageView, CommandError> {
    app.state::<Session>().with_generation(generation,|open|{if offset>1_000_000{return Err(err("page limit"));}let total=open.conn.query_row("SELECT count(*) FROM grounded_location",[],|r|r.get(0))?;let mut stmt=open.conn.prepare("SELECT id,observation_id,entity_id,label,longitude,latitude,uncertainty,rationale,created_utc FROM grounded_location ORDER BY rowid DESC LIMIT 1000 OFFSET ?1")?;let rows=stmt.query_map([offset as i64],|r|Ok(LocationView{id:r.get(0)?,observation_id:r.get(1)?,entity_id:r.get(2)?,label:r.get(3)?,longitude:r.get(4)?,latitude:r.get(5)?,uncertainty:r.get(6)?,rationale:r.get(7)?,created_utc:r.get(8)?}))?.collect::<Result<Vec<_>,_>>()?;Ok(LocationPageView{rows,total,offset,limit:1000})})
}
#[tauri::command]
pub async fn location_create(
    app: AppHandle,
    generation: u64,
    request: LocationRequest,
) -> Result<String, CommandError> {
    app.state::<Session>()
        .with_generation(generation, |open| create_location(open, &request))
}
pub fn create_location(
    open: &mut kokin_store::OpenCase,
    r: &LocationRequest,
) -> Result<String, CommandError> {
    if !r.longitude.is_finite()
        || !r.latitude.is_finite()
        || !(-180.0..=180.0).contains(&r.longitude)
        || !(-90.0..=90.0).contains(&r.latitude)
        || [&r.label, &r.uncertainty, &r.rationale]
            .iter()
            .any(|v| v.trim().is_empty() || v.len() > 8192)
    {
        return Err(err(
            "enter valid coordinates, a label, uncertainty and source rationale",
        ));
    }
    crate::ai::verified_context(open, std::slice::from_ref(&r.observation_id))?;
    let id = kokin_ai::persistence::id().map_err(|_| err("id"))?;
    let tx = open.conn.transaction()?;
    tx.execute(
        "INSERT INTO grounded_location VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'user:local',?9)",
        params![
            id,
            r.observation_id,
            r.entity_id,
            r.label,
            r.longitude,
            r.latitude,
            r.uncertainty,
            r.rationale,
            kokin_store::now_utc_rfc3339()
        ],
    )?;
    kokin_store::append_audit_event(&tx,&kokin_store::NewAuditEvent{actor:"user:local",action:"location.recorded",subject_kind:Some("location"),subject_id:Some(&id),payload_json:&serde_json::json!({"observation_id":r.observation_id,"entity_id":r.entity_id,"uncertainty":r.uncertainty,"rationale":r.rationale}).to_string()})?;
    tx.commit()?;
    Ok(id)
}
#[tauri::command]
pub async fn map_state_read(app: AppHandle, generation: u64) -> Result<MapStateView, CommandError> {
    app.state::<Session>()
        .with_generation(generation, |open| state_read(&open.conn))
}
pub fn state_read(conn: &rusqlite::Connection) -> Result<MapStateView, CommandError> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT revision,payload_json FROM map_state_revision ORDER BY revision DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((revision, json)) = row {
        let mut value: MapStateView =
            serde_json::from_str(&json).map_err(|_| err("saved map state invalid"))?;
        value.revision = revision;
        Ok(value)
    } else {
        Ok(MapStateView {
            revision: 0,
            archive_id: None,
            longitude: 0.0,
            latitude: 0.0,
            zoom: 1.0,
            bearing: 0.0,
            pitch: 0.0,
            show_locations: true,
        })
    }
}
#[tauri::command]
pub async fn map_state_save(
    app: AppHandle,
    generation: u64,
    request: MapStateView,
) -> Result<MapStateView, CommandError> {
    app.state::<Session>()
        .with_generation(generation, |open| state_save(&mut open.conn, request))
}
pub fn state_save(
    conn: &mut rusqlite::Connection,
    mut request: MapStateView,
) -> Result<MapStateView, CommandError> {
    if request
        .archive_id
        .as_ref()
        .is_some_and(|id| id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()))
        || [
            request.longitude,
            request.latitude,
            request.zoom,
            request.bearing,
            request.pitch,
        ]
        .iter()
        .any(|v| !v.is_finite())
        || !(-180.0..=180.0).contains(&request.longitude)
        || !(-85.0512..=85.0512).contains(&request.latitude)
        || !(0.0..=22.0).contains(&request.zoom)
        || !(0.0..=60.0).contains(&request.pitch)
    {
        return Err(err("invalid map camera"));
    }
    let tx = conn.transaction()?;
    if state_read(&tx)?.revision != request.revision {
        return Err(err("map state changed; reload before saving"));
    }
    request.revision = request
        .revision
        .checked_add(1)
        .ok_or_else(|| err("map revision limit"))?;
    let json = serde_json::to_string(&request).map_err(|_| err("map state"))?;
    tx.execute(
        "INSERT INTO map_state_revision VALUES(?1,?2,?3)",
        params![request.revision, json, kokin_store::now_utc_rfc3339()],
    )?;
    tx.commit()?;
    Ok(request)
}

pub fn verify_illustration_archive(
    app: &AppHandle,
    request: &MapIllustrationRequest,
) -> Result<(), CommandError> {
    let archive = info(app, &request.archive_id)?;
    if archive.attribution != request.attribution {
        return Err(err(
            "illustration attribution differs from the imported archive; refresh before rendering",
        ));
    }
    Ok(())
}
pub fn save_illustration(
    open: &mut kokin_store::OpenCase,
    r: &MapIllustrationRequest,
) -> Result<serde_json::Value, CommandError> {
    use base64::Engine;
    if r.location_ids.len() > 1000 || r.image.len() > 12 * 1024 * 1024 {
        return Err(err("illustration exceeds limits"));
    }
    let encoded = r
        .image
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| err("illustration must be PNG"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| err("invalid image"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(err("illustration exceeds 8 MiB"));
    }
    let mut decoder =
        image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    decoder.limits(limits);
    decoder
        .decode()
        .map_err(|_| err("invalid or oversized PNG illustration"))?;
    for location in &r.location_ids {
        let oid: String = open.conn.query_row(
            "SELECT observation_id FROM grounded_location WHERE id=?1",
            [location],
            |r| r.get(0),
        )?;
        crate::ai::verified_context(open, &[oid])?;
    }
    let store = kokin_blob::BlobStore::new(open.paths.blobs());
    let outcome = crate::media::put_derivative(open, &store, &bytes)?;
    let hash = match &outcome {
        kokin_blob::PutOutcome::Stored(r) => r.content_hash.clone(),
        kokin_blob::PutOutcome::AlreadyPresent { content_hash, .. } => content_hash.clone(),
    };
    let id = kokin_ai::persistence::id().map_err(|_| err("illustration id"))?;
    let now = kokin_store::now_utc_rfc3339();
    let tx = open.conn.transaction()?;
    if let kokin_blob::PutOutcome::Stored(reference) = outcome {
        tx.execute(
            "INSERT INTO blob VALUES(?1,?2,?3,?4,?5,NULL)",
            params![
                reference.content_hash,
                reference.size as i64,
                reference.wrapped_key.ciphertext,
                reference.wrapped_key.nonce.as_slice(),
                now
            ],
        )?;
    }
    tx.execute(
        "INSERT INTO map_illustration VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            id,
            hash,
            r.archive_id,
            r.attribution,
            serde_json::to_string(&r.location_ids).map_err(|_| err("location refs"))?,
            now
        ],
    )?;
    kokin_store::append_audit_event(&tx,&kokin_store::NewAuditEvent{actor:"user:local",action:"map.illustration_saved",subject_kind:Some("map_illustration"),subject_id:Some(&id),payload_json:&serde_json::json!({"archive_id":r.archive_id,"location_ids":r.location_ids,"attribution":r.attribution,"analyst_rendered":true}).to_string()})?;
    tx.commit()?;
    Ok(
        serde_json::json!({"illustration_id":id,"analyst_rendered":true,"basemap_excluded_from_package":true}),
    )
}
#[tauri::command]
pub async fn map_illustrations(
    app: AppHandle,
    generation: u64,
) -> Result<Vec<MapIllustrationView>, CommandError> {
    app.state::<Session>().with_generation(generation,|open|{let mut stmt=open.conn.prepare("SELECT id,created_utc,archive_id,attribution FROM map_illustration ORDER BY rowid DESC LIMIT 100")?;let rows=stmt.query_map([],|r|Ok(MapIllustrationView{id:r.get(0)?,created_utc:r.get(1)?,archive_id:r.get(2)?,attribution:r.get(3)?}))?.collect::<Result<_,_>>()?;Ok(rows)})
}
pub fn illustration(
    open: &kokin_store::OpenCase,
    id: &str,
) -> Result<(Vec<u8>, String, Vec<String>), CommandError> {
    let (hash, credit, ids): (String, String, String) = open.conn.query_row(
        "SELECT content_hash,attribution,location_ids_json FROM map_illustration WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let kokin_store::BlobAccess::Readable(reference) = kokin_store::blob_access(&open.conn, &hash)?
    else {
        return Err(err("illustration bytes unavailable"));
    };
    if reference.size > 8 * 1024 * 1024 {
        return Err(err("illustration exceeds limits"));
    }
    let bytes = kokin_blob::BlobStore::new(open.paths.blobs()).get(&reference, &open.cmk)?;
    Ok((
        bytes,
        credit,
        serde_json::from_str(&ids).map_err(|_| err("illustration references invalid"))?,
    ))
}

struct WorldSelection {
    plan: kokin_maps::world::Plan,
    etag: String,
    size: u64,
    expires: Instant,
}
#[tauri::command]
pub async fn map_world_review(
    app: AppHandle,
    consent: bool,
) -> Result<WorldSelectionView, CommandError> {
    if !consent {
        return Err(err("approve the disclosed public metadata request first"));
    }
    let (job, cancel) = app
        .state::<crate::resources::ResourceJobs>()
        .begin("world-review", 8 * 1024 * 1024)?;
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let mut transferred = 127;

            let mut source = kokin_net::map_resource::WorldSource::default();
            let header = source
                .read(0, 127, &cancel)
                .map_err(|e| err(e.to_string()))?;
            let size = source.identity().ok_or_else(|| err("source identity"))?.1;
            let plan = kokin_maps::world::plan(&header, size, |offset, length| {
                let bytes = source
                    .read(offset, length, &cancel)
                    .map_err(|e| kokin_maps::Error(e.to_string()))?;
                transferred += length;
                app.state::<crate::resources::ResourceJobs>()
                    .progress(&job, transferred);
                Ok(bytes)
            })
            .map_err(|e| err(e.to_string()))?;
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(err("world metadata review cancelled"));
            }
            let etag = source
                .identity()
                .ok_or_else(|| err("source identity"))?
                .0
                .to_owned();
            let token = kokin_ai::persistence::id().map_err(|_| err("id"))?;
            let view = WorldSelectionView {
                selection: token.clone(),
                url: kokin_net::map_resource::WORLD_URL.into(),
                etag: etag.clone(),
                download_bytes: plan.download_bytes,
                maximum_output_bytes: plan.output_bytes,
                destination: root(&app)?.to_string_lossy().into_owned(),
            };
            let state = app.state::<MapResources>();
            let mut pending = state
                .worlds
                .lock()
                .map_err(|_| err("world resource registry"))?;
            pending.retain(|_, s| s.expires > Instant::now());
            if pending.len() >= 4 {
                return Err(err("pending world selection limit"));
            }
            pending.insert(
                token,
                WorldSelection {
                    plan,
                    etag,
                    size,
                    expires: Instant::now() + Duration::from_secs(600),
                },
            );
            Ok(view)
        })();
        app.state::<crate::resources::ResourceJobs>().finish(
            &job,
            result
                .as_ref()
                .map(|view: &WorldSelectionView| view.selection.clone())
                .map_err(Clone::clone),
        );
        result
    })
    .await
    .map_err(|_| err("world metadata review failed"))?
}
#[tauri::command]
pub fn map_world_install(
    app: AppHandle,
    selection: String,
    consent: bool,
) -> Result<String, CommandError> {
    if !consent {
        return Err(err("review actual size and destination before downloading"));
    }
    let selected = app
        .state::<MapResources>()
        .worlds
        .lock()
        .map_err(|_| err("world registry"))?
        .remove(&selection)
        .ok_or_else(|| err("world selection expired"))?;
    if selected.expires < Instant::now() {
        return Err(err("world selection expired; review again"));
    }
    let (job, cancel) = app
        .state::<crate::resources::ResourceJobs>()
        .begin("world-z0-z6", selected.plan.download_bytes)?;
    let operation = job.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let mut source =
                kokin_net::map_resource::WorldSource::resume(selected.etag.clone(), selected.size);
            let mut transferred = 0;
            let bytes = selected
                .plan
                .assemble(|offset, length| {
                    let bytes = source
                        .read(offset, length, &cancel)
                        .map_err(|e| kokin_maps::Error(e.to_string()))?;
                    transferred += length;
                    app.state::<crate::resources::ResourceJobs>()
                        .progress(&job, transferred);
                    Ok(bytes)
                })
                .map_err(|e| err(e.to_string()))?;
            let directory = root(&app)?;
            std::fs::create_dir_all(&directory).map_err(|_| err("map resource destination"))?;
            let stage = directory.join(format!(".incoming-{job}"));
            let publication = (|| {
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&stage)
                    .map_err(|_| err("world staging"))?;
                output
                    .write_all(&bytes)
                    .and_then(|_| output.sync_all())
                    .map_err(|_| err("world staging write failed"))?;
                drop(output);
                kokin_maps::validate(&stage).map_err(|e| err(e.to_string()))?;
                let id = format!("{:x}", Sha256::digest(&bytes));
                let destination = path(&app, &id)?;
                app.state::<crate::resources::ResourceJobs>()
                    .publish(&job)?;
                if !destination.exists() {
                    let credit = destination.with_extension("credits.txt");
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&credit)
                        .map_err(|_| err("world credits destination"))?;
                    let attribution=format!("© OpenStreetMap contributors (ODbL); Natural Earth (public domain); Protomaps. World z0–z6 extracted from {}. Source ETag {}; archive SHA-256 {}. ETag consistency is not a digital signature.",kokin_net::map_resource::WORLD_URL,selected.etag,id);
                    if file
                        .write_all(attribution.as_bytes())
                        .and_then(|_| file.sync_all())
                        .and_then(|_| std::fs::hard_link(&stage, &destination))
                        .is_err()
                    {
                        std::fs::remove_file(credit)
                            .map_err(|_| err("world publication cleanup failed"))?;
                        return Err(err("world publication failed; no archive replaced"));
                    }
                }
                Ok(id)
            })();
            if stage.exists() {
                std::fs::remove_file(stage).map_err(|_| err("world staging cleanup failed"))?;
            }
            publication
        })();
        app.state::<crate::resources::ResourceJobs>()
            .finish(&job, result);
    });
    Ok(operation)
}
