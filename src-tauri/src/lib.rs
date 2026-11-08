//! Tauri command surface.
//!
//! This layer stays deliberately thin: it validates and marshals, and delegates
//! every decision to the `kokin-*` crates. Business logic here would be
//! untestable without a webview.
//!
//! That is why every `#[tauri::command]` below is one line over a method on
//! [`session::Session`]. `Session` knows nothing about Tauri, so the whole
//! lifecycle — create, open, recover, close, and every refusal — is exercised
//! by ordinary `cargo test` with no window on screen. What is not covered by
//! those tests is exactly the one line of marshalling, which is the amount of
//! untested code this arrangement is buying.
//!
//! # What crosses this boundary
//!
//! Nothing carrying key material, with one deliberate exception: the recovery
//! key, returned once by `create_case` and never again (see
//! [`session::NewCaseView`]). `kokin_store::OpenCase` holds the case master key
//! and does not implement `Serialize`; it must never gain it.

#[cfg(all(feature = "desktop-test", not(debug_assertions)))]
compile_error!("desktop-test must never be enabled in release binaries");

#[cfg(feature = "desktop-test")]
pub mod desktop_test;

pub mod ai;
pub mod discovery;
pub mod error;
pub mod faces;
pub mod image_query;
pub mod maps;
pub mod media;
pub mod operations;
pub mod read;
pub mod reports;
pub mod research;
pub mod research_import;
pub mod research_preview;
pub mod resources;
pub mod semantic;
pub mod session;
pub mod sources;
pub mod views;
pub mod workspace;
pub mod write;

pub use error::{CommandError, ErrorCode};
pub use session::Session;
pub use views::{
    ArtifactRowView, AssessRequest, CaseView, ClipboardImageRequest, CoverageView, DocumentContent,
    DocumentView, EntityRowView, EntityView, EvidenceListView, ExtractRequest, ExtractView,
    GroundingInput, HitView, ImagePrepareRequest, ImageSearchRequest, IngestFileRequest,
    IngestView, LineageView, MergeRequest, NewCaseView, NewEntityRequest, NewIdentifierRequest,
    NewRelationshipRequest, ObservationView, QuoteView, RejectRequest, ResolutionView,
    SearchRequest, SearchView, SessionView, SplitRequest, WriteView,
};

use std::path::PathBuf;

use tauri::{Manager, State};

