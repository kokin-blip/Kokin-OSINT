//! Optional inference: snapshots and commits are case-bound, socket waits are not.
use crate::{views::*, CommandError, ErrorCode, Session};
use kokin_net::local::{Call, InferenceCapability, LoopbackInference};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Manager};
fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, message)
}
fn client(endpoint: &str) -> Result<LoopbackInference, CommandError> {
    LoopbackInference::new(endpoint).map_err(|_| {
        invalid("use a literal loopback HTTP endpoint, such as http://127.0.0.1:11434/")
    })
}
fn inference(
    app: &AppHandle,
    endpoint: &str,
) -> Result<Box<dyn InferenceCapability>, CommandError> {
    #[cfg(feature = "desktop-test")]
    if let Some(response) = app
        .state::<crate::operations::Operations>()
        .test_state
        .lock()
        .map_err(|_| invalid("test state"))?
        .ai_response
        .clone()
    {
        struct Replay(serde_json::Value);
        impl InferenceCapability for Replay {
            fn send(
                &self,
                call: Call,
                cancel: &AtomicBool,
            ) -> Result<serde_json::Value, kokin_net::local::Error> {
                if cancel.load(Ordering::SeqCst) {
                    return Err(kokin_net::local::Error::Cancelled);
                }
                let c = kokin_ai::registry()
                    .map_err(|_| kokin_net::local::Error::Refused)?
                    .candidates
                    .remove(0);
                Ok(match call {
                    Call::Version => {
                        serde_json::json!({"version":"synthetic replay fixture","from_fixture":true})
                    }
                    Call::Tags => {
                        serde_json::json!({"models":[{"name":format!("kokin-{}",c.id),"digest":c.sha256,"size":c.bytes}],"from_fixture":true})
                    }
                    Call::Show { .. } => {
                        serde_json::json!({"modelfile":format!("FROM /synthetic-fixture/sha256-{}",c.sha256),"from_fixture":true})
                    }
                    Call::Running => serde_json::json!({"models":[],"from_fixture":true}),
                    Call::Generate { .. } => {
                        let mut r = self.0.clone();
                        r["from_fixture"] = true.into();
                        r
                    }
                })
            }
        }
        return Ok(Box::new(Replay(response)));
    }
    let _ = app;
    Ok(Box::new(client(endpoint)?))
}
fn managed(name: &str) -> Result<kokin_ai::Candidate, CommandError> {
    kokin_ai::registry()
        .map_err(|_| invalid("registry unavailable"))?
        .candidates
        .into_iter()
        .find(|c| format!("kokin-{}", c.id) == name)
        .ok_or_else(|| {
            invalid("select a reviewed Kokin model imported from a pinned local artifact")
        })
}
fn verified(show: &serde_json::Value, sha: &str) -> bool {
    show.get("remote_host").is_none()
        && show.get("remote_model").is_none()
        && show["modelfile"].as_str().is_some_and(|text| {
            text.lines()
                .filter_map(|l| l.strip_prefix("FROM "))
                .any(|from| {
                    from.trim()
                        .trim_matches('"')
                        .ends_with(&format!("/sha256-{sha}"))
                })
        })
}
#[tauri::command]
pub async fn ai_status(app: AppHandle, endpoint: String) -> Result<AiStatusView, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = crate::resources::root(&app)?;
        std::fs::create_dir_all(&directory).map_err(|_| invalid("resource folder unavailable"))?;
        let hardware = kokin_ai::hardware::detect(&directory);
        let http = inference(&app, &endpoint)?;
        let cancel = AtomicBool::new(false);
        let tags = http.send(Call::Tags, &cancel);
        let mut models = Vec::new();
        let status = if let Ok(tags) = tags {
            for row in tags["models"].as_array().into_iter().flatten().take(64) {
                let name = row["name"]
                    .as_str()
                    .unwrap_or_default()
                    .trim_end_matches(":latest")
                    .to_owned();
                let reviewed = if let Ok(c) = managed(&name) {
                    http.send(
                        Call::Show {
                            model: name.clone(),
                        },
                        &cancel,
                    )
                    .is_ok_and(|show| verified(&show, &c.sha256))
                } else {
                    false
                };
                models.push(AiModelView {
                    name,
                    digest: row["digest"].as_str().unwrap_or_default().into(),
                    bytes: row["size"].as_u64().unwrap_or(0),
                    reviewed_artifact: reviewed,
                });
            }
            if tags["from_fixture"] == true {
                "available — synthetic replay fixture"
            } else {
                "available"
            }
        } else {
            "unavailable — install/start Ollama separately; other workspaces remain usable"
        };
        let loaded = http
            .send(Call::Running, &cancel)
            .unwrap_or(serde_json::Value::Null);
        let mut registry = kokin_ai::registry().map_err(|_| invalid("model registry"))?;
        let cached = benchmark_path(&app)
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        for c in &mut registry.candidates {
            if !models
                .iter()
                .any(|m| m.name == format!("kokin-{}", c.id) && m.reviewed_artifact)
            {
                continue;
            }
            if let Some(row) = cached.as_ref().map(|v| &v["measurements"][&c.id]) {
                if row["artifact_sha256"].as_str() == Some(c.sha256.as_str())
                    && models.iter().any(|m| {
                        m.name == format!("kokin-{}", c.id)
                            && row["ollama_digest"].as_str() == Some(m.digest.as_str())
                    })
                {
                    c.benchmark = serde_json::from_value(row.clone()).ok();
                }
            }
        }
        let runtime_version = http
            .send(Call::Version, &cancel)
            .unwrap_or(serde_json::Value::Null);
        let hw = hardware_identity(
            &hardware,
            &endpoint,
            runtime_version["version"].as_str().unwrap_or("unknown"),
        );
        let choice = |name| {
            kokin_ai::profile(
                &registry,
                &hw,
                hardware.available_memory_bytes.unwrap_or(0),
                "cited_summary",
                name,
            )
            .map(|c| format!("kokin-{}", c.id))
        };
        let recommendation = choice("balanced");
        let faster = choice("faster");
        let higher_quality = choice("higher_quality");
        Ok(AiStatusView {
            architecture: hardware.architecture,
            cpu_threads: hardware.cpu_threads,
            total_memory_bytes: hardware.total_memory_bytes,
            available_memory_bytes: hardware.available_memory_bytes,
            free_storage_bytes: hardware.free_storage_bytes,
            memory_kind: hardware.memory_kind,
            accelerator_status: hardware.accelerator_status,
            ollama_status: status.into(),
            models,
            loaded_models: serde_json::json!({"runtime":runtime_version,"loaded":loaded}),
            recommendation,
            faster,
            higher_quality,
        })
    })
    .await
    .map_err(|_| invalid("local runtime status failed"))?
}
#[tauri::command]
pub async fn ai_page(
    app: AppHandle,
    generation: u64,
    offset: usize,
) -> Result<AiPageView, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>().with_generation(generation, |open| {
            let (rows, total) = kokin_ai::persistence::list(&open.conn, offset)
                .map_err(|e| invalid(e.to_string()))?;
            Ok(AiPageView {
                rows: rows
                    .into_iter()
                    .map(|r| AiSuggestionView {
                        id: r.id,
                        model: r.model,
                        model_digest: r.model_digest,
                        kind: r.suggestion.kind,
                        text: r.suggestion.text,
                        cited_observation_ids: r.suggestion.cited_observation_ids,
                        proposed_name: r.suggestion.proposed_name,
                        proposed_type: r.suggestion.proposed_type,
                        relationship_kind: r.suggestion.relationship_kind,
                        created_utc: r.created_utc,
                        review: r.review,
                        review_rationale: r.review_rationale,
                        review_actor: r.review_actor,
                        review_utc: r.review_utc,
                    })
                    .collect(),
                total,
                offset,
                limit: 50,
            })
        })
    })
    .await
    .map_err(|_| invalid("AI history read failed"))?
}
#[tauri::command]
pub async fn ai_review(
    app: AppHandle,
    generation: u64,
    request: AiReviewRequest,
) -> Result<(), CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>().with_generation(generation, |open| {
            kokin_ai::persistence::review(
                &mut open.conn,
                &request.id,
                &request.decision,
                &request.rationale,
            )
            .map_err(|e| invalid(e.to_string()))
        })
    })
    .await
    .map_err(|_| invalid("review failed"))?
}
pub fn run(
    app: &AppHandle,
    operation: &str,
    request: AiRequest,
    generation: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    if request.version != 1 || !request.consent {
        return Err(invalid("review the local inference disclosure and opt in"));
    }
    let c = managed(&request.model)?;
    let session = app.state::<Session>();
    let context = session.with_generation(generation, |open| {
        verified_context(open, &request.observation_ids)
    })?;
    let (system, prompt) =
        kokin_ai::prompt(&context, &request.task).map_err(|e| invalid(e.to_string()))?;
    let http = inference(app, &request.endpoint)?;
    let show = http
        .send(
            Call::Show {
                model: request.model.clone(),
            },
            cancel,
        )
        .map_err(|_| invalid("Ollama/model unavailable; use setup/import controls"))?;
    if !verified(&show, &c.sha256) {
        return Err(invalid(
            "model does not match a reviewed local GGUF artifact; cloud models are not permitted",
        ));
    }
    let response = http
        .send(
            Call::Generate {
                model: request.model.clone(),
                system: system.clone(),
                prompt: prompt.clone(),
                tokens: 512,
                benchmark: false,
            },
            cancel,
        )
        .map_err(|_| {
            invalid("local inference failed or was cancelled; no suggestion was recorded")
        })?;
    let suggestions = kokin_ai::validate(&response, &context, &request.task)
        .map_err(|e| invalid(e.to_string()))?;
    if cancel.load(Ordering::SeqCst) {
        return Err(invalid("inference cancelled before recording suggestions"));
    }
    app.state::<crate::operations::Operations>()
        .begin_publish(operation)?;
    session.with_generation(generation,|open|{verified_context(open,&request.observation_ids)?;let ids=kokin_ai::persistence::save(&mut open.conn,&if response["from_fixture"]==true {format!("fixture:{}",request.model)} else {request.model.clone()},&c.sha256,&kokin_store::blake3_hex(format!("{system}{prompt}").as_bytes()),&kokin_store::blake3_hex(prompt.as_bytes()),&suggestions).map_err(|e|invalid(e.to_string()))?;Ok(serde_json::json!({"suggestion_ids":ids,"human_review_required":true,"model":request.model}))})
}
pub fn verified_context(
    open: &kokin_store::OpenCase,
    ids: &[String],
) -> Result<Vec<kokin_ai::Context>, CommandError> {
    if ids.is_empty() || ids.len() > 24 {
        return Err(invalid("choose 1–24 case observations"));
    }
    ids.iter().map(|id|{let view=crate::read::observation(open,id)?;if !matches!(view.quote,QuoteView::Exact{..}|QuoteView::Contains{..}|QuoteView::DerivedExact{..}|QuoteView::StructuredExact{..}){return Err(invalid("chosen observation cannot be verified against its source; inspect its citation first"));}Ok(kokin_ai::Context{observation_id:id.clone(),value:view.value})}).collect()
}
pub fn import_model(
    app: &AppHandle,
    args: &serde_json::Value,
    cancel: Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    let name = args["model"]
        .as_str()
        .ok_or_else(|| invalid("model required"))?;
    let endpoint = args["endpoint"]
        .as_str()
        .ok_or_else(|| invalid("endpoint required"))?;
    if args["consent"] != true {
        return Err(invalid("explicit local upload/import consent required"));
    }
    let c = managed(name)?;
    let path = crate::resources::root(app)?.join(&c.file);
    crate::resources::verify(&path, c.bytes, &c.sha256, &cancel)?;
    client(endpoint)?
        .import_file(name, &path, &c.sha256, c.bytes, cancel)
        .map_err(|_| {
            invalid("local model import failed; verified resource file remains available")
        })?;
    Ok(serde_json::json!({"model":name,"imported_local_artifact":true}))
}

