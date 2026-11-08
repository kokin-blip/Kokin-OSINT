//! Generation-bound workspace commands; data stays in the encrypted database.
use crate::views::{GuideStatus, WorkspacePage, WorkspaceSaveRequest, WorkspaceView};
use crate::{CommandError, ErrorCode, Session};
use kokin_store::workspace::{self, SaveWorkspace, Workspace, WorkspaceError};
use tauri::{AppHandle, Manager};
fn mapped(error: WorkspaceError) -> CommandError {
    match error {
        WorkspaceError::Storage(_) => {
            CommandError::new(ErrorCode::Storage, "workspace storage failed")
        }
        other => CommandError::new(ErrorCode::InvalidValue, other.to_string()),
    }
}
#[tauri::command]
pub async fn workspace_page(
    app: AppHandle,
    generation: u64,
    kind: String,
    offset: usize,
) -> Result<WorkspacePage, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>().with_generation(generation, |open| {
            let (rows, total) = workspace::list(&open.conn, &kind, offset).map_err(mapped)?;
            Ok(WorkspacePage {
                rows: rows.into_iter().map(Into::into).collect(),
                total,
                offset,
                limit: 50,
            })
        })
    })
    .await
    .map_err(|_| CommandError::new(ErrorCode::Internal, "workspace worker failed"))?
}
#[tauri::command]
pub async fn workspace_read(
    app: AppHandle,
    generation: u64,
    id: String,
) -> Result<WorkspaceView, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>().with_generation(generation, |open| {
            let mut value = workspace::get(&open.conn, &id)
                .map_err(mapped)?
                .ok_or_else(|| CommandError::new(ErrorCode::NotFound, "workspace not found"))?;
            if value.kind == "board" {
                hydrate_board(&open.conn, &mut value)?;
            }
            Ok(value.into())
        })
    })
    .await
    .map_err(|_| CommandError::new(ErrorCode::Internal, "workspace worker failed"))?
}
#[tauri::command]
pub async fn workspace_save(
    app: AppHandle,
    generation: u64,
    request: WorkspaceSaveRequest,
) -> Result<WorkspaceView, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>().with_generation(generation, |open| {
            workspace::save(
                &open.conn,
                &SaveWorkspace {
                    id: request.id,
                    kind: request.kind,
                    name: request.name,
                    expected_revision: request.expected_revision,
                    payload: request.payload,
                    archived: request.archived,
                },
            )
            .map(Into::into)
            .map_err(mapped)
        })
    })
    .await
    .map_err(|_| CommandError::new(ErrorCode::Internal, "workspace worker failed"))?
}

pub(crate) fn hydrate_board(
    conn: &rusqlite::Connection,
    value: &mut Workspace,
) -> Result<(), CommandError> {
    use rusqlite::OptionalExtension;
    let mut notices = Vec::new();
    if let Some(view) = value
        .payload
        .get_mut("view")
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(nodes) = view
            .get_mut("nodes")
            .and_then(serde_json::Value::as_array_mut)
        {
            let mut refreshed = Vec::new();
            for node in nodes.iter().take(300) {
                let id = node["id"].as_str().unwrap_or("");
                let row:Option<(String,String)>=conn.query_row("SELECT e.display_name,COALESCE(m.canonical_id,e.id) FROM entity e LEFT JOIN er_merge_map m ON m.entity_id=e.id AND m.active=1 WHERE e.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(kokin_store::StoreError::from)?;
                if let Some((name, canonical)) = row {
                    let mut node = node.clone();
                    if node["canonical_id"].as_str() != Some(&canonical) {
                        notices.push(format!("Record {id} now resolves to {canonical}"));
                    }
                    node["name"] = name.into();
                    node["canonical_id"] = canonical.into();
                    refreshed.push(node);
                } else {
                    notices.push(format!("Record {id} is unavailable"));
                }
            }
            *nodes = refreshed;
        }
        let ids = std::collections::HashSet::<String>::from_iter(
            view["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n["id"].as_str().map(str::to_owned)),
        );
        if let Some(edges) = view
            .get_mut("edges")
            .and_then(serde_json::Value::as_array_mut)
        {
            let mut refreshed = Vec::new();
            for edge in edges.iter().take(1000) {
                let id = edge["id"].as_str().unwrap_or("");
                let row: Option<(String, String, String)> = conn
                    .query_row(
                        "SELECT from_entity,to_entity,kind FROM relationship WHERE id=?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()
                    .map_err(kokin_store::StoreError::from)?;
                if let Some((from, to, kind)) = row {
                    if ids.contains(&from) && ids.contains(&to) {
                        refreshed
                            .push(serde_json::json!({"id":id,"from":from,"to":to,"kind":kind}));
                    }
                } else {
                    notices.push(format!("Relationship {id} is unavailable"));
                }
            }
            *edges = refreshed;
        }
    }
    value.payload["notices"] = serde_json::json!(notices);
    Ok(())
}
#[tauri::command]
pub async fn guide_status(app: AppHandle, generation: u64) -> Result<GuideStatus, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Session>().with_generation(generation, |open| {
            let count = |sql: &str| -> Result<i64, CommandError> {
                open.conn
                    .query_row(sql, [], |r| r.get(0))
                    .map_err(|e| kokin_store::StoreError::from(e).into())
            };
            Ok(GuideStatus {
                artifacts: count("SELECT COUNT(*) FROM artifact")?,
                complete: crate::read::coverage(&open.conn)?.complete,
                entities: count("SELECT COUNT(*) FROM entity")?,
                relationships: count("SELECT COUNT(*) FROM relationship")?,
                human_assessments: count(
                    "SELECT COUNT(*) FROM assessment WHERE actor LIKE 'user:%' AND subject_kind='relationship'",
                )?,
                boards: workspace::list(&open.conn, "board", 0).map_err(mapped)?.1,
                reports: workspace::list(&open.conn, "report", 0).map_err(mapped)?.1,
            })
        })
    })
    .await
    .map_err(|_| CommandError::new(ErrorCode::Internal, "workspace worker failed"))?
}
