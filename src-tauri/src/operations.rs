//! Case-bound native selections and background operations. Never trust a path
//! supplied by the webview. Network work finishes before the case lock is taken.
use crate::{error::ErrorCode, views::OperationView, CommandError, Session};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{Manager, State};

struct Selection {
    path: PathBuf,
    generation: u64,
    expires: Instant,
    mode: String,
    identity: Option<(u64, SystemTime)>,
}
struct Operation {
    view: OperationView,
    cancel: Arc<AtomicBool>,
}
pub struct Operations {
    #[cfg(feature = "desktop-test")]
    pub(crate) test_state: Mutex<crate::desktop_test::TestState>,
    selections: Mutex<HashMap<String, Selection>>,
    jobs: Mutex<HashMap<String, Operation>>,
    online: Arc<AtomicBool>,
    limiter: Arc<Mutex<kokin_net::RateLimiter>>,
}
impl Default for Operations {
    fn default() -> Self {
        let policy = policy(Vec::new());
        Self {
            #[cfg(feature = "desktop-test")]
            test_state: Mutex::new(Default::default()),
            selections: Mutex::new(HashMap::new()),
            jobs: Mutex::new(HashMap::new()),
            online: Arc::new(AtomicBool::new(false)),
            limiter: Arc::new(Mutex::new(kokin_net::RateLimiter::new(
                "http_fetch",
                &policy,
            ))),
        }
    }
}
fn fault(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::Internal, message)
}
fn refused(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, message)
}
fn random_id() -> Result<String, CommandError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| fault("random generator unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn policy(hosts: Vec<String>) -> kokin_net::EgressPolicy {
    kokin_net::EgressPolicy {
        allowed_hosts: hosts,
        max_response_bytes: 16 * 1024 * 1024,
        requests_per_second: 1.0,
        burst: 1,
    }
}
impl Operations {
    pub fn cancel_all(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.cancel.store(true, Ordering::SeqCst);
            }
        }
        #[cfg(feature = "desktop-test")]
        if let Ok(mut state) = self.test_state.lock() {
            state.selections.clear();
        }
        self.online.store(false, Ordering::SeqCst);
        if let Ok(mut selections) = self.selections.lock() {
            selections.clear();
        }
    }
    fn select(&self, path: PathBuf, mode: &str, generation: u64) -> Result<String, CommandError> {
        let path = if mode == "input" {
            std::fs::canonicalize(path).map_err(|_| refused("selected file is unavailable"))?
        } else {
            path
        };
        let identity = if mode == "input" {
            let meta =
                std::fs::metadata(&path).map_err(|_| refused("selected file is unavailable"))?;
            if !meta.is_file() {
                return Err(refused("choose a regular file"));
            }
            Some((
                meta.len(),
                meta.modified()
                    .map_err(|_| refused("file metadata unavailable"))?,
            ))
        } else {
            if path.exists() {
                return Err(refused("destination already exists; choose a new name"));
            }
            None
        };
        let token = random_id()?;
        let mut selections = self
            .selections
            .lock()
            .map_err(|_| fault("selection registry unavailable"))?;
        selections.retain(|_, s| s.expires > Instant::now() && s.generation == generation);
        if selections.len() >= 32 {
            return Err(refused("too many pending selections"));
        }
        selections.insert(
            token.clone(),
            Selection {
                path,
                mode: mode.into(),
                generation,
                identity,
                expires: Instant::now() + Duration::from_secs(300),
            },
        );
        Ok(token)
    }
    fn consume(&self, token: &str, mode: &str, generation: u64) -> Result<PathBuf, CommandError> {
        let selected = self
            .selections
            .lock()
            .map_err(|_| fault("selection registry unavailable"))?
            .remove(token)
            .ok_or_else(|| refused("selection expired; choose the file again"))?;
        if selected.generation != generation
            || selected.expires < Instant::now()
            || selected.mode != mode
        {
            return Err(refused("selection expired or belongs to another case"));
        }
        if let Some(identity) = selected.identity {
            let metadata = std::fs::metadata(&selected.path)
                .map_err(|_| refused("selected file disappeared"))?;
            if !metadata.is_file()
                || (
                    metadata.len(),
                    metadata
                        .modified()
                        .map_err(|_| refused("file metadata unavailable"))?,
                ) != identity
            {
                return Err(refused("selected file changed; choose it again"));
            }
        } else if selected.path.exists() {
            return Err(refused("destination now exists; choose a new name"));
        }
        Ok(selected.path)
    }
    fn update(&self, id: &str, stage: &str, cancellable: bool) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get_mut(id) {
                job.view.stage = stage.into();
                job.view.cancellable = cancellable;
            }
        }
    }
}

