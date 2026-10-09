//! Debug-only desktop fixtures. Never compiled into production builds.
use crate::{operations::Operations, CommandError, ErrorCode, Session};
use std::{collections::VecDeque, path::PathBuf};
use tauri::{Manager, State};
#[derive(Default)]
pub struct TestState {
    pub selections: VecDeque<(String, PathBuf)>,
    pub replay: Option<PathBuf>,
}
#[tauri::command]
pub fn test_control(
    app: tauri::AppHandle,
    session: State<'_, Session>,
    action: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, CommandError> {
    let bad = || CommandError::new(ErrorCode::InvalidValue, "invalid desktop test request");
    let operations = app.state::<Operations>();
    match action.as_str() {
        "selection" => {
            let purpose = args["purpose"].as_str().ok_or_else(bad)?;
            let path = args["path"].as_str().ok_or_else(bad)?;
            operations
                .test_state
                .lock()
                .map_err(|_| bad())?
                .selections
                .push_back((purpose.into(), path.into()));
            Ok(serde_json::json!(null))
        }
        "replay" => {
            let directory = PathBuf::from(args["directory"].as_str().ok_or_else(bad)?);
            let url = args["url"].as_str().ok_or_else(bad)?;
            let body = args["body"].as_str().ok_or_else(bad)?;
            let request = kokin_net::HttpRequest::get(url);
            let key = request.fixture_key().map_err(|_| bad())?;
            kokin_net::ReplayHttp::new(&directory)
                .store(&kokin_net::http::Fixture {
                    format_version: 2,
                    key,
                    request,
                    response: kokin_net::HttpResponse {
                        status: 200,
                        headers: [("content-type".into(), "text/html".into())].into(),
                        body: String::new(),
                        body_bytes: Some(body.as_bytes().to_vec()),
                        redirect_chain: Vec::new(),
                        from_fixture: true,
                    },
                    recorded_utc: "2026-10-08T00:00:00Z".into(),
                    note: Some("synthetic desktop acceptance".into()),
                })
                .map_err(|_| bad())?;
            operations.test_state.lock().map_err(|_| bad())?.replay = Some(directory);
            Ok(serde_json::json!(null))
        }
        "snapshot" => session.with_case(|open| {
            let storage = |e: rusqlite::Error| CommandError::new(ErrorCode::Storage, e.to_string());
            let mut snapshot = serde_json::Map::new();
            for table in [
                "source",
                "capture",
                "artifact",
                "observation",
                "entity",
                "identifier",
                "relationship",
                "evidence_link",
                "assessment",
                "transform_run",
            ] {
                let mut statement = open
                    .conn
                    .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                    .map_err(storage)?;
                let columns = statement.column_count();
                let rows = statement
                    .query_map([], |r| {
                        let mut values = Vec::new();
                        for i in 0..columns {
                            values.push(format!("{:?}", r.get::<_, rusqlite::types::Value>(i)?));
                        }
                        Ok(values)
                    })
                    .map_err(storage)?;
                let values = rows.collect::<Result<Vec<_>, _>>().map_err(storage)?;
                snapshot.insert(table.into(), serde_json::json!(values));
            }
            Ok(serde_json::Value::Object(snapshot))
        }),
        _ => Err(bad()),
    }
}
