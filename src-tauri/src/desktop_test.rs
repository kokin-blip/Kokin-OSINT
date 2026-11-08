//! Debug-only desktop fixtures. Never compiled into production builds.
use crate::{operations::Operations, CommandError, ErrorCode, Session};
use std::{collections::VecDeque, path::PathBuf};
use tauri::{Manager, State};
#[path = "../tests/support/corpus.rs"]
mod corpus;
#[derive(Default)]
pub struct TestState {
    pub selections: VecDeque<(String, PathBuf)>,
    pub replay: Option<PathBuf>,
    pub ai_response: Option<serde_json::Value>,
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
        "ai_replay" => {operations.test_state.lock().map_err(|_|bad())?.ai_response=Some(args["response"].clone());Ok(serde_json::Value::Null)},
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
            let mut request = kokin_net::HttpRequest::get(url);
            if let Some(accept)=args["accept"].as_str(){request=request.with_header("Accept",accept);}
            let key = request.fixture_key().map_err(|_| bad())?;
            kokin_net::ReplayHttp::new(&directory)
                .store(&kokin_net::http::Fixture {
                    format_version: 2,
                    key,
                    request,
                    response: kokin_net::HttpResponse {
                        status: args["status"].as_u64().unwrap_or(200) as u16,
                        headers: [("content-type".into(), args["media_type"].as_str().unwrap_or("text/html").into())].into(),
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
        "maximum_board" => session.with_case(|open| {
            corpus::seed(open,100_000,20_000,300_000);
            let seeds=corpus::board(open);
            let kinds=vec!["stress_path".to_owned()];
            let expansion=kokin_graph::explore::expand_multi(&open.conn,&seeds,kokin_graph::explore::ExpansionOptions{hops:4,node_limit:300,per_node:128,direction:"both",kinds:&kinds},&std::sync::atomic::AtomicBool::new(false)).map_err(|_|bad())?;
            let view=serde_json::json!({"nodes":expansion.nodes.iter().map(|n|serde_json::json!({"id":n.id,"name":n.name,"canonical_id":n.canonical_id,"depth":n.depth})).collect::<Vec<_>>(),"edges":expansion.edges.iter().map(|e|serde_json::json!({"id":e.id,"from":e.from,"to":e.to,"kind":e.kind})).collect::<Vec<_>>(),"completed_hops":expansion.completed_hops,"truncated":expansion.truncated,"truncation_reasons":expansion.truncation_reasons});
            let board=kokin_store::workspace::save(&open.conn,&kokin_store::workspace::SaveWorkspace{id:"maximum-board".into(),kind:"board".into(),name:"Synthetic maximum board".into(),expected_revision:0,payload:serde_json::json!({"version":1,"view":view,"roots":seeds,"positions":{},"camera":{"x":0,"y":0,"scale":1},"hops":4,"limit":300,"perNode":128,"direction":"both","relationshipKind":"stress_path","visibleQuery":""}),archived:false}).map_err(|_|bad())?;
            Ok(serde_json::json!({"board_id":board.id,"nodes":expansion.nodes.len(),"edges":expansion.edges.len()}))
        }),
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
                "ai_suggestion",
                "ai_suggestion_citation",
                "ai_review",
                "observation_embedding",
                "grounded_location",
                "map_state_revision",
                "map_illustration",
                "image_query",
                "face_run",
                "face_detection",
                "face_review",
                "face_group",
                "face_group_member",
                "face_preferences",
                "research_run",
                "research_result",
                "research_document",
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