/// Reports whether the storage layer is genuinely linked against SQLCipher.
///
/// Surfaced in the UI's about panel so a mis-built binary that would silently
/// write unencrypted cases is visible to the user, not just to CI.
#[tauri::command]
fn storage_encryption_status() -> Result<String, CommandError> {
    // Queries the linked library rather than creating a throwaway case, so this
    // costs no key derivation and touches no filesystem.
    match kokin_store::cipher_backend() {
        Ok(Some(version)) => Ok(format!("sqlcipher {version}")),
        Ok(None) => Err(CommandError::new(
            ErrorCode::Storage,
            "plain sqlite — case files would NOT be encrypted",
        )),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
fn create_case(
    app: tauri::AppHandle,
    session: State<'_, Session>,
    path: PathBuf,
    case_id: String,
    passphrase: String,
) -> Result<NewCaseView, CommandError> {
    if path.as_os_str().is_empty() {
        let directory = app.path().app_data_dir().map_err(|_| {
            CommandError::new(ErrorCode::Storage, "application data folder unavailable")
        })?;
        session.create_auto(&directory.join("cases"), &case_id, &passphrase)
    } else {
        session.create(&path, &case_id, &passphrase)
    }
}

#[tauri::command]
fn open_case(
    session: State<'_, Session>,
    path: PathBuf,
    passphrase: String,
) -> Result<CaseView, CommandError> {
    session.open(&path, &passphrase)
}

#[tauri::command]
fn open_case_with_recovery_key(
    session: State<'_, Session>,
    path: PathBuf,
    recovery_key: String,
) -> Result<CaseView, CommandError> {
    session.open_with_recovery_key(&path, &recovery_key)
}

#[tauri::command]
async fn close_case(app: tauri::AppHandle) -> Result<(), CommandError> {
    use tauri::Manager;
    app.state::<operations::Operations>().cancel_all();
    app.state::<research::ResearchPlans>().clear();
    tauri::async_runtime::spawn_blocking(move || {
        let result = app.state::<Session>().close();
        app.state::<research::ResearchPlans>().clear();
        result
    })
    .await
    .map_err(|_| CommandError::new(ErrorCode::Internal, "case close failed"))?
}

#[tauri::command]
fn session_status(session: State<'_, Session>) -> Result<SessionView, CommandError> {
    session.view()
}

/// Search the open case. Returns hits and coverage together, always.
#[tauri::command]
fn search(session: State<'_, Session>, request: SearchRequest) -> Result<SearchView, CommandError> {
    session.with_case(|open| read::search(open, &request))
}

/// How much of the case a search can reach, for a view that has not searched.
#[tauri::command]
fn coverage(session: State<'_, Session>) -> Result<CoverageView, CommandError> {
    session.with_case(|open| read::coverage(&open.conn))
}

/// An artifact and whatever the case can still show of it.
#[tauri::command]
fn artifact_document(
    session: State<'_, Session>,
    artifact_id: String,
) -> Result<DocumentView, CommandError> {
    session.with_case(|open| read::artifact_document(open, &artifact_id))
}

/// Every artifact in the case, with how much of each has been read.
#[tauri::command]
fn artifacts(
    session: State<'_, Session>,
    limit: Option<usize>,
) -> Result<Vec<ArtifactRowView>, CommandError> {
    session.with_case(|open| read::artifacts(open, limit))
}

/// What one artifact yielded, and what the run that read it says it missed.
#[tauri::command]
fn artifact_observations(
    session: State<'_, Session>,
    artifact_id: String,
    include_superseded: bool,
) -> Result<EvidenceListView, CommandError> {
    session.with_case(|open| read::artifact_observations(open, &artifact_id, include_superseded))
}

/// One observation, its lineage, and the bytes it cites.
#[tauri::command]
fn observation(
    session: State<'_, Session>,
    observation_id: String,
) -> Result<ObservationView, CommandError> {
    session.with_case(|open| read::observation(open, &observation_id))
}

/// Every entity in the case, canonical rows only.
#[tauri::command]
fn entities(
    session: State<'_, Session>,
    limit: Option<usize>,
) -> Result<Vec<EntityRowView>, CommandError> {
    session.with_case(|open| read::entities(open, limit))
}

/// An entity, read through the merge map.
#[tauri::command]
fn entity(session: State<'_, Session>, entity_id: String) -> Result<EntityView, CommandError> {
    session.with_case(|open| read::entity(open, &entity_id))
}

// Writes. Every one of them records `write::ACTOR`, and none of them takes an
// actor — see the note at the top of [`write`] for why that is the increment's
// central control rather than a detail of these signatures.

/// Read an artifact already in the case and record what it says.
#[tauri::command]
fn extract(
    session: State<'_, Session>,
    request: ExtractRequest,
) -> Result<ExtractView, CommandError> {
    session.with_case(|open| write::extract(open, &request))
}

#[tauri::command]
fn create_entity(
    session: State<'_, Session>,
    request: NewEntityRequest,
) -> Result<WriteView, CommandError> {
    session.with_case(|open| write::create_entity(open, &request))
}

#[tauri::command]
fn add_identifier(
    session: State<'_, Session>,
    request: NewIdentifierRequest,
) -> Result<WriteView, CommandError> {
    session.with_case(|open| write::add_identifier(open, &request))
}

#[tauri::command]
fn relate(
    session: State<'_, Session>,
    request: NewRelationshipRequest,
) -> Result<WriteView, CommandError> {
    session.with_case(|open| write::relate(open, &request))
}

#[tauri::command]
fn assess(session: State<'_, Session>, request: AssessRequest) -> Result<WriteView, CommandError> {
    session.with_case(|open| write::assess(open, &request))
}

#[tauri::command]
fn merge_entities(
    session: State<'_, Session>,
    request: MergeRequest,
) -> Result<ResolutionView, CommandError> {
    session.with_case(|open| write::merge(open, &request))
}

#[tauri::command]
fn reject_merge(
    session: State<'_, Session>,
    request: RejectRequest,
) -> Result<WriteView, CommandError> {
    session.with_case(|open| write::reject(open, &request))
}

#[tauri::command]
fn split_entity(
    session: State<'_, Session>,
    request: SplitRequest,
) -> Result<ResolutionView, CommandError> {
    session.with_case(|open| write::split(open, &request))
}

#[tauri::command]
fn artifact_page(
    session: State<'_, Session>,
    request: views::PageRequest,
) -> Result<views::PageView<ArtifactRowView>, CommandError> {
    session.with_case(|open| read::artifact_page(open, &request))
}
#[tauri::command]
fn entity_page(
    session: State<'_, Session>,
    request: views::PageRequest,
) -> Result<views::PageView<EntityRowView>, CommandError> {
    session.with_case(|open| read::entity_page(open, &request))
}
#[tauri::command]
fn observation_page(
    session: State<'_, Session>,
    artifact_id: String,
    include_superseded: bool,
    request: views::PageRequest,
) -> Result<(EvidenceListView, views::PageView<views::ObservationRowView>), CommandError> {
    session
        .with_case(|open| read::observation_page(open, &artifact_id, include_superseded, &request))
}
#[tauri::command]
fn activity(
    session: State<'_, Session>,
    request: views::PageRequest,
) -> Result<views::ActivityView, CommandError> {
    session.with_case(|open| read::activity(open, &request))
}
#[tauri::command]
fn subject(
    session: State<'_, Session>,
    subject_kind: String,
    subject_id: String,
) -> Result<views::SubjectView, CommandError> {
    session.with_case(|open| read::subject(open, &subject_kind, &subject_id))
}
#[tauri::command]
fn scales(session: State<'_, Session>) -> Result<Vec<views::ScaleOptionView>, CommandError> {
    session.with_case(|open| read::scales(open))
}

#[tauri::command]
fn entity_types(session: State<'_, Session>) -> Result<Vec<(String, String)>, CommandError> {
    session.with_case(|open| {
        let mut stmt = open
            .conn
            .prepare("SELECT key,label FROM entity_type ORDER BY key")
            .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))
    })
}

