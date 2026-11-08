//! Native global resources: never case jobs, browser paths, or silent downloads.
use crate::{
    views::{ResourceInstallRequest, ResourceJobView, ResourceView},
    CommandError, ErrorCode,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Manager};
pub struct ResourceJobs {
    jobs: Mutex<HashMap<String, (ResourceJobView, Arc<AtomicBool>)>>,
}
impl Default for ResourceJobs {
    fn default() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
        }
    }
}
fn err(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, message)
}
pub fn root(app: &AppHandle) -> Result<PathBuf, CommandError> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| err("resource folder unavailable"))?
        .join("resources/local"))
}
pub fn catalogue(app: &AppHandle) -> Result<Vec<ResourceView>, CommandError> {
    let directory = root(app)?;
    let mut resources = Vec::new();
    for c in kokin_ai::registry()
        .map_err(|_| err("model registry invalid"))?
        .candidates
    {
        resources.push(ResourceView {
            id: c.id,
            file: c.file.clone(),
            bytes: c.bytes,
            sha256: c.sha256,
            url: c.download_url,
            license: c.license,
            destination: directory.join(&c.file).to_string_lossy().into_owned(),
            present: directory.join(c.file).is_file(),
        });
    }
    let registry: serde_json::Value =
        serde_json::from_str(include_str!("../../resources/embedding-registry.json"))
            .map_err(|_| err("embedding registry"))?;
    for r in registry["files"]
        .as_array()
        .ok_or_else(|| err("embedding files"))?
    {
        let file = r["file"].as_str().ok_or_else(|| err("file"))?;
        resources.push(ResourceView {
            id: r["id"].as_str().unwrap_or_default().into(),
            file: file.into(),
            bytes: r["bytes"].as_u64().unwrap_or(0),
            sha256: r["sha256"].as_str().unwrap_or_default().into(),
            url: r["url"].as_str().unwrap_or_default().into(),
            license: "MIT".into(),
            destination: directory.join(file).to_string_lossy().into_owned(),
            present: directory.join(file).is_file(),
        });
    }
    let face_registry: serde_json::Value =
        serde_json::from_str(include_str!("../../resources/face-registry.json"))
            .map_err(|_| err("face registry"))?;
    for r in face_registry["files"]
        .as_array()
        .ok_or_else(|| err("face files"))?
    {
        let file = r["file"].as_str().ok_or_else(|| err("face filename"))?;
        resources.push(ResourceView {
            id: r["id"].as_str().unwrap_or_default().into(),
            file: file.into(),
            bytes: r["bytes"].as_u64().unwrap_or(0),
            sha256: r["sha256"].as_str().unwrap_or_default().into(),
            url: r["url"].as_str().unwrap_or_default().into(),
            license: r["license"].as_str().unwrap_or_default().into(),
            destination: directory.join(file).to_string_lossy().into_owned(),
            present: directory.join(file).is_file(),
        });
    }
    Ok(resources)
}
pub fn verify(
    path: &Path,
    bytes: u64,
    digest: &str,
    cancel: &AtomicBool,
) -> Result<(), CommandError> {
    let mut file = std::fs::File::open(path).map_err(|_| err("resource missing"))?;
    if file.metadata().map_err(|_| err("resource metadata"))?.len() != bytes {
        return Err(err("resource size mismatch"));
    }
    let mut hash = Sha256::new();
    let mut block = [0; 65536];
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(err("resource operation cancelled"));
        }
        let n = file
            .read(&mut block)
            .map_err(|_| err("resource unreadable"))?;
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    if format!("{:x}", hash.finalize()) != digest {
        return Err(err("resource identity mismatch"));
    }
    Ok(())
}
pub fn selected(app: &AppHandle, id: &str) -> Result<ResourceView, CommandError> {
    catalogue(app)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| err("unknown reviewed resource"))
}
#[tauri::command]
pub async fn resource_list(app: AppHandle) -> Result<Vec<ResourceView>, CommandError> {
    catalogue(&app)
}
#[tauri::command]
pub fn resource_status(
    app: AppHandle,
    operation_id: String,
) -> Result<ResourceJobView, CommandError> {
    app.state::<ResourceJobs>()
        .jobs
        .lock()
        .map_err(|_| err("resource registry"))?
        .get(&operation_id)
        .map(|r| r.0.clone())
        .ok_or_else(|| err("unknown resource operation"))
}
#[tauri::command]
pub fn resource_cancel(app: AppHandle, operation_id: String) -> Result<(), CommandError> {
    let state = app.state::<ResourceJobs>();
    let jobs = state.jobs.lock().map_err(|_| err("resource registry"))?;
    let row = jobs
        .get(&operation_id)
        .ok_or_else(|| err("unknown resource operation"))?;
    if row.0.stage.starts_with("Publishing") {
        return Err(err("resource publication has begun; wait for the result"));
    }
    row.1.store(true, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
pub fn resource_install(
    app: AppHandle,
    request: ResourceInstallRequest,
) -> Result<String, CommandError> {
    if request.version != 1
        || !request.consent
        || !matches!(request.mode.as_str(), "download" | "import")
    {
        return Err(err("review the resource size/destination and opt in first"));
    }
    let resource = selected(&app, &request.resource_id)?;
    let id = kokin_ai::persistence::id().map_err(|_| err("random id"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<ResourceJobs>();
        let mut jobs = state.jobs.lock().map_err(|_| err("resource registry"))?;
        if jobs.values().any(|r| r.0.status == "running") {
            return Err(err("another global resource operation is running"));
        }
        if jobs.len() > 32 {
            jobs.retain(|_, r| r.0.status == "running");
        }
        jobs.insert(
            id.clone(),
            (
                ResourceJobView {
                    operation_id: id.clone(),
                    resource_id: resource.id.clone(),
                    status: "running".into(),
                    stage: "Preparing reviewed resource".into(),
                    transferred: 0,
                    bytes: resource.bytes,
                    error: None,
                },
                cancel.clone(),
            ),
        );
    }
    let job = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            install(&app, &job, &request.mode, &resource, &cancel)
        }))
        .unwrap_or_else(|_| Err(err("resource worker failed; installation not complete")));
        if let Ok(mut jobs) = app.state::<ResourceJobs>().jobs.lock() {
            if let Some(row) = jobs.get_mut(&job) {
                row.0.status = match &result {
                    Ok(()) => "completed",
                    Err(_) if cancel.load(Ordering::SeqCst) => "cancelled",
                    Err(_) => "failed",
                }
                .into();
                row.0.stage = "Finished".into();
                row.0.error = result.err().map(|e| e.message);
            }
        }
    });
    Ok(id)
}
fn install(
    app: &AppHandle,
    id: &str,
    mode: &str,
    resource: &ResourceView,
    cancel: &AtomicBool,
) -> Result<(), CommandError> {
    let directory = root(app)?;
    std::fs::create_dir_all(&directory).map_err(|_| err("resource folder could not be created"))?;
    let destination = directory.join(&resource.file);
    if destination.exists() {
        return verify(&destination, resource.bytes, &resource.sha256, cancel);
    }
    let input = if mode == "import" {
        #[cfg(feature = "desktop-test")]
        let queued = {
            let operations = app.state::<crate::operations::Operations>();
            let mut state = operations
                .test_state
                .lock()
                .map_err(|_| err("test selection registry"))?;
            if state
                .selections
                .front()
                .is_some_and(|(purpose, _)| purpose == "resource_import")
            {
                state.selections.pop_front().map(|(_, path)| path)
            } else {
                None
            }
        };
        #[cfg(not(feature = "desktop-test"))]
        let queued: Option<PathBuf> = None;
        Some(match queued {
            Some(path) => path,
            None => rfd::FileDialog::new()
                .set_title("Import the reviewed resource file")
                .pick_file()
                .ok_or_else(|| err("native selection cancelled"))?,
        })
    } else {
        None
    };
    let stage = directory.join(format!(".incoming-{id}"));
    let outcome = (|| {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(|_| err("resource staging unavailable"))?;
        let progress = |bytes| {
            if let Ok(mut jobs) = app.state::<ResourceJobs>().jobs.lock() {
                if let Some(row) = jobs.get_mut(id) {
                    row.0.stage =
                        "Transferring; verifying pinned identity before publication".into();
                    row.0.transferred = bytes;
                }
            }
        };
        if let Some(path) = input {
            let mut file =
                std::fs::File::open(path).map_err(|_| err("selected resource unavailable"))?;
            if file.metadata().map_err(|_| err("selected metadata"))?.len() != resource.bytes {
                return Err(err("selected file size differs from reviewed artifact"));
            }
            let mut count = 0u64;
            let mut buffer = [0; 65536];
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return Err(err("cancelled before publication"));
                }
                let n = file
                    .read(&mut buffer)
                    .map_err(|_| err("selected resource read failed"))?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                if count > resource.bytes {
                    return Err(err("selected file changed"));
                }
                output
                    .write_all(&buffer[..n])
                    .map_err(|_| err("resource write failed; check free storage"))?;
                progress(count);
            }
        } else {
            kokin_net::resources::download(
                kokin_net::resources::PinnedResource {
                    url: &resource.url,
                    hosts: &[
                        "huggingface.co".into(),
                        "cdn-lfs.huggingface.co".into(),
                        "cas-bridge.xethub.hf.co".into(),
                        "us.aws.cdn.hf.co".into(),
                    ],
                    bytes: resource.bytes,
                    sha256: &resource.sha256,
                },
                &mut output,
                cancel,
                &progress,
                &kokin_net::Attribution {
                    connector: "resource_install".into(),
                    job_id: id.into(),
                    case_id: "global_resources".into(),
                },
            )
            .map_err(|e| err(e.to_string()))?;
        }
        output
            .flush()
            .and_then(|_| output.sync_all())
            .map_err(|_| err("resource flush failed"))?;
        drop(output);
        verify(&stage, resource.bytes, &resource.sha256, cancel)?;
        {
            let state = app.state::<ResourceJobs>();
            let mut jobs = state.jobs.lock().map_err(|_| err("resource registry"))?;
            if cancel.load(Ordering::SeqCst) {
                return Err(err("cancelled before publication"));
            }
            let row = jobs
                .get_mut(id)
                .ok_or_else(|| err("resource operation unavailable"))?;
            row.0.stage = "Publishing verified resource".into();
        }
        std::fs::hard_link(&stage, &destination).map_err(|_| {
            err("resource destination exists or publication failed; nothing was overwritten")
        })?;
        Ok(())
    })();
    if stage.exists() {
        std::fs::remove_file(&stage).map_err(|_| {
            err("incomplete resource staging cleanup failed; operation not reported complete")
        })?;
    }
    outcome
}

