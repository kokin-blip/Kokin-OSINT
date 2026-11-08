//! Media orchestration: decrypt briefly, parse without the case lock, commit by generation.
use crate::{CommandError, ErrorCode};
use kokin_store::OpenCase;
use rusqlite::params;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
fn error(e: impl std::fmt::Display) -> CommandError {
    CommandError::new(ErrorCode::FileUnreadable, e.to_string())
}
fn invalid(s: &str) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, s)
}
pub fn supported(media: &str) -> bool {
    matches!(
        media,
        "application/pdf" | "image/jpeg" | "image/png" | "image/webp" | "image/tiff"
    )
}
fn id() -> Result<String, CommandError> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(error)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Ok(bytes
        .iter()
        .flat_map(|b| {
            [
                char::from(HEX[(b >> 4) as usize]),
                char::from(HEX[(b & 15) as usize]),
            ]
        })
        .collect())
}
pub fn source(open: &OpenCase, artifact: &str) -> Result<(String, Vec<u8>), CommandError> {
    let (media, length, hash): (String, i64, String) = open
        .conn
        .query_row(
            "SELECT media_type,byte_length,content_hash FROM artifact WHERE id=?1",
            [artifact],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(error)?;
    if !supported(&media) {
        return Err(invalid(
            "unsupported media format; original evidence is preserved",
        ));
    }
    if length < 0 || length as usize > kokin_media::MAX_INPUT {
        return Err(invalid(
            "media exceeds the 64 MiB processing limit; original evidence is preserved",
        ));
    }
    Ok((media, blob(open, &hash, kokin_media::MAX_INPUT)?))
}
fn blob(open: &OpenCase, hash: &str, max: usize) -> Result<Vec<u8>, CommandError> {
    let kokin_store::BlobAccess::Readable(reference) = kokin_store::blob_access(&open.conn, hash)?
    else {
        return Err(error("source or derivative is unavailable"));
    };
    if reference.size > max as u64 {
        return Err(invalid("media read exceeds its bound"));
    }
    kokin_blob::BlobStore::new(open.paths.blobs())
        .get(&reference, &open.cmk)
        .map_err(error)
}
pub struct MediaJob<'a> {
    pub artifact: &'a str,
    pub page: u16,
    pub ocr: bool,
    pub preview_only: bool,
    pub generation: u64,
}
pub fn run(
    app: &tauri::AppHandle,
    job: &str,
    request: MediaJob<'_>,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    let MediaJob {
        artifact,
        page,
        ocr,
        preview_only,
        generation,
    } = request;
    use tauri::Manager;
    let session = app.state::<crate::Session>();
    let (media, bytes) = session.with_generation(generation, |open| {
        let pages = open.conn.query_row(
            "SELECT pages FROM media_derivative WHERE artifact_id=?1 ORDER BY rowid DESC LIMIT 1",
            [artifact],
            |r| r.get::<_, u16>(0),
        );
        if let Ok(pages) = pages {
            if page >= pages {
                return Err(invalid(
                    "selected page is outside the known document page count",
                ));
            }
        }
        source(open, artifact)
    })?;
    let resources = app.path().resource_dir().map_err(error)?;
    let binary = std::env::current_exe()
        .map_err(error)?
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
    // No webview paths and no environment-variable runtime override in production.
    let data = app
        .path()
        .app_data_dir()
        .map_err(error)?
        .join("resources/ocr-eng");
    if ocr && verify_resource(&data.join("eng.traineddata")).is_err() {
        return Err(invalid("English OCR data is not installed; install the reviewed resource before requesting OCR"));
    }
    let request = kokin_media::Request {
        version: 1,
        media_type: media,
        page,
        ocr,
        image_recipe: None,
    };
    let processed = kokin_media::worker::run(
        &binary,
        &native,
        ocr.then_some(data.as_path()),
        bytes,
        &request,
        cancel.clone(),
    );
    let mut result = match processed {
        Ok(result) => result,
        Err(failure) => {
            let detail = failure.to_string();
            session.with_generation(generation, |open| {
                record_failure(open, artifact, preview_only, &detail)
            })?;
            return Err(error(detail));
        }
    };
    if ocr {
        result.observations.retain(|o| o.kind == "ocr.word");
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(invalid("media cancelled before commit"));
    }
    app.state::<crate::operations::Operations>()
        .begin_publish(job)?;
    session.with_generation(generation, |open| {
        commit_mode(open, artifact, page, ocr, preview_only, result)
    })
}
pub fn commit(
    open: &mut OpenCase,
    artifact: &str,
    page: u16,
    ocr: bool,
    result: kokin_media::Result,
) -> Result<serde_json::Value, CommandError> {
    commit_mode(open, artifact, page, ocr, false, result)
}
fn commit_mode(
    open: &mut OpenCase,
    artifact: &str,
    page: u16,
    ocr: bool,
    preview_only: bool,
    mut result: kokin_media::Result,
) -> Result<serde_json::Value, CommandError> {
    if preview_only {
        result.observations.clear();
        result.gaps =
            vec!["Derived page preview only; extraction remains a separate action.".into()];
    }

    let now = kokin_store::now_utc_rfc3339();
    let run = id()?;
    let transform = if preview_only {
        format!("render.media.page.{page}")
    } else if ocr {
        format!("extract.media.ocr.page.{page}")
    } else {
        "extract.media.native".into()
    };
    let store = kokin_blob::BlobStore::new(open.paths.blobs());
    let thumbnail = put_derivative(open, &store, &result.thumbnail_png)?;
    let snapshot_bytes = serde_json::to_vec(&result).map_err(error)?;
    let snapshot = put_derivative(open, &store, &snapshot_bytes)?;
    let tx = open.conn.transaction().map_err(error)?;
    for put in [&thumbnail, &snapshot] {
        if let kokin_blob::PutOutcome::Stored(reference) = put {
            tx.execute("INSERT INTO blob(content_hash,size_bytes,wrapped_key,wrapped_nonce,stored_utc,shredded_utc) VALUES(?1,?2,?3,?4,?5,NULL)",params![reference.content_hash,reference.size as i64,reference.wrapped_key.ciphertext,reference.wrapped_key.nonce.as_slice(),now]).map_err(error)?;
        }
    }
    let hash = |put: &kokin_blob::PutOutcome| match put {
        kokin_blob::PutOutcome::Stored(r) => r.content_hash.clone(),
        kokin_blob::PutOutcome::AlreadyPresent { content_hash, .. } => content_hash.clone(),
    };
    let snapshot_hash = hash(&snapshot);
    let thumb_hash = hash(&thumbnail);
    tx.execute(
        "INSERT INTO transform_run VALUES(?1,?2,'1',?3,'media-1',?4,?4,'succeeded',NULL,NULL)",
        params![run, transform, env!("CARGO_PKG_VERSION"), now],
    )
    .map_err(error)?;
    if !preview_only {
        tx.execute(
            "INSERT INTO run_subject VALUES(?1,'artifact',?2)",
            params![run, artifact],
        )
        .map_err(error)?;
    }
    let previous: Vec<(String, String, String)> = {
        let mut stmt=tx.prepare("SELECT o.id,o.kind,o.locator_json FROM observation o JOIN transform_run r ON r.id=o.run_id WHERE o.artifact_id=?1 AND r.transform_name=?2 AND o.id NOT IN(SELECT superseded_id FROM observation_supersession)").map_err(error)?;
        let rows = stmt
            .query_map(params![artifact, transform], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(error)?
            .collect::<Result<_, _>>()
            .map_err(error)?;
        drop(stmt);
        rows
    };
    let mut replacements = std::collections::HashMap::new();
    for (index, observation) in result.observations.iter().enumerate() {
        let oid = id()?;
        let mut locator = observation.locator.clone();
        locator["kind"] = locator["scheme"].clone();
        locator["snapshot_hash"] = snapshot_hash.clone().into();
        locator["snapshot_index"] = (index as u64).into();
        replacements.insert(
            (observation.kind.clone(), observation.locator.to_string()),
            oid.clone(),
        );
        tx.execute(
            "INSERT INTO observation VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                oid,
                artifact,
                run,
                observation.kind,
                observation.value,
                locator.to_string(),
                now
            ],
        )
        .map_err(error)?;
        tx.execute(
            "INSERT INTO derivation VALUES(?1,?2,'artifact',?3,'observation',?4)",
            params![id()?, run, artifact, oid],
        )
        .map_err(error)?;
    }
    for (old, kind, raw) in &previous {
        let mut locator: serde_json::Value = serde_json::from_str(raw).map_err(error)?;
        if let Some(object) = locator.as_object_mut() {
            object.remove("kind");
            object.remove("snapshot_hash");
            object.remove("snapshot_index");
        }
        let replacement = replacements.get(&(kind.clone(), locator.to_string()));
        tx.execute(
            "INSERT INTO observation_supersession VALUES(?1,?2,?3,?4)",
            params![old, replacement, run, now],
        )
        .map_err(error)?;
    }
    for gap in &result.gaps {
        tx.execute("INSERT INTO coverage_gap VALUES(?1,?2,'artifact',?3,'media_partial',?4,1,'limitations',?5)",params![id()?,run,artifact,gap,now]).map_err(error)?;
    }
    let derivative = id()?;
    tx.execute(
        "INSERT INTO media_derivative VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            derivative,
            artifact,
            run,
            page,
            result.pages,
            thumb_hash,
            snapshot_hash,
            result.perceptual_hash,
            result.engine,
            now
        ],
    )
    .map_err(error)?;
    tx.execute(
        "INSERT INTO derivation VALUES(?1,?2,'artifact',?3,'media_derivative',?4)",
        params![id()?, run, artifact, derivative],
    )
    .map_err(error)?;
    kokin_store::audit::append(&tx,&kokin_store::NewAuditEvent{actor:"user:local",action:if preview_only{"evidence.media_previewed"}else{"evidence.media_extracted"},subject_kind:Some("artifact"),subject_id:Some(artifact),payload_json:&serde_json::json!({"run_id":run,"observations":result.observations.len(),"page":page,"ocr":ocr,"engine":result.engine}).to_string()})?;
    tx.commit().map_err(error)?;
    Ok(
        serde_json::json!({"run_id":run,"artifact_id":artifact,"observations":result.observations.len(),"superseded":previous.len(),"pages":result.pages,"page":page,"complete":result.gaps.is_empty(),"limitations":result.gaps}),
    )
}
/// Verify both original bytes and the encrypted transform snapshot. This is a
/// transformed-value check, explicitly weaker than an independently reproduced OCR.
pub fn citation(
    open: &OpenCase,
    artifact: &str,
    locator: &serde_json::Value,
    value: &str,
) -> Result<Option<(String, String)>, CommandError> {
    let Some(hash) = locator["snapshot_hash"].as_str() else {
        return Ok(None);
    };
    let Some(index) = locator["snapshot_index"].as_u64() else {
        return Ok(None);
    };
    let (source_hash,engine):(String,String)=open.conn.query_row("SELECT a.content_hash,m.engine FROM media_derivative m JOIN artifact a ON a.id=m.artifact_id WHERE m.artifact_id=?1 AND m.snapshot_hash=?2 ORDER BY m.rowid DESC LIMIT 1",params![artifact,hash],|r|Ok((r.get(0)?,r.get(1)?))).map_err(error)?;
    let kokin_store::BlobAccess::Readable(reference) =
        kokin_store::blob_access(&open.conn, &source_hash)?
    else {
        return Err(error("original source is unavailable"));
    };
    kokin_blob::BlobStore::new(open.paths.blobs())
        .read_to(&reference, &open.cmk, &mut std::io::sink())
        .map_err(error)?;
    let snapshot: kokin_media::Result =
        serde_json::from_slice(&blob(open, hash, kokin_media::MAX_OUTPUT)?).map_err(error)?;
    let Some(observation) = snapshot.observations.get(index as usize) else {
        return Ok(None);
    };
    let mut original_locator = locator.clone();
    if let Some(object) = original_locator.as_object_mut() {
        object.remove("kind");
        object.remove("snapshot_hash");
        object.remove("snapshot_index");
    }
    if observation.value != value || observation.locator != original_locator {
        return Ok(None);
    };
    let basis = if observation.kind == "image.query_recipe" {
        format!("Matches the recorded preparation recipe in the encrypted {engine} snapshot; the derived PNG bytes are intact. This does not independently reproduce the recipe or verify original source pixels.")
    } else if observation.kind.starts_with("ocr.") {
        format!("Matches recorded OCR from {engine}; source bytes are intact. This does not independently verify the words against the image.")
    } else {
        format!("Matches the encrypted {engine} transformation snapshot; source bytes are intact. Inspect the original page/image and recorded location.")
    };
    Ok(Some((observation.value.clone(), basis)))
}
#[tauri::command]
pub fn media_read(
    session: tauri::State<'_, crate::Session>,
    generation: u64,
    artifact_id: String,
    page: u16,
) -> Result<Option<crate::views::MediaView>, CommandError> {
    session.with_generation(generation, |open| preview(open, &artifact_id, page))
}
pub fn preview(
    open: &OpenCase,
    artifact: &str,
    page: u16,
) -> Result<Option<crate::views::MediaView>, CommandError> {
    use base64::Engine;
    let row=open.conn.query_row("SELECT thumbnail_hash,snapshot_hash FROM media_derivative WHERE artifact_id=?1 AND page=?2 ORDER BY rowid DESC LIMIT 1",params![artifact,page],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)));
    let (thumbnail, snapshot) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(error(e)),
    };
    let result: kokin_media::Result =
        serde_json::from_slice(&blob(open, &snapshot, kokin_media::MAX_OUTPUT)?).map_err(error)?;
    let png = blob(open, &thumbnail, 8 * 1024 * 1024)?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(error("invalid encrypted preview"));
    }
    let mut candidates = Vec::new();
    let mut scan_truncated = false;
    if let Some(phash) = result
        .perceptual_hash
        .as_ref()
        .and_then(|s| u64::from_str_radix(s, 16).ok())
    {
        let mut stmt=open.conn.prepare("SELECT m.artifact_id,m.perceptual_hash FROM media_derivative m JOIN artifact a ON a.id=m.artifact_id JOIN blob b ON b.content_hash=a.content_hash JOIN blob mb ON mb.content_hash=m.snapshot_hash WHERE b.wrapped_key IS NOT NULL AND mb.wrapped_key IS NOT NULL AND m.artifact_id<>?1 AND perceptual_hash IS NOT NULL AND NOT EXISTS(SELECT 1 FROM media_derivative newer WHERE newer.artifact_id=m.artifact_id AND newer.rowid>m.rowid) ORDER BY m.rowid DESC LIMIT 5001").map_err(error)?;
        for (index, row) in stmt
            .query_map([artifact], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(error)?
            .enumerate()
        {
            if index >= 5000 {
                scan_truncated = true;
                break;
            }
            let (aid, hash) = row.map_err(error)?;
            if u64::from_str_radix(&hash, 16).is_ok_and(|h| (phash ^ h).count_ones() <= 8) {
                if candidates.len() < 20 {
                    candidates.push(aid);
                } else {
                    scan_truncated = true;
                }
            }
        }
    }
    Ok(Some(crate::views::MediaView {
        artifact_id: artifact.into(),
        page,
        pages: result.pages,
        preview_png: base64::engine::general_purpose::STANDARD.encode(png),
        engine: result.engine,
        limitations: result.gaps,
        duplicate_candidates: candidates,
        candidate_scan_truncated: scan_truncated,
    }))
}
const OCR_SIZE: u64 = 4_113_088;
const OCR_SHA: &str = "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2";
fn resource_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, CommandError> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(error)?
        .join("resources/ocr-eng/eng.traineddata"))
}
fn verify_resource(path: &std::path::Path) -> Result<(), CommandError> {
    use sha2::Digest;
    let metadata = std::fs::metadata(path).map_err(error)?;
    if !metadata.is_file() || metadata.len() != OCR_SIZE {
        return Err(invalid("OCR resource does not match the reviewed size"));
    }
    let bytes = std::fs::read(path).map_err(error)?;
    if format!("{:x}", sha2::Sha256::digest(bytes)) != OCR_SHA {
        return Err(invalid(
            "OCR resource identity is not the reviewed English model",
        ));
    }
    Ok(())
}
#[tauri::command]
pub fn ocr_resource_info(
    app: tauri::AppHandle,
) -> Result<crate::views::OcrResourceView, CommandError> {
    let path = resource_path(&app)?;
    Ok(crate::views::OcrResourceView {
        size_bytes: OCR_SIZE,
        destination: path.to_string_lossy().into_owned(),
        installed: verify_resource(&path).is_ok(),
        license: "Apache-2.0; tessdata_fast 87416418657359cb625c412a48b6e1d6d41c29bd".into(),
        sha256: OCR_SHA.into(),
    })
}
/// Native selection only. Installations are global resources, not case writes.
#[tauri::command]
pub async fn install_ocr_resource(app: tauri::AppHandle) -> Result<bool, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Select reviewed English eng.traineddata (3.92 MiB)")
            .add_filter("OCR data", &["traineddata"])
            .pick_file()
        else {
            return Ok(false);
        };
        let destination = resource_path(&app)?;
        install_resource(&path, &destination)?;
        Ok(true)
    })
    .await
    .map_err(|_| error("OCR resource installation failed"))?
}
fn install_resource(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), CommandError> {
    use sha2::Digest;
    use std::io::Write;
    // Read once and verify those same bytes; a path swap cannot change the installed artifact.
    let mut bytes = Vec::new();
    use std::io::Read;
    std::fs::File::open(source)
        .map_err(error)?
        .take(OCR_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() != OCR_SIZE as usize || format!("{:x}", sha2::Sha256::digest(&bytes)) != OCR_SHA
    {
        return Err(invalid(
            "selected OCR data is not the reviewed English resource",
        ));
    }
    if destination.exists() {
        verify_resource(destination)?;
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("invalid resource destination"))?;
    std::fs::create_dir_all(parent).map_err(error)?;
    let staging = parent.join(format!("{}.incoming", id()?));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(error)?;
        file.write_all(&bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        drop(file);
        std::fs::hard_link(&staging, destination).map_err(error)
    })();
    let cleanup = std::fs::remove_file(&staging);
    result?;
    cleanup.map_err(error)?;
    verify_resource(destination)
}
fn record_failure(
    open: &mut OpenCase,
    artifact: &str,
    preview_only: bool,
    detail: &str,
) -> Result<(), CommandError> {
    let run = id()?;
    let now = kokin_store::now_utc_rfc3339();
    let tx = open.conn.transaction().map_err(error)?;
    tx.execute("INSERT INTO transform_run VALUES(?1,?2,'1',?3,'media-1',?4,?4,'failed','media_worker_failed',?5)",params![run,if preview_only{"render.media.preview"}else{"extract.media.native"},env!("CARGO_PKG_VERSION"),now,detail]).map_err(error)?;
    if !preview_only {
        tx.execute(
            "INSERT INTO run_subject VALUES(?1,'artifact',?2)",
            params![run, artifact],
        )
        .map_err(error)?;
    }
    kokin_store::audit::append(
        &tx,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action: "evidence.media_failed",
            subject_kind: Some("artifact"),
            subject_id: Some(artifact),
            payload_json: &serde_json::json!({"run_id":run,"error":"media_worker_failed"})
                .to_string(),
        },
    )?;
    tx.commit().map_err(error)
}