#[tauri::command]
fn evidence_target(
    session: State<'_, Session>,
    kind: String,
    id: String,
) -> Result<(String, String), CommandError> {
    session.with_case(|open| read::evidence_target(open, &kind, &id))
}
#[tauri::command]
fn relationships(
    session: State<'_, Session>,
    id: String,
    request: views::PageRequest,
) -> Result<views::PageView<views::SubjectView>, CommandError> {
    session.with_case(|open| read::relationships(open, &id, &request))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::expect_used)]
pub fn run() {
    // Failing to start the webview is unrecoverable and happens before any case
    // is open, so there is no case data to lose. This is the one deliberate
    // panic in the application; everything downstream returns errors instead.
    let builder = tauri::Builder::default();
    #[cfg(feature = "desktop-test")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                use tauri::{Emitter, Manager};
                let session = window.state::<Session>();
                let generation = session.generation();
                // Only the native event supplies filesystem paths. The webview receives a
                // single-use, expiring selection and must explicitly preview/import it.
                if session.with_generation(generation, |_| Ok(())).is_ok() {
                    if let Ok(payload) = window
                        .state::<operations::Operations>()
                        .select_drop(paths, generation)
                    {
                        let _ = window.emit("research-file-drop", payload);
                    }
                }
            }
        })
        .manage(Session::default())
        .manage(faces::FacePreviews::default())
        .manage(operations::Operations::default())
        .manage(research::ResearchPlans::default())
        .manage(resources::ResourceJobs::default())
        .manage(maps::MapResources::default())
        .invoke_handler(tauri::generate_handler![
            faces::face_read,
            faces::face_group_members,
            faces::person_faces,
            faces::face_person_create,
            faces::face_preview,
            faces::face_status,
            faces::face_auto_analyze,
            faces::face_page,
            faces::face_search,
            faces::face_review,
            faces::face_groups,
            discovery::discovery_health,
            discovery::research_browser,
            sources::source_disclosure,
            image_query::image_query_info,
            image_query::image_query_search,
            research::research_registry,
            research::research_local_search,
            research::research_import_session,
            research::research_table,
            research::research_table_map,
            research::research_table_suggestions,
            research::research_table_candidates,
            research::research_candidates,
            research::research_evidence_preview,
            research::research_import_preview,
            research::research_open_url,
            research::research_provider_status,
            research::research_directory,
            research::research_selector,
            research::research_bulk,
            research::research_prepare,
            research::research_runs,
            research::research_results,
            research::research_review_candidate,
            research::research_documents,
            research::research_document_save,
            maps::map_illustrations,
            maps::map_archives,
            maps::map_select,
            maps::map_world_review,
            maps::map_world_install,
            maps::map_import,
            maps::map_range,
            maps::location_page,
            maps::location_create,
            maps::map_state_read,
            maps::map_state_save,
            ai::ai_status,
            ai::ai_model_install,
            ai::ai_page,
            ai::ai_review,
            semantic::semantic_status,
            semantic::semantic_search,
            resources::resource_jobs,
            resources::resource_list,
            resources::resource_install,
            resources::resource_status,
            resources::resource_cancel,
            workspace::guide_status,
            workspace::workspace_page,
            workspace::workspace_read,
            media::media_read,
            media::ocr_resource_info,
            media::install_ocr_resource,
            workspace::workspace_save,
            #[cfg(feature = "desktop-test")]
            desktop_test::test_control,
            evidence_target,
            relationships,
            entity_types,
            artifact_page,
            entity_page,
            observation_page,
            activity,
            subject,
            scales,
            operations::select_path,
            operations::start_operation,
            operations::operation_status,
            operations::cancel_operation,
            operations::network_mode,
            operations::set_network_mode,
            storage_encryption_status,
            create_case,
            open_case,
            open_case_with_recovery_key,
            close_case,
            session_status,
            search,
            coverage,
            artifact_document,
            artifacts,
            artifact_observations,
            observation,
            entities,
            entity,
            extract,
            create_entity,
            add_identifier,
            relate,
            assess,
            merge_entities,
            reject_merge,
            split_entity,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Kokin-OSINT window");
}
