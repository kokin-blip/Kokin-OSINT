//! Encrypted, revisioned UI workspaces. No analytical facts are changed here.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub revision: i64,
    pub payload: Value,
    pub archived: bool,
    pub updated_utc: String,
}
#[derive(Debug)]
pub struct SaveWorkspace {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub expected_revision: i64,
    pub payload: Value,
    pub archived: bool,
}
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("invalid workspace; check its name, version and size")]
    Invalid,
    #[error("workspace changed; reload its latest revision before saving")]
    Conflict,
    #[error("workspace storage failed")]
    Storage(#[from] rusqlite::Error),
}
pub type Result<T> = std::result::Result<T, WorkspaceError>;
fn kind_valid(kind: &str) -> bool {
    matches!(kind, "board" | "report" | "guide")
}
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    let json: String = row.get(4)?;
    let payload = serde_json::from_str(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Workspace {
        id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        revision: row.get(3)?,
        payload,
        archived: row.get(5)?,
        updated_utc: row.get(6)?,
    })
}
pub fn get(conn: &Connection, id: &str) -> Result<Option<Workspace>> {
    Ok(conn.query_row("SELECT workspace_id,kind,name,revision,payload_json,archived,updated_utc FROM workspace_revision WHERE workspace_id=?1 ORDER BY revision DESC LIMIT 1",[id],row).optional()?)
}
pub fn list(conn: &Connection, kind: &str, offset: usize) -> Result<(Vec<Workspace>, i64)> {
    if !kind_valid(kind) || offset > 1_000_000 {
        return Err(WorkspaceError::Invalid);
    }
    let filter="FROM workspace_revision w WHERE kind=?1 AND archived=0 AND revision=(SELECT MAX(revision) FROM workspace_revision WHERE workspace_id=w.workspace_id)";
    let count = conn.query_row(&format!("SELECT COUNT(*) {filter}"), [kind], |r| r.get(0))?;
    let mut stmt=conn.prepare(&format!("SELECT workspace_id,kind,name,revision,'{{}}',archived,updated_utc {filter} ORDER BY updated_utc DESC,workspace_id LIMIT 50 OFFSET ?2"))?;
    let rows = stmt
        .query_map(params![kind, offset as i64], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok((rows, count))
}
pub fn save(conn: &Connection, input: &SaveWorkspace) -> Result<Workspace> {
    let json = serde_json::to_string(&input.payload).map_err(|_| WorkspaceError::Invalid)?;
    if !kind_valid(&input.kind)
        || input.id.is_empty()
        || input.id.len() > 128
        || !input
            .id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || input.name.trim().is_empty()
        || input.name.chars().count() > 120
        || input.expected_revision < 0
        || json.len()
            > if input.kind == "board" {
                2_097_152
            } else {
                262_144
            }
        || input.payload.get("version").and_then(Value::as_u64) != Some(1)
    {
        return Err(WorkspaceError::Invalid);
    }
    let transaction = conn.unchecked_transaction()?;
    let previous = get(&transaction, &input.id)?;
    if previous.as_ref().map_or(0, |w| w.revision) != input.expected_revision
        || previous.as_ref().is_some_and(|w| w.kind != input.kind)
    {
        return Err(WorkspaceError::Conflict);
    }
    let revision = input
        .expected_revision
        .checked_add(1)
        .ok_or(WorkspaceError::Invalid)?;
    let updated = crate::now_utc_rfc3339();
    transaction.execute("INSERT INTO workspace_revision(workspace_id,revision,kind,name,payload_json,archived,updated_utc) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![input.id,revision,input.kind,input.name.trim(),json,input.archived,updated])?;
    transaction.commit()?;
    Ok(Workspace {
        id: input.id.clone(),
        kind: input.kind.clone(),
        name: input.name.trim().into(),
        revision,
        payload: input.payload.clone(),
        archived: input.archived,
        updated_utc: updated,
    })
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::migrate(&conn).unwrap();
        conn
    }
    fn input(revision: i64) -> SaveWorkspace {
        SaveWorkspace {
            id: "board-1".into(),
            kind: "board".into(),
            name: "Investigation".into(),
            expected_revision: revision,
            payload: serde_json::json!({"version":1,"seeds":[]}),
            archived: false,
        }
    }
    #[test]
    fn stale_saves_are_refused_and_archive_preserves_history() {
        let conn = db();
        save(&conn, &input(0)).unwrap();
        assert!(matches!(
            save(&conn, &input(0)),
            Err(WorkspaceError::Conflict)
        ));
        let mut archived = input(1);
        archived.archived = true;
        save(&conn, &archived).unwrap();
        assert_eq!(list(&conn, "board", 0).unwrap().1, 0);
        assert_eq!(get(&conn, "board-1").unwrap().unwrap().revision, 2);
        assert!(conn.execute("DELETE FROM workspace_revision", []).is_err());
        assert!(conn
            .execute("UPDATE workspace_revision SET name='x'", [])
            .is_err());
    }
    #[test]
    fn invalid_payload_and_kind_change_cannot_commit() {
        let conn = db();
        let mut request = input(0);
        request.payload = serde_json::json!({"version":2});
        assert!(save(&conn, &request).is_err());
        save(&conn, &input(0)).unwrap();
        request = input(1);
        request.kind = "report".into();
        assert!(matches!(
            save(&conn, &request),
            Err(WorkspaceError::Conflict)
        ));
        assert_eq!(get(&conn, "board-1").unwrap().unwrap().kind, "board");
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod fault_tests {
    use super::*;
    #[test]
    fn full_database_does_not_commit_an_incomplete_revision() {
        let conn = Connection::open_in_memory().unwrap();
        crate::migrations::migrate(&conn).unwrap();
        let first = SaveWorkspace {
            id: "full-test".into(),
            kind: "report".into(),
            name: "Initial".into(),
            expected_revision: 0,
            payload: serde_json::json!({"version":1}),
            archived: false,
        };
        save(&conn, &first).unwrap();
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        conn.execute_batch(&format!("PRAGMA max_page_count={pages}"))
            .unwrap();
        let large = SaveWorkspace {
            expected_revision: 1,
            payload: serde_json::json!({"version":1,"text":"x".repeat(200_000)}),
            ..first
        };
        assert!(save(&conn, &large).is_err());
        assert_eq!(get(&conn, "full-test").unwrap().unwrap().revision, 1);
    }
}
