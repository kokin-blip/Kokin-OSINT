//! Suggestions and human review never create analytical records.
use crate::Suggestion;
use rusqlite::{params, Connection};
use serde_json::Value;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid suggestion or review")]
    Invalid,
    #[error("storage: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("audit: {0}")]
    Store(#[from] kokin_store::StoreError),
}
pub fn id() -> Result<String, Error> {
    let mut bytes = [0; 24];
    getrandom::fill(&mut bytes).map_err(|_| Error::Invalid)?;
    Ok(kokin_store::blake3_hex(&bytes))
}
#[derive(Debug)]
pub struct Stored {
    pub id: String,
    pub model: String,
    pub model_digest: String,
    pub suggestion: Suggestion,
    pub created_utc: String,
    pub review: Option<String>,
    pub review_rationale: Option<String>,
    pub review_actor: Option<String>,
    pub review_utc: Option<String>,
}
pub fn save(
    conn: &mut Connection,
    model: &str,
    digest: &str,
    prompt_hash: &str,
    context_hash: &str,
    suggestions: &[Suggestion],
) -> Result<Vec<String>, Error> {
    if model.len() > 160
        || digest.len() != 64
        || !digest.bytes().all(|b| b.is_ascii_hexdigit())
        || suggestions.is_empty()
        || suggestions.len() > 12
    {
        return Err(Error::Invalid);
    }
    let tx = conn.transaction()?;
    let now = kokin_store::now_utc_rfc3339();
    let mut ids = Vec::new();
    for suggestion in suggestions {
        if suggestion.cited_observation_ids.is_empty()
            || !matches!(
                suggestion.kind.as_str(),
                "cited_summary" | "entity_candidate" | "relationship_candidate"
            )
        {
            return Err(Error::Invalid);
        }
        let sid = id()?;
        let payload = serde_json::to_string(suggestion).map_err(|_| Error::Invalid)?;
        let citations =
            serde_json::to_string(&suggestion.cited_observation_ids).map_err(|_| Error::Invalid)?;
        tx.execute(
            "INSERT INTO ai_suggestion VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                sid,
                model,
                digest,
                prompt_hash,
                context_hash,
                suggestion.kind,
                payload,
                citations,
                now
            ],
        )?;
        for observation in &suggestion.cited_observation_ids {
            tx.execute(
                "INSERT OR IGNORE INTO ai_suggestion_citation VALUES(?1,?2)",
                params![sid, observation],
            )?;
        }
        kokin_store::append_audit_event(
            &tx,
            &kokin_store::NewAuditEvent {
                actor: &format!("ai:{model}@{digest}"),
                action: "ai.suggested",
                subject_kind: Some("ai_suggestion"),
                subject_id: Some(&sid),
                payload_json:
                    &serde_json::json!({"prompt_hash":prompt_hash,"context_hash":context_hash})
                        .to_string(),
            },
        )?;
        ids.push(sid);
    }
    tx.commit()?;
    Ok(ids)
}
pub fn review(
    conn: &mut Connection,
    suggestion: &str,
    decision: &str,
    rationale: &str,
) -> Result<(), Error> {
    if !matches!(decision, "reviewed" | "rejected")
        || rationale.trim().is_empty()
        || rationale.len() > 8192
    {
        return Err(Error::Invalid);
    }
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO ai_review VALUES(?1,?2,'user:local',?3,?4,?5)",
        params![
            id()?,
            suggestion,
            decision,
            rationale,
            kokin_store::now_utc_rfc3339()
        ],
    )?;
    kokin_store::append_audit_event(
        &tx,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action: "ai.reviewed",
            subject_kind: Some("ai_suggestion"),
            subject_id: Some(suggestion),
            payload_json: &serde_json::json!({"decision":decision,"rationale":rationale})
                .to_string(),
        },
    )?;
    tx.commit()?;
    Ok(())
}
pub fn list(conn: &Connection, offset: usize) -> Result<(Vec<Stored>, i64), Error> {
    if offset > 1_000_000 {
        return Err(Error::Invalid);
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS ai_review_subject ON ai_review(suggestion_id)",
        [],
    )?;
    let total = conn.query_row("SELECT count(*) FROM ai_suggestion", [], |r| r.get(0))?;
    let mut stmt=conn.prepare("SELECT s.id,s.model,s.model_digest,s.payload_json,s.created_utc,(SELECT decision FROM ai_review WHERE suggestion_id=s.id ORDER BY rowid DESC LIMIT 1),(SELECT rationale FROM ai_review WHERE suggestion_id=s.id ORDER BY rowid DESC LIMIT 1),(SELECT actor FROM ai_review WHERE suggestion_id=s.id ORDER BY rowid DESC LIMIT 1),(SELECT created_utc FROM ai_review WHERE suggestion_id=s.id ORDER BY rowid DESC LIMIT 1) FROM ai_suggestion s ORDER BY s.rowid DESC LIMIT 50 OFFSET ?1")?;
    let rows = stmt
        .query_map([offset as i64], |r| {
            let payload: String = r.get(3)?;
            let suggestion: Suggestion = serde_json::from_str(&payload).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(Stored {
                id: r.get(0)?,
                model: r.get(1)?,
                model_digest: r.get(2)?,
                suggestion,
                created_utc: r.get(4)?,
                review: r.get(5)?,
                review_rationale: r.get(6)?,
                review_actor: r.get(7)?,
                review_utc: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok((rows, total))
}
pub fn get(conn: &Connection, id: &str) -> Result<Suggestion, Error> {
    let payload: String = conn.query_row(
        "SELECT payload_json FROM ai_suggestion WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    serde_json::from_str(&payload).map_err(|_| Error::Invalid)
}
pub fn value(s: &Suggestion) -> Value {
    serde_json::to_value(s).unwrap_or(Value::Null)
}