fn hardware_identity(h: &kokin_ai::hardware::Hardware, endpoint: &str, runtime: &str) -> String {
    kokin_store::blake3_hex(
        format!(
            "{}:{}:{}:{}:{endpoint}:{runtime}",
            h.architecture,
            h.cpu_threads,
            h.total_memory_bytes.unwrap_or(0),
            h.accelerator_status
        )
        .as_bytes(),
    )
}
fn benchmark_path(app: &AppHandle) -> Result<std::path::PathBuf, CommandError> {
    Ok(crate::resources::root(app)?.join("model-benchmarks.json"))
}
pub fn benchmark(
    app: &AppHandle,
    operation: &str,
    endpoint: &str,
    generation: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    let h = kokin_ai::hardware::detect(&crate::resources::root(app)?);
    let mut registry = kokin_ai::registry().map_err(|_| invalid("registry"))?;
    let http = inference(app, endpoint)?;
    let version = http
        .send(Call::Version, cancel)
        .map_err(|_| invalid("runtime version unavailable; no recommendation published"))?;
    let hardware = hardware_identity(
        &h,
        endpoint,
        version["version"]
            .as_str()
            .ok_or_else(|| invalid("runtime version unavailable"))?,
    );

    let context=vec![kokin_ai::Context{observation_id:"synthetic-1".into(),value:"A synthetic employment record describes Mercer working at Tungsten Holdings. It provides no independent authenticity proof.".into()}];
    let (system, prompt) =
        kokin_ai::prompt(&context, "cited_summary").map_err(|e| invalid(e.to_string()))?;
    let tags = http
        .send(Call::Tags, cancel)
        .map_err(|_| invalid("model inventory unavailable"))?;
    let mut measurements = serde_json::Map::new();
    for c in &mut registry.candidates {
        let name = format!("kokin-{}", c.id);
        let show = match http.send(
            Call::Show {
                model: name.clone(),
            },
            cancel,
        ) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if show["from_fixture"] == true {
            return Err(invalid(
                "synthetic transport replay cannot establish a model performance recommendation",
            ));
        }
        if !verified(&show, &c.sha256) {
            continue;
        }
        let digest = tags["models"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| {
                row["name"]
                    .as_str()
                    .is_some_and(|v| v.trim_end_matches(":latest") == name)
            })
            .and_then(|row| row["digest"].as_str())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid("model identity unavailable"))?;

        if h.total_memory_bytes
            .is_none_or(|m| c.bytes + 2 * 1024 * 1024 * 1024 + registry.system_reserve_bytes > m)
        {
            continue;
        }
        let mut times = Vec::new();
        let mut correct = 0;
        let mut peak = c.bytes;
        for sample in 0..20 {
            app.state::<crate::operations::Operations>().update(
                operation,
                &format!("Benchmarking {}: {} / 20 synthetic tasks", c.id, sample + 1),
                true,
            );
            if cancel.load(Ordering::SeqCst) || app.state::<Session>().generation() != generation {
                return Err(invalid("model benchmark cancelled"));
            }
            let start = std::time::Instant::now();
            let output = http
                .send(
                    Call::Generate {
                        model: name.clone(),
                        system: system.clone(),
                        prompt: prompt.clone(),
                        tokens: 256,
                        benchmark: true,
                    },
                    cancel,
                )
                .map_err(|_| {
                    invalid("model resource/inference failure; no recommendation published")
                })?;
            times.push(start.elapsed().as_millis() as u64);
            if let Ok(s) = kokin_ai::validate(&output, &context, "cited_summary") {
                if s.iter().any(|s| {
                    s.text.to_lowercase().contains("mercer")
                        && s.text.to_lowercase().contains("tungsten")
                        && (!s.text.to_lowercase().contains("verified")
                            || ["not verified", "not independently verified", "unverified"]
                                .iter()
                                .any(|term| s.text.to_lowercase().contains(term)))
                }) {
                    correct += 1;
                }
            }
            if let Ok(ps) = http.send(Call::Running, cancel) {
                for model in ps["models"].as_array().into_iter().flatten() {
                    if model["name"]
                        .as_str()
                        .is_some_and(|m| m.trim_end_matches(":latest") == name)
                    {
                        peak = peak.max(model["size"].as_u64().unwrap_or(c.bytes));
                    }
                }
            }
        }
        times.sort();
        let quality = correct as f64 / 20.0;
        let b = kokin_ai::Benchmark {
            hardware_identity: hardware.clone(),
            p95_ms: times[18],
            peak_memory_bytes: peak,
            citation_quality_passed: correct >= 18,
            quality_score: quality,
            samples: 20,
        };
        measurements.insert(c.id.clone(),serde_json::json!({"artifact_sha256":c.sha256,"ollama_digest":digest,"hardware_identity":b.hardware_identity,"p95_ms":b.p95_ms,"peak_memory_bytes":b.peak_memory_bytes,"citation_quality_passed":b.citation_quality_passed,"quality_score":b.quality_score,"samples":b.samples}));
        c.benchmark = Some(b);
    }
    if measurements.is_empty() {
        return Err(invalid(
            "no eligible reviewed local model is available to benchmark",
        ));
    }
    let available = h.available_memory_bytes.unwrap_or(0);
    let storage = h.free_storage_bytes.unwrap_or(0);
    let recommended = kokin_ai::balanced(&registry, &hardware, available, storage, "cited_summary")
        .map(|c| format!("kokin-{}", c.id));
    app.state::<crate::operations::Operations>()
        .begin_publish(operation)?;
    let data = serde_json::json!({"version":1,"hardware_identity":hardware,"measurements":measurements,"balanced":recommended});
    let destination = benchmark_path(app)?;
    let stage = destination.with_extension(format!("{operation}.incoming"));
    std::fs::write(&stage, data.to_string()).map_err(|_| invalid("benchmark staging failed"))?;
    std::fs::rename(&stage, destination).map_err(|_| invalid("benchmark publication failed"))?;
    Ok(data)
}

/// Runtime model installation is global, like the reviewed resource it consumes.
#[tauri::command]
pub fn ai_model_install(
    app: AppHandle,
    request: AiModelInstallRequest,
) -> Result<String, CommandError> {
    if request.version != 1 || !request.consent {
        return Err(invalid("review the local upload disclosure first"));
    }
    let candidate = managed(&request.model)?;
    client(&request.endpoint)?;
    let (job, cancel) = app
        .state::<crate::resources::ResourceJobs>()
        .begin(&format!("ollama:{}", candidate.id), candidate.bytes)?;
    let output = job.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||import_model(&app,&serde_json::json!({"model":request.model,"endpoint":request.endpoint,"consent":true}),cancel)))
            .unwrap_or_else(|_|Err(invalid("local model import worker failed")))
            .map(|_|request.model);
        app.state::<crate::resources::ResourceJobs>()
            .finish(&job, result);
    });
    Ok(output)
}
