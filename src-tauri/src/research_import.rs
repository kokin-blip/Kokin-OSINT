//! Durable, encrypted import workspaces. Submission comes from runs, never a UI flag.
use crate::{CommandError, ErrorCode};
use kokin_connectors::research::{BulkRow, ImportParent, Selector};
use kokin_store::{research, OpenCase};
use serde_json::{json, Value};
fn invalid(message: &str) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, message)
}
pub fn validate(open: &OpenCase, document: &research::Document) -> Result<(), CommandError> {
    if document.kind != "saved_search" || document.payload["purpose"] != "import_session" {
        return Err(invalid("choose a saved import workspace"));
    }
    let rows: Vec<BulkRow> = serde_json::from_value(document.payload["rows"].clone())
        .map_err(|_| invalid("invalid import rows"))?;
    if rows.is_empty() || rows.len() > 20_000 {
        return Err(invalid("save 1–20,000 reviewed import rows"));
    }
    let mut keys = std::collections::HashSet::new();
    for row in rows {
        if row.value.len() > 8192
            || row.error.as_ref().is_some_and(|s| s.len() > 8192)
            || row.repeats == 0
            || row.origin_ids.len() > 100
            || row
                .origin_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128)
        {
            return Err(invalid("import row exceeds its preview bounds"));
        }
        if let Some(selector) = row.selector {
            let parsed = Selector::parse(row.kind, &row.value)
                .map_err(|_| invalid("saved selector is invalid"))?;
            if selector.kind != parsed.kind
                || selector.original != parsed.original
                || selector.normalized != parsed.normalized
                || !keys.insert((format!("{:?}", selector.kind), selector.normalized))
            {
                return Err(invalid(
                    "saved rows must use the validated, distinct preview values",
                ));
            }
        }
    }
    let _ = open; // Case ownership is enforced by the revisioned document store.
    Ok(())
}
pub fn validate_parent(
    open: &OpenCase,
    parent: &ImportParent,
    selectors: &[Selector],
) -> Result<(), CommandError> {
    let document = research::document(&open.conn, &parent.id)?
        .ok_or_else(|| invalid("parent import is not in this case"))?;
    validate(open, &document)?;
    if document.archived || document.revision != parent.revision {
        return Err(invalid("parent import changed; reload and review again"));
    }
    let rows: Vec<BulkRow> = serde_json::from_value(document.payload["rows"].clone())
        .map_err(|_| invalid("invalid saved import"))?;
    if selectors.iter().any(|s| {
        !rows.iter().any(|r| {
            r.selector.as_ref().is_some_and(|v| {
                v.kind == s.kind && v.original == s.original && v.normalized == s.normalized
            })
        })
    }) {
        return Err(invalid(
            "selected query is not in the reviewed parent import",
        ));
    }
    Ok(())
}
pub fn state(open: &OpenCase, id: &str, offset: usize) -> Result<Value, CommandError> {
    if offset > 1_000_000 {
        return Err(invalid("import run offset exceeds bound"));
    }
    let document =
        research::document(&open.conn, id)?.ok_or_else(|| invalid("saved import unavailable"))?;
    validate(open, &document)?;
    let mut statement = open.conn.prepare("SELECT DISTINCT json_extract(s.value,'$.kind'),json_extract(s.value,'$.normalized') FROM research_run r,json_each(r.plan_json,'$.selectors') s WHERE json_extract(r.plan_json,'$.parent_import.id')=?1")?;
    let keys = statement
        .query_map([id], |r| {
            Ok(format!(
                "{}:{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let total: i64 = open.conn.query_row(
        "SELECT COUNT(*) FROM research_run WHERE json_extract(plan_json,'$.parent_import.id')=?1",
        [id],
        |r| r.get(0),
    )?;
    let mut statement=open.conn.prepare("SELECT id,status,expected,completed,created_utc FROM research_run WHERE json_extract(plan_json,'$.parent_import.id')=?1 ORDER BY rowid DESC LIMIT 50 OFFSET ?2")?;
    let runs=statement.query_map(rusqlite::params![id,offset as i64],|r|Ok(json!({"id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"expected":r.get::<_,i64>(2)?,"completed":r.get::<_,i64>(3)?,"created_utc":r.get::<_,String>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(
        json!({"document":document,"submitted_keys":keys,"runs":{"rows":runs,"total":total,"offset":offset,"limit":50}}),
    )
}
