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
    kind: String,
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
pub(crate) fn random_id() -> Result<String, CommandError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| fault("random generator unavailable"))?;
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
fn policy(hosts: Vec<String>) -> kokin_net::EgressPolicy {
    kokin_net::EgressPolicy {
        allowed_hosts: hosts,
        max_response_bytes: 16 * 1024 * 1024,
        requests_per_second: 1.0,
        burst: 1,
    }
}
impl Operations {
    pub fn online_flag(&self) -> Arc<AtomicBool> {
        self.online.clone()
    }
    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }
    pub fn cancel_all(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.cancel.store(true, Ordering::SeqCst);
            }
        }
        #[cfg(feature = "desktop-test")]
        if let Ok(mut state) = self.test_state.lock() {
            state.selections.clear();
            state.ai_response = None;
        }
        self.online.store(false, Ordering::SeqCst);
        if let Ok(mut selections) = self.selections.lock() {
            selections.clear();
        }
    }
    /// Called only with paths from the native window drop event, never IPC paths.
    pub(crate) fn select_drop(
        &self,
        paths: &[PathBuf],
        generation: u64,
    ) -> Result<serde_json::Value, CommandError> {
        if paths.len() != 1 {
            return Err(refused("drop one local file at a time"));
        }
        let path = &paths[0];
        let metadata = std::fs::metadata(path).map_err(|_| refused("dropped file unavailable"))?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 * 1024 {
            return Err(refused("drop a regular file up to 64 MiB"));
        }
        let selection = self.select(path.clone(), "input", generation)?;
        Ok(
            serde_json::json!({"generation":generation,"selection":selection,"name":path.file_name().and_then(|v|v.to_str()).unwrap_or("Dropped file")}),
        )
    }
    pub(crate) fn select(
        &self,
        path: PathBuf,
        mode: &str,
        generation: u64,
    ) -> Result<String, CommandError> {
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
    pub(crate) fn consume(
        &self,
        token: &str,
        mode: &str,
        generation: u64,
    ) -> Result<PathBuf, CommandError> {
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
    pub(crate) fn begin_publish(&self, id: &str) -> Result<(), CommandError> {
        // Use the same lock as cancel_operation: cancellation and publication
        // have one atomic boundary, including a cancel arriving after assembly.
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| fault("operation registry unavailable"))?;
        let job = jobs
            .get_mut(id)
            .ok_or_else(|| refused("unknown operation"))?;
        if job.cancel.load(Ordering::SeqCst) {
            return Err(refused("operation cancelled before publication"));
        }
        job.view.stage = "Committing complete output".into();
        job.view.cancellable = false;
        Ok(())
    }
    pub(crate) fn update(&self, id: &str, stage: &str, cancellable: bool) {
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
            let mode = if matches!(purpose.as_str(), "import" | "package" | "research_import") {
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
            "research_import" => (
                rfd::FileDialog::new()
                    .set_title("Preview research selectors locally")
                    .add_filter("Text tables", &["csv", "tsv", "json", "txt", "md", "log"])
                    .pick_file(),
                "input",
            ),
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
            "research_csv" => (
                rfd::FileDialog::new()
                    .set_file_name("Research.csv")
                    .save_file(),
                "output",
            ),
            "research_json" => (
                rfd::FileDialog::new()
                    .set_file_name("Research.json")
                    .save_file(),
                "output",
            ),
            "report_markdown" => (
                rfd::FileDialog::new()
                    .set_file_name("Investigation.md")
                    .save_file(),
                "output",
            ),
            "report_html" => (
                rfd::FileDialog::new()
                    .set_file_name("Investigation.html")
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

// Network work shares kokin-net's four-request admission; native inference stays serial.
fn network_job(kind: &str) -> bool {
    matches!(
        kind,
        "research_run" | "source_collect" | "collect" | "web_discover"
    )
}
fn admit<'a>(kind: &str, running: impl Iterator<Item = &'a str>) -> Result<(), CommandError> {
    let active = running.collect::<Vec<_>>();
    if active.len() >= 8 || active.contains(&"restore") || (kind == "restore" && !active.is_empty())
    {
        return Err(refused(
            "operation capacity reached or restore requires an idle workspace",
        ));
    }
    if network_job(kind) {
        if active.iter().filter(|k| network_job(k)).count() >= 4 {
            return Err(refused(
                "four network jobs are already scheduled; wait or cancel one",
            ));
        }
    } else if active.iter().any(|k| !network_job(k)) {
        return Err(refused(
            "a native operation is running; network research may continue alongside it",
        ));
    }
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
        let running = jobs
            .values()
            .filter(|job| job.view.status == "running")
            .collect::<Vec<_>>();
        admit(&kind, running.iter().map(|job| job.kind.as_str()))?;
        // Bound in-memory history. The persistent audit remains in the case.
        if jobs.len() >= 100 {
            jobs.retain(|_, job| job.view.status == "running");
        }
        jobs.insert(
            id.clone(),
            Operation {
                kind: kind.clone(),
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
                        job.view.status =
                            if kind == "research_run" && result["status"] == "cancelled" {
                                "cancelled"
                            } else if matches!(
                                kind.as_str(),
                                "source_collect" | "face_analyze" | "research_run"
                            ) && result["complete"] == false
                            {
                                "partial"
                            } else {
                                "completed"
                            }
                            .into();
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
    if kind == "face_analyze" {
        return crate::faces::run(app, id, generation, decode(args)?, cancel);
    }
    if kind == "research_export" {
        return crate::research::export(app, id, generation, &args, cancel);
    }
    if kind == "research_run" {
        return crate::research::run(app, id, generation, args, cancel);
    }
    if kind == "face_regroup" {
        return crate::faces::regroup(app, id, generation, cancel);
    }
    if kind == "web_discover" {
        return crate::discovery::run(
            app,
            id,
            generation,
            decode(args)?,
            cancel,
            &operations.online,
        );
    }
    if kind == "ai_benchmark" {
        return crate::ai::benchmark(app, id, field(&args, "endpoint")?, generation, cancel);
    }
    if kind == "map_illustration" {
        operations.begin_publish(id)?;
        let request = decode::<crate::views::MapIllustrationRequest>(args)?;
        crate::maps::verify_illustration_archive(app, &request)?;
        return session.with_generation(generation, |open| {
            crate::maps::save_illustration(open, &request)
        });
    }
    if kind == "embedding_rebuild" {
        return crate::semantic::rebuild(app, id, generation, cancel);
    }
    if kind == "ai_generate" {
        return crate::ai::run(app, id, decode(args)?, generation, cancel);
    }
    if kind == "ai_model_import" {
        return crate::ai::import_model(app, &args, cancel.clone());
    }
    if matches!(kind, "graph" | "graph_explore") {
        operations.update(id, "Expanding relationships within limits", true);
        return session.with_generation(generation, |open| {
            let seeds = if kind == "graph_explore" {
                decode::<Vec<String>>(args["seeds"].clone())?
            } else {
                vec![field(&args, "seed")?.to_owned()]
            };
            let direction = args["direction"].as_str().unwrap_or("both");
            let kinds = if args["kinds"].is_null() {
                Vec::new()
            } else {
                decode::<Vec<String>>(args["kinds"].clone())?
            };
            if seeds.is_empty()
                || seeds.len() > 300
                || !matches!(direction, "both" | "incoming" | "outgoing")
                || kinds.len() > 32
                || kinds.iter().any(|k| k.len() > 128)
            {
                return Err(refused("invalid graph roots or filters"));
            }
            let number = |key: &str, default: usize| {
                args[key].as_u64().map(|v| v as usize).unwrap_or(default)
            };
            let result = kokin_graph::explore::expand_multi(
                &open.conn,
                &seeds,
                kokin_graph::explore::ExpansionOptions {
                    hops: number("hops", 2),
                    node_limit: number("node_limit", 40),
                    per_node: number("per_node", 16),
                    direction,
                    kinds: &kinds,
                },
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
                truncation_reasons: result.truncation_reasons,
            };
            serde_json::to_value(view).map_err(|_| fault("graph encoding failed"))
        });
    }
    if kind == "image_clipboard_import" {
        if cancel.load(Ordering::SeqCst) {
            return Err(refused("clipboard import cancelled"));
        }
        let request = decode::<crate::views::ClipboardImageRequest>(args)?;
        operations.begin_publish(id)?;
        return session.with_generation(generation, |open| {
            crate::image_query::import_clipboard(open, id, request)
        });
    }
    if kind == "image_prepare" {
        operations.update(id, "Preparing a local derived query image", true);
        return crate::image_query::run(
            app,
            id,
            generation,
            decode::<crate::views::ImagePrepareRequest>(args)?,
            cancel,
        );
    }
    if matches!(kind, "extract" | "media_extract" | "media_preview") {
        let request = if kind == "extract" {
            let request = decode::<crate::views::ExtractRequest>(args.clone())?;
            crate::views::MediaProcessRequest {
                version: 1,
                artifact_id: request.artifact_id,
                page: 0,
                ocr: false,
            }
        } else {
            decode::<crate::views::MediaProcessRequest>(args.clone())?
        };
        if request.version != 1 || (kind == "media_preview" && request.ocr) {
            return Err(refused("unsupported media request version or preview mode"));
        }
        let artifact = request.artifact_id.as_str();
        let media = session.with_generation(generation, |open| {
            open.conn
                .query_row(
                    "SELECT media_type FROM artifact WHERE id=?1",
                    [artifact],
                    |r| r.get::<_, String>(0),
                )
                .map_err(|_| refused("artifact is unavailable"))
        })?;
        if kind == "extract"
            && (media == "application/json"
                || media == "application/dns-json"
                || media.ends_with("+json"))
        {
            operations.begin_publish(id)?;
            return session.with_generation(generation,|open| {
                let blobs=kokin_blob::BlobStore::new(open.paths.blobs());
                let (run,count,complete)=kokin_extract::json::extract(&mut open.conn,&blobs,&open.cmk,artifact).map_err(|_|CommandError::new(ErrorCode::FileUnreadable,"JSON extraction failed; raw evidence remains preserved"))?;
                Ok(serde_json::json!({"artifact_id":artifact,"run_id":run,"observations":count,"complete":complete}))
            });
        }
        if crate::media::supported(&media) {
            operations.update(id, "Processing media in isolated worker", true);
            return crate::media::run(
                app,
                id,
                crate::media::MediaJob {
                    artifact,
                    page: request.page,
                    ocr: request.ocr,
                    preview_only: kind == "media_preview",
                    generation,
                },
                cancel,
            );
        }
        if kind != "extract" {
            return Err(refused(
                "unsupported media format; raw evidence is preserved",
            ));
        }
    }
    if kind == "source_collect" {
        let request = decode::<crate::views::SourceRequest>(args)?;
        if !request.consent || !operations.online.load(Ordering::SeqCst) {
            return Err(refused(
                "enable online mode and acknowledge the disclosed request first",
            ));
        }
        let plan = crate::sources::plan(&request)?;
        let case = session
            .view()?
            .case
            .ok_or_else(CommandError::no_case_open)?;
        let hosts = plan
            .manifest
            .destinations
            .iter()
            .map(|v| kokin_net::normalise(v).map(|u| u.host))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refused("invalid declared destination"))?;
        let mut policy = policy(hosts);
        policy.max_response_bytes = plan.manifest.max_response_bytes;
        let live = kokin_net::live::LiveHttp::new(
            policy,
            kokin_net::Attribution {
                connector: plan.manifest.id.clone(),
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
                if let Some(path) = operations
                    .test_state
                    .lock()
                    .map_err(|_| fault("test state unavailable"))?
                    .replay
                    .clone()
                {
                    Box::new(kokin_net::ReplayHttp::new(path))
                } else {
                    Box::new(live)
                }
            }
            #[cfg(not(feature = "desktop-test"))]
            {
                Box::new(live)
            }
        };
        let outgoing = plan
            .requests
            .first()
            .ok_or_else(|| refused("empty source plan"))?;
        operations.update(id, "Collecting disclosed public-source response", true);
        let response = http.send(outgoing).map_err(|e| {
            CommandError::new(
                ErrorCode::FileUnreadable,
                format!("source request refused or failed: {e}"),
            )
        })?;
        if cancel.load(Ordering::SeqCst) || !operations.online.load(Ordering::SeqCst) {
            return Err(refused("source collection cancelled before commit"));
        }
        operations.begin_publish(id)?;
        let completeness = if request.connector == "wayback_capture" {
            if response
                .headers
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("memento-datetime") && !v.is_empty())
            {
                "archived_static_html_only"
            } else {
                "unverified_archive_response"
            }
        } else {
            "structured_response_only"
        };
        return session.with_generation(generation,|open| {
            let blobs=kokin_blob::BlobStore::new(open.paths.blobs());
            let captured=kokin_ingest::ingest_response(&mut open.conn,&blobs,&open.cmk,outgoing,&response,&kokin_ingest::IngestContext{connector:&plan.manifest.id,job_id:id},completeness)?;
            let mut result=crate::views::SourceResultView{artifact_id:captured.artifact_id.clone(),size_bytes:captured.size_bytes,deduplicated:captured.deduplicated,from_fixture:captured.from_fixture,http_status:response.status,capture_completeness:completeness.into(),limitation:plan.limitation,complete:true,run_id:None,observations:0,processing_error:None};
            if completeness=="unverified_archive_response" {result.complete=false;result.processing_error=Some("Archived response lacks Memento metadata; raw bytes are preserved with unverified archive provenance.".into());}
            if !(200..300).contains(&response.status) {result.complete=false;result.processing_error=Some("Service error response preserved as evidence; extraction was not attempted.".into());}
            else if request.connector!="wayback_capture" {
                match kokin_extract::json::extract(&mut open.conn,&blobs,&open.cmk,&captured.artifact_id){
                    Ok((run,count,complete))=>{result.run_id=Some(run);result.observations=count;result.complete=complete;},
                    Err(_)=>{result.complete=false;result.processing_error=Some("Raw response saved, but JSON extraction failed; inspect evidence and the failed run.".into());}
                }
            }
            serde_json::to_value(result).map_err(|_|fault("source result encoding failed"))
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
    if kind == "import" {
        let path = operations.consume(field(&args, "selection")?, "input", generation)?;
        operations.update(
            id,
            "Encrypting selected file; cancellation remains available",
            true,
        );
        return session.with_generation(generation, |open| {
            let result = crate::write::ingest_file_controlled(
                open,
                &crate::views::IngestFileRequest {
                    path: path.to_string_lossy().into_owned(),
                },
                id,
                cancel,
                || operations.begin_publish(id).is_ok(),
            )?;
            serde_json::to_value(result).map_err(|_| fault("result encoding failed"))
        });
    }
    let input = if matches!(kind, "import" | "verify" | "restore") {
        Some(operations.consume(field(&args, "selection")?, "input", generation)?)
    } else {
        None
    };
    let output = if matches!(kind, "export" | "restore" | "report_export") {
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
    if kind == "report_preview" {
        operations.update(id, "Validating report citations", true);
        return session.with_generation(generation,|open|Ok(serde_json::json!({"report_id":field(&args,"report_id")?,"revision":args["revision"],"content":crate::reports::render(open,field(&args,"report_id")?,"markdown",args["revision"].as_i64().ok_or_else(||refused("missing report revision"))?,cancel)?})));
    }
    if kind == "report_export" {
        operations.update(id, "Assembling and validating report citations", true);
        let content = session.with_generation(generation, |open| {
            crate::reports::render(
                open,
                field(&args, "report_id")?,
                field(&args, "format")?,
                args["revision"]
                    .as_i64()
                    .ok_or_else(|| refused("missing report revision"))?,
                cancel,
            )
        })?;
        operations.begin_publish(id)?;
        let destination = output
            .as_ref()
            .ok_or_else(|| refused("missing destination"))?;
        let staging = destination.with_extension(format!("{}.staging", random_id()?));
        let publish=session.with_generation(generation,|_|{
            use std::io::Write;
            let result=(||->std::io::Result<()>{let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&staging)?;file.write_all(content.as_bytes())?;file.sync_all()?;std::fs::hard_link(&staging,destination)?;Ok(())})();
            let _=std::fs::remove_file(&staging);
            result.map_err(|_|CommandError::new(ErrorCode::Storage,"report export failed; destination must be new and have enough space"))?;
            Ok(serde_json::json!({"destination":destination,"bytes":content.len(),"plaintext":true}))
        });
        return publish;
    }
    session.with_generation(generation,|open| {
        match kind {
            "import" => serde_json::to_value(crate::write::ingest_file(open,&crate::views::IngestFileRequest {path:input.as_ref().ok_or_else(||refused("missing file"))?.to_string_lossy().into_owned()})?).map_err(|_|fault("result encoding failed")),
            "extract" => serde_json::to_value(crate::write::extract(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "create_entity" => serde_json::to_value(crate::write::create_entity(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "add_identifier" => serde_json::to_value(crate::write::add_identifier(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "relate" => serde_json::to_value(crate::write::relate(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
            "add_evidence"=>serde_json::to_value(crate::write::add_evidence(open,&decode(args)?)?).map_err(|_|fault("result encoding failed")),
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
    #[test]
    fn resource_admission_bounds_network_and_native_work() {
        assert!(admit(
            "research_run",
            ["face_analyze", "research_run", "source_collect"].into_iter()
        )
        .is_ok());
        assert!(admit("research_run", ["research_run"; 4].into_iter()).is_err());
        assert!(admit("face_analyze", ["embedding_rebuild"].into_iter()).is_err());
        assert!(admit("restore", ["research_run"].into_iter()).is_err());
        assert!(admit("research_run", ["restore"].into_iter()).is_err());
    }
    #[test]
    fn report_publication_rejects_a_cancel_after_assembly() {
        let ops = Operations::default();
        let cancel = Arc::new(AtomicBool::new(true));
        ops.jobs.lock().unwrap().insert(
            "report".into(),
            Operation {
                kind: "report_export".into(),
                view: OperationView {
                    operation_id: "report".into(),
                    status: "running".into(),
                    stage: "Assembling".into(),
                    cancellable: true,
                    result_json: None,
                    error: None,
                },
                cancel: cancel.clone(),
            },
        );
        assert!(ops.begin_publish("report").is_err());
        assert!(ops.jobs.lock().unwrap()["report"].view.cancellable);
        cancel.store(false, Ordering::SeqCst);
        ops.begin_publish("report").unwrap();
        assert!(!ops.jobs.lock().unwrap()["report"].view.cancellable);
    }
    fn directory() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kokin-selection-{}", random_id().unwrap()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }
    #[test]
    fn native_drop_is_bounded_single_use_and_case_bound() {
        let root = std::env::temp_dir().join(format!("kokin-drop-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("synthetic.csv");
        std::fs::write(&path, b"name\nsynthetic").unwrap();
        let operations = Operations::default();
        assert!(operations.select_drop(&[], 7).is_err());
        assert!(operations
            .select_drop(&[path.clone(), path.clone()], 7)
            .is_err());
        assert!(operations
            .select_drop(std::slice::from_ref(&root), 7)
            .is_err());
        let dropped = operations
            .select_drop(std::slice::from_ref(&path), 7)
            .unwrap();
        let token = dropped["selection"].as_str().unwrap();
        assert_eq!(dropped["name"], "synthetic.csv");
        assert!(operations.consume(token, "input", 8).is_err());
        let dropped = operations
            .select_drop(std::slice::from_ref(&path), 7)
            .unwrap();
        let token = dropped["selection"].as_str().unwrap();
        assert_eq!(
            operations.consume(token, "input", 7).unwrap(),
            std::fs::canonicalize(&path).unwrap()
        );
        assert!(operations.consume(token, "input", 7).is_err());
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(64 * 1024 * 1024 + 1)
            .unwrap();
        assert!(operations.select_drop(&[path], 7).is_err());
        std::fs::remove_dir_all(root).unwrap();
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