pub(crate) fn put_derivative(
    open: &OpenCase,
    store: &kokin_blob::BlobStore,
    bytes: &[u8],
) -> Result<kokin_blob::PutOutcome, CommandError> {
    let outcome = store
        .put(std::io::Cursor::new(bytes), &open.cmk)
        .map_err(error)?;
    if let kokin_blob::PutOutcome::AlreadyPresent { content_hash, .. } = &outcome {
        match kokin_store::blob_access(&open.conn, content_hash)? {
            kokin_store::BlobAccess::Unrecorded => {
                store.remove(content_hash, &open.cmk).map_err(error)?;
                let retried = store
                    .put(std::io::Cursor::new(bytes), &open.cmk)
                    .map_err(error)?;
                if matches!(retried, kokin_blob::PutOutcome::AlreadyPresent { .. }) {
                    return Err(error("concurrent unrecorded derivative publication"));
                }
                return Ok(retried);
            }
            kokin_store::BlobAccess::Shredded { .. } => return Err(error(
                "these derived bytes were previously shredded; they cannot be silently recreated",
            )),
            kokin_store::BlobAccess::Readable(_) => {}
        }
    }
    Ok(outcome)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod resource_tests {
    use super::*;
    #[test]
    fn rendering_only_does_not_extract_or_supersede_observations() {
        let root =
            std::env::temp_dir().join(format!("kokin-media-preview-only-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let (mut open, _) =
            kokin_store::create_case(root.join("case.kokincase"), "preview-only", "synthetic key")
                .unwrap();
        let input = root.join("image.png");
        std::fs::write(&input, b"synthetic raw source").unwrap();
        let artifact = crate::write::ingest_file(
            &mut open,
            &crate::views::IngestFileRequest {
                path: input.to_string_lossy().into_owned(),
            },
        )
        .unwrap()
        .artifact_id;
        let result = kokin_media::Result {
            version: 1,
            engine: "synthetic fixture".into(),
            observations: vec![kokin_media::Observation {
                kind: "image.dimensions".into(),
                value: "2 pixels".into(),
                locator: serde_json::json!({"scheme":"image_dimensions"}),
            }],
            thumbnail_png: b"\x89PNG\r\n\x1a\nsynthetic protocol preview".to_vec(),
            perceptual_hash: None,
            pages: 1,
            gaps: vec![],
        };
        commit_mode(&mut open, &artifact, 0, false, true, result).unwrap();
        let count: i64 = open
            .conn
            .query_row("SELECT count(*) FROM observation", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(matches!(
            kokin_search::coverage::artifact_coverage(&open.conn, &artifact)
                .unwrap()
                .unwrap()
                .state,
            kokin_search::coverage::CoverageState::NeverAttempted
        ));
        drop(open);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_resources_never_publish_or_replace_existing_data() {
        let root =
            std::env::temp_dir().join(format!("kokin-ocr-resource-invalid-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let source = root.join("candidate.traineddata");
        std::fs::write(&source, b"wrong model").unwrap();
        let destination = root.join("eng.traineddata");
        assert!(install_resource(&source, &destination).is_err());
        assert!(!destination.exists());
        std::fs::write(&destination, b"preserve existing").unwrap();
        assert!(install_resource(&source, &destination).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"preserve existing");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "explicit acceptance using the already reviewed local English OCR asset"]
    fn reviewed_resource_installation_is_atomic_and_idempotent() {
        let source = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_OCR_FILE").unwrap());
        let root =
            std::env::temp_dir().join(format!("kokin-ocr-resource-valid-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("eng.traineddata");
        install_resource(&source, &destination).unwrap();
        verify_resource(&destination).unwrap();
        install_resource(&source, &destination).unwrap();
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