#[tauri::command]
pub async fn select_path(
    app: tauri::AppHandle,
    purpose: String,
    expected_generation: u64,
) -> Result<Option<String>, CommandError> {
    let generation = app.state::<Session>().generation();
    if generation != expected_generation {
        return Err(refused("originating case is no longer open"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(feature = "desktop-test")]
        if let Some((queued_purpose, path)) = app
            .state::<Operations>()
            .test_state
            .lock()
            .map_err(|_| fault("test queue unavailable"))?
            .selections
            .pop_front()
        {
            if queued_purpose != purpose {
                return Err(refused("unexpected test selection purpose"));
            }
            let mode = if matches!(purpose.as_str(), "import" | "package") {
                "input"
            } else {
                "output"
            };
            return app
                .state::<Operations>()
                .select(path, mode, generation)
                .map(Some);
        }
        let (path, mode) = match purpose.as_str() {
            "import" => (
                rfd::FileDialog::new()
                    .set_title("Import evidence")
                    .pick_file(),
                "input",
            ),
            "package" => (
                rfd::FileDialog::new()
                    .add_filter("Encrypted case package", &["kokinpkg"])
                    .pick_file(),
                "input",
            ),
            "export" => (
                rfd::FileDialog::new()
                    .set_file_name("Case.kokinpkg")
                    .save_file(),
                "output",
            ),
            "restore" => (
                rfd::FileDialog::new()
                    .set_file_name("Restored.kokincase")
                    .save_file(),
                "output",
            ),
            _ => return Err(refused("unknown selection purpose")),
        };
        if app.state::<Session>().generation() != generation {
            return Err(refused("case changed while the dialog was open"));
        }
        path.map(|p| app.state::<Operations>().select(p, mode, generation))
            .transpose()
    })
    .await
    .map_err(|_| fault("file dialog failed"))?
}
#[tauri::command]
pub fn network_mode(operations: State<'_, Operations>) -> bool {
    operations.online.load(Ordering::SeqCst)
}
#[tauri::command]
pub fn set_network_mode(
    operations: State<'_, Operations>,
    online: bool,
) -> Result<bool, CommandError> {
    #[cfg(feature = "desktop-test")]
    let fixture_mode = operations
        .test_state
        .lock()
        .map_err(|_| fault("test state unavailable"))?
        .replay
        .is_some();
    #[cfg(not(feature = "desktop-test"))]
    let fixture_mode = false;
    if online && !fixture_mode && std::env::var("KOKIN_NETWORK").as_deref() == Ok("deny") {
        return Err(refused("network disabled by process policy"));
    }
    operations.online.store(online, Ordering::SeqCst);
    Ok(online)
}
#[tauri::command]
pub fn operation_status(
    operations: State<'_, Operations>,
    operation_id: String,
) -> Result<OperationView, CommandError> {
    operations
        .jobs
        .lock()
        .map_err(|_| fault("operation registry unavailable"))?
        .get(&operation_id)
        .map(|j| j.view.clone())
        .ok_or_else(|| refused("unknown operation"))
}
#[tauri::command]
pub fn cancel_operation(
    operations: State<'_, Operations>,
    operation_id: String,
) -> Result<(), CommandError> {
    let jobs = operations
        .jobs
        .lock()
        .map_err(|_| fault("operation registry unavailable"))?;
    let job = jobs
        .get(&operation_id)
        .ok_or_else(|| refused("unknown operation"))?;
    if !job.view.cancellable {
        return Err(refused("this operation is committing; wait for its result"));
    }
    job.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

/// Args are command-layer requests. Actor identities never come from the caller.
#[tauri::command]
pub fn start_operation(
    app: tauri::AppHandle,
    kind: String,
    args: serde_json::Value,
    expected_generation: u64,
) -> Result<String, CommandError> {
    let generation = app.state::<Session>().generation();
    if generation != expected_generation {
        return Err(refused("originating case is no longer open"));
    }
    let operations = app.state::<Operations>();
    let id = random_id()?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = operations
            .jobs
            .lock()
            .map_err(|_| fault("operation registry unavailable"))?;
        if jobs.values().any(|job| job.view.status == "running") {
            return Err(refused(
                "an operation is already running; wait or cancel it",
            ));
        }
        // Bound in-memory history. The persistent audit remains in the case.
        if jobs.len() >= 100 {
            jobs.retain(|_, job| job.view.status == "running");
        }
        jobs.insert(
            id.clone(),
            Operation {
                view: OperationView {
                    operation_id: id.clone(),
                    status: "running".into(),
                    stage: "Starting".into(),
                    cancellable: true,
                    result_json: None,
                    error: None,
                },
                cancel: cancel.clone(),
            },
        );
    }
    let job_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(&app, &job_id, &kind, args, generation, &cancel)
        }));
        let result = outcome.unwrap_or_else(|_| {
            Err(fault(
                "operation failed unexpectedly; inspect the case before retrying",
            ))
        });
        let operations = app.state::<Operations>();
        if let Ok(mut jobs) = operations.jobs.lock() {
            if let Some(job) = jobs.get_mut(&job_id) {
                job.view.cancellable = false;
                job.view.stage = "Finished".into();
                match result {
                    Ok(result) => {
                        job.view.status = "completed".into();
                        job.view.result_json = Some(result.to_string());
                    }
                    Err(error) => {
                        job.view.status = if cancel.load(Ordering::SeqCst) {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into();
                        job.view.error = Some(error);
                    }
                }
            }
        };
    });
    Ok(id)
}
fn field<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, CommandError> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| refused(format!("missing {key}")))
}
fn decode<T: serde::de::DeserializeOwned>(args: serde_json::Value) -> Result<T, CommandError> {
    serde_json::from_value(args).map_err(|_| refused("invalid operation request"))
}
fn run(
    app: &tauri::AppHandle,
    id: &str,
    kind: &str,
    args: serde_json::Value,
    generation: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    let session = app.state::<Session>();
    let operations = app.state::<Operations>();
    if cancel.load(Ordering::SeqCst) || session.generation() != generation {
        return Err(refused("operation cancelled or originating case closed"));
    }
    if kind == "graph" {
        operations.update(id, "Expanding relationships within limits", true);
        return session.with_generation(generation, |open| {
            let seed = field(&args, "seed")?;
            let number = |key: &str, default: usize| {
                args[key].as_u64().map(|v| v as usize).unwrap_or(default)
            };
            let result = kokin_graph::explore::expand(
                &open.conn,
                seed,
                number("hops", 2),
                number("node_limit", 100),
                number("per_node", 32),
                cancel,
            )?;
            let view = crate::views::GraphView {
                nodes: result
                    .nodes
                    .into_iter()
                    .map(|n| crate::views::GraphNodeView {
                        id: n.id,
                        name: n.name,
                        canonical_id: n.canonical_id,
                        depth: n.depth,
                    })
                    .collect(),
                edges: result
                    .edges
                    .into_iter()
                    .map(|e| crate::views::GraphEdgeView {
                        id: e.id,
                        from: e.from,
                        to: e.to,
                        kind: e.kind,
                    })
                    .collect(),
                truncated: result.truncated,
                completed_hops: result.completed_hops,
            };
            serde_json::to_value(view).map_err(|_| fault("graph encoding failed"))
        });
    }
    if kind == "collect" {
        let url = field(&args, "url")?;
        let normalized = kokin_net::normalise(url).map_err(|_| refused("invalid public URL"))?;
        let case = session
            .view()?
            .case
            .ok_or_else(CommandError::no_case_open)?;
        let live_http = kokin_net::live::LiveHttp::new(
            policy(vec![normalized.host.clone()]),
            kokin_net::Attribution {
                connector: "http_fetch".into(),
                job_id: id.into(),
                case_id: case.case_id,
            },
            operations.online.clone(),
            cancel.clone(),
            operations.limiter.clone(),
        );
        let http: Box<dyn kokin_net::HttpCapability> = {
            #[cfg(feature = "desktop-test")]
            {
                let fixture = operations
                    .test_state
                    .lock()
                    .map_err(|_| fault("test state unavailable"))?
                    .replay
                    .clone();
                if let Some(path) = fixture {
                    Box::new(kokin_net::ReplayHttp::new(path))
                } else {
                    Box::new(live_http)
                }
            }
            #[cfg(not(feature = "desktop-test"))]
            {
                Box::new(live_http)
            }
        };
        operations.update(id, "Collecting static page", true);
        let response=http.send(&kokin_net::HttpRequest::get(url)).map_err(|_|CommandError::new(ErrorCode::FileUnreadable,"collection refused, timed out, or exceeded its limits; check offline mode and the public destination"))?;
        if cancel.load(Ordering::SeqCst) || !operations.online.load(Ordering::SeqCst) {
            return Err(refused("collection cancelled before commit"));
        }
        operations.update(id, "Saving verified capture", false);
        struct Captured(kokin_net::HttpResponse);
        impl kokin_net::HttpCapability for Captured {
            fn send(
                &self,
                _: &kokin_net::HttpRequest,
            ) -> Result<kokin_net::HttpResponse, kokin_net::HttpError> {
                Ok(self.0.clone())
            }
        }
        let from_fixture = response.from_fixture;
        return session.with_generation(generation,|open| {
            let blobs=kokin_blob::BlobStore::new(open.paths.blobs());
            let result=kokin_ingest::ingest_url(&mut open.conn,&blobs,&Captured(response),&open.cmk,url,&kokin_ingest::IngestContext {connector:"http_fetch",job_id:id})?;
            Ok(serde_json::json!({"artifact_id":result.artifact_id,"size_bytes":result.size_bytes,"deduplicated":result.deduplicated,"capture_completeness":"static_html_only","from_fixture":from_fixture}))
        });
    }
    let input = if matches!(kind, "import" | "verify" | "restore") {
        Some(operations.consume(field(&args, "selection")?, "input", generation)?)
    } else {
        None
    };
    let output = if matches!(kind, "export" | "restore") {
        Some(operations.consume(field(&args, "destination")?, "output", generation)?)
    } else {
        None
    };
    if cancel.load(Ordering::SeqCst) {
        return Err(refused("operation cancelled before commit"));
    }
    // Once an atomic action starts, cancellation is no longer advertised. A
    // completed write is reported as completed, never retroactively cancelled.
    operations.update(id, "Committing operation; writes are serialized", false);
    if kind == "verify" {
        let report =
            kokin_export::verify(input.as_deref().ok_or_else(|| refused("missing package"))?)
                .map_err(|_| {
                    CommandError::new(ErrorCode::Storage, "package verification failed")
                })?;
        return Ok(
            serde_json::json!({"verdict":format!("{:?}",report.verdict),"case_id":report.case_id,"omissions":report.omissions,"problems":report.problems}),
        );
    }
    if kind == "restore" {
        let source = input.as_deref().ok_or_else(|| refused("missing package"))?;
        let destination = output
            .as_deref()
            .ok_or_else(|| refused("missing destination"))?;
        let report = kokin_export::import(source, destination, field(&args, "passphrase")?)
            .map_err(|_| {
                CommandError::new(
                    ErrorCode::Storage,
                    "restore failed; the package or passphrase may be invalid",
                )
            })?;
        return Ok(
            serde_json::json!({"root":report.root,"case_id":report.case_id,"omissions":report.omissions}),
        );
    }
    session.with_generation(generation,|open| {
        match kind {
            "import" => serde_json::to_value(crate::write::ingest_file(open,&crate::views::IngestFileRequest {path:input.as_ref().ok_or_else(||refused("missing file"))?.to_string_lossy().into_owned()})?).map_err(|_|fault("result encoding failed")),
            "extract" => serde_json::to_value(crate::write::extract(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "create_entity" => serde_json::to_value(crate::write::create_entity(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "add_identifier" => serde_json::to_value(crate::write::add_identifier(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "relate" => serde_json::to_value(crate::write::relate(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "assess" => serde_json::to_value(crate::write::assess(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "merge" => serde_json::to_value(crate::write::merge(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "reject" => serde_json::to_value(crate::write::reject(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "split" => serde_json::to_value(crate::write::split(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "export" => {
                let dest=output.as_ref().ok_or_else(||refused("missing destination"))?;
                let mut options=kokin_export::PackageOptions::default();
                if let Some(exclude)=args.get("exclude").and_then(|v|v.as_array()){for item in exclude {options.exclude.insert(item.as_str().ok_or_else(||refused("invalid exclusion"))?.into());}}
                let report=kokin_export::package(open,dest,field(&args,"passphrase")?,&options).map_err(|_|CommandError::new(ErrorCode::Storage,"export failed; check available space and destination permissions"))?;
                Ok(serde_json::json!({"case_id":report.case_id,"bytes":report.bytes,"omissions":report.omissions,"destination":dest}))
            },
            _=>Err(refused("unknown operation")),
        }
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    fn directory() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kokin-selection-{}", random_id().unwrap()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }
    #[test]
    fn selections_are_single_use_case_bound_and_expiring() {
        let dir = directory();
        let file = dir.join("input.html");
        std::fs::write(&file, "original").unwrap();
        let ops = Operations::default();
        let token = ops.select(file.clone(), "input", 7).unwrap();
        assert!(ops.consume(&token, "input", 8).is_err());
        assert!(ops.consume(&token, "input", 7).is_err());
        let token = ops.select(file.clone(), "input", 7).unwrap();
        assert_eq!(
            ops.consume(&token, "input", 7).unwrap(),
            std::fs::canonicalize(&file).unwrap()
        );
        assert!(ops.consume(&token, "input", 7).is_err());
        let token = ops.select(file.clone(), "input", 7).unwrap();
        ops.selections
            .lock()
            .unwrap()
            .get_mut(&token)
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        assert!(ops.consume(&token, "input", 7).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn changed_files_and_existing_destinations_are_refused() {
        let dir = directory();
        let file = dir.join("input.html");
        std::fs::write(&file, "original").unwrap();
        let ops = Operations::default();
        let token = ops.select(file.clone(), "input", 1).unwrap();
        std::fs::write(&file, "different-length").unwrap();
        assert!(ops.consume(&token, "input", 1).is_err());
        assert!(ops.select(file.clone(), "output", 1).is_err());
        let target = dir.join("output.kokinpkg");
        let token = ops.select(target.clone(), "output", 1).unwrap();
        std::fs::write(&target, "keep this").unwrap();
        assert!(ops.consume(&token, "output", 1).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep this");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn closing_operations_invalidates_selections_and_disables_network() {
        let dir = directory();
        let path = dir.join("output");
        let ops = Operations::default();
        ops.online.store(true, Ordering::SeqCst);
        let token = ops.select(path, "output", 1).unwrap();
        ops.cancel_all();
        assert!(!ops.online.load(Ordering::SeqCst));
        assert!(ops.consume(&token, "output", 1).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