impl ResourceJobs {
    pub(crate) fn begin(
        &self,
        resource_id: &str,
        bytes: u64,
    ) -> Result<(String, Arc<AtomicBool>), CommandError> {
        let id = kokin_ai::persistence::id().map_err(|_| err("resource id"))?;
        let cancel = Arc::new(AtomicBool::new(false));
        let mut jobs = self.jobs.lock().map_err(|_| err("resource registry"))?;
        if jobs.values().any(|r| r.0.status == "running") {
            return Err(err("another global resource operation is running"));
        }
        jobs.insert(
            id.clone(),
            (
                ResourceJobView {
                    operation_id: id.clone(),
                    resource_id: resource_id.into(),
                    status: "running".into(),
                    stage: "Preparing global resource".into(),
                    transferred: 0,
                    bytes,
                    error: None,
                },
                cancel.clone(),
            ),
        );
        Ok((id, cancel))
    }
    pub(crate) fn progress(&self, id: &str, transferred: u64) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(row) = jobs.get_mut(id) {
                row.0.transferred = transferred;
                row.0.stage = "Copying and verifying local archive".into();
            }
        }
    }
    pub(crate) fn publish(&self, id: &str) -> Result<(), CommandError> {
        let mut jobs = self.jobs.lock().map_err(|_| err("resource registry"))?;
        let row = jobs.get_mut(id).ok_or_else(|| err("resource job"))?;
        if row.1.load(Ordering::SeqCst) {
            return Err(err("cancelled before resource publication"));
        }
        row.0.stage = "Publishing verified map archive".into();
        Ok(())
    }
    pub(crate) fn finish(&self, id: &str, result: Result<String, CommandError>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(row) = jobs.get_mut(id) {
                match result {
                    Ok(resource) => {
                        row.0.status = "completed".into();
                        row.0.resource_id = resource;
                    }
                    Err(e) => {
                        row.0.status = if row.1.load(Ordering::SeqCst) {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into();
                        row.0.error = Some(e.message);
                    }
                }
                row.0.stage = "Finished".into();
            }
        }
    }
}
#[tauri::command]
pub fn resource_jobs(app: AppHandle) -> Result<Vec<ResourceJobView>, CommandError> {
    Ok(app
        .state::<ResourceJobs>()
        .jobs
        .lock()
        .map_err(|_| err("resource registry"))?
        .values()
        .map(|r| r.0.clone())
        .collect())
}
