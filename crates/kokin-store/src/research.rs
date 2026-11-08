//! Case-encrypted research state. Results are append-only; UI work is revisioned.
use crate::{Result, StoreError};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
fn invalid(s: &str) -> StoreError {
    StoreError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, s))
}
fn encode(v: &Value) -> Result<String> {
    serde_json::to_string(v).map_err(|_| invalid("invalid research JSON"))
}
fn value(s: String) -> rusqlite::Result<Value> {
    serde_json::from_str(&s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
pub fn epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
fn audit(conn: &Connection, action: &str, id: &str, payload: &Value) -> Result<()> {
    crate::append_audit_event(
        conn,
        &crate::NewAuditEvent {
            actor: "user:local",
            action,
            subject_kind: Some("research"),
            subject_id: Some(id),
            payload_json: &encode(payload)?,
        },
    )?;
    Ok(())
}
pub fn begin(conn: &Connection, id: &str, plan: &Value, expected: usize) -> Result<()> {
    if expected == 0 || expected > 1500 || id.is_empty() || id.len() > 128 {
        return Err(invalid("research run exceeds 15 selectors × 100 sources"));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("INSERT INTO research_run(id,plan_json,status,created_utc,expected) VALUES(?1,?2,'running',?3,?4)",params![id,encode(plan)?,crate::now_utc_rfc3339(),expected as i64])?;
    audit(
        &tx,
        "research_started",
        id,
        &serde_json::json!({"expected":expected}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn append(
    conn: &Connection,
    run: &str,
    id: &str,
    cache_key: &str,
    payload: &Value,
) -> Result<()> {
    if payload["id"].as_str() != Some(id) || payload["run_id"].as_str() != Some(run) {
        return Err(invalid("research result identity mismatch"));
    }
    let outcome = payload["outcome"]
        .as_str()
        .ok_or_else(|| invalid("result outcome required"))?;
    let artifact = payload["artifact_id"].as_str();
    if artifact.is_none() && matches!(outcome, "found" | "not_found" | "taken" | "available") {
        return Err(invalid(
            "definitive research result needs captured evidence",
        ));
    }
    let encoded = encode(payload)?;
    if encoded.len() > 8 * 1024 * 1024 {
        return Err(invalid("result exceeds 8 MiB"));
    }
    let tx = conn.unchecked_transaction()?;
    let (status, expected, completed): (String, i64, i64) = tx.query_row(
        "SELECT status,expected,completed FROM research_run WHERE id=?1",
        [run],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if completed >= expected {
        return Err(invalid("research result count exceeds the authorized run"));
    }
    if status != "running" {
        return Err(invalid("research run is no longer accepting results"));
    }
    tx.execute("INSERT INTO research_result(id,run_id,cache_key,outcome,artifact_id,payload_json,created_epoch) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,run,cache_key,outcome,artifact,encoded,epoch()])?;
    tx.execute(
        "UPDATE research_run SET completed=completed+1 WHERE id=?1",
        [run],
    )?;
    audit(
        &tx,
        "research_result",
        id,
        &serde_json::json!({"run_id":run,"outcome":outcome,"artifact_id":artifact}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn finish(conn: &Connection, id: &str, status: &str) -> Result<()> {
    if !matches!(
        status,
        "completed" | "partial" | "cancelled" | "interrupted"
    ) {
        return Err(invalid("invalid terminal research state"));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE research_run SET status=?2,finished_utc=?3 WHERE id=?1 AND status='running'",
        params![id, status, crate::now_utc_rfc3339()],
    )?;
    audit(
        &tx,
        "research_finished",
        id,
        &serde_json::json!({"status":status}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn recover(conn: &Connection) -> Result<()> {
    // Never retry interrupted billable work or resume monitors just because a case opened.
    conn.execute(
        "UPDATE research_run SET status='interrupted',finished_utc=?1 WHERE status='running'",
        [crate::now_utc_rfc3339()],
    )?;
    Ok(())
}
pub fn has_failures(conn: &Connection, run: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM research_result WHERE run_id=?1 AND outcome IN('error','blocked','unsupported'))",[run],|r|r.get(0))?)
}
pub fn cached(conn: &Connection, key: &str) -> Result<Option<Value>> {
    cached_for(conn, key, 8 * 3600)
}
/// Reviewed sources may shorten freshness, never extend the eight-hour ceiling.
pub fn cached_for(conn: &Connection, key: &str, max_age_seconds: u32) -> Result<Option<Value>> {
    if max_age_seconds == 0 {
        return Ok(None);
    }
    let raw:Option<String>=conn.query_row("SELECT r.payload_json FROM research_result r JOIN artifact a ON a.id=r.artifact_id JOIN blob b ON b.content_hash=a.content_hash WHERE r.cache_key=?1 AND r.created_epoch>?2 AND r.created_epoch<=?3 AND COALESCE(json_extract(r.payload_json,'$.from_cache'),0)=0 AND r.outcome IN('found','not_found','taken','available') AND b.wrapped_key IS NOT NULL ORDER BY r.sequence DESC LIMIT 1",params![key,epoch()-i64::from(max_age_seconds.min(8*3600)),epoch()],|r|r.get(0)).optional()?;
    raw.map(value).transpose().map_err(Into::into)
}
#[derive(Debug, Serialize)]
pub struct Page {
    pub rows: Vec<Value>,
    pub total: i64,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResultFilter {
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub sources: Vec<String>,
    /// Exact scalar leaf key. Empty selects any key when a pattern is supplied.
    pub field: String,
    /// SQLite GLOB: case-sensitive *, ?, and [] wildcards; no SQL interpolation.
    pub pattern: String,
    pub module: String,
    /// Empty, true, false, or unknown. Missing/null/string booleans are unknown.
    pub verified: String,
    pub private: String,
    pub min_age: Option<u16>,
    pub max_age: Option<u16>,
    pub date_from: String,
    pub date_to: String,
    /// Empty defaults to creation_date; last_seen is also supported.
    pub date_field: String,
}
#[derive(Debug, Serialize)]
pub struct ResultPage {
    #[serde(flatten)]
    pub page: Page,
    pub run_total: i64,
    /// Facets describe the whole run, including outcomes hidden by this filter.
    pub source_counts: std::collections::BTreeMap<String, i64>,
    pub outcome_counts: std::collections::BTreeMap<String, i64>,
}
const RESULT_PROJECTION: &str = "WITH projected AS (
 SELECT r.sequence,r.payload_json,
 COALESCE(json_extract(r.payload_json,'$.source'),'unknown') AS source,
 CASE WHEN r.artifact_id IS NOT NULL AND (a.id IS NULL OR b.wrapped_key IS NULL) THEN 'unknown' ELSE r.outcome END AS effective_outcome,
 CASE WHEN r.artifact_id IS NOT NULL AND (a.id IS NULL OR b.wrapped_key IS NULL) THEN 0 ELSE 1 END AS readable,
 CASE WHEN r.artifact_id IS NOT NULL AND (a.id IS NULL OR b.wrapped_key IS NULL) THEN '{}' WHEN json_type(r.payload_json,'$.fields') IN('object','array') THEN json_extract(r.payload_json,'$.fields') ELSE '{}' END AS search_fields
 FROM research_result r LEFT JOIN artifact a ON a.id=r.artifact_id
 LEFT JOIN blob b ON b.content_hash=a.content_hash WHERE r.run_id=?1)";
// All card predicates live in one EXISTS: different accounts cannot satisfy different
// constraints. Legacy raw fields are searchable but never interpreted as typed metadata.
const RESULT_PREDICATE: &str = "(?2='' OR effective_outcome=?2)
 AND (json_array_length(?3)=0 OR source IN(SELECT value FROM json_each(?3)))
 AND (?4='{}' OR EXISTS (
 SELECT 1 FROM json_each(CASE WHEN json_type(search_fields,'$.cards')='array' AND json_array_length(search_fields,'$.cards')>0 THEN json_extract(search_fields,'$.cards') ELSE '[{}]' END) AS card
 WHERE card.type='object'
 AND (json_extract(?4,'$.module')='' OR json_extract(card.value,'$.module')=json_extract(?4,'$.module'))
 AND (json_extract(?4,'$.verified')='' OR CASE WHEN json_type(card.value,'$.fields.verified') IN('true','false') THEN json_type(card.value,'$.fields.verified') ELSE 'unknown' END=json_extract(?4,'$.verified'))
 AND (json_extract(?4,'$.private')='' OR CASE WHEN json_type(card.value,'$.fields.private') IN('true','false') THEN json_type(card.value,'$.fields.private') ELSE 'unknown' END=json_extract(?4,'$.private'))
 AND (json_extract(?4,'$.min_age') IS NULL OR (json_type(card.value,'$.fields.age')='integer' AND json_extract(card.value,'$.fields.age') BETWEEN 0 AND 150 AND json_extract(card.value,'$.fields.age')>=json_extract(?4,'$.min_age')))
 AND (json_extract(?4,'$.max_age') IS NULL OR (json_type(card.value,'$.fields.age')='integer' AND json_extract(card.value,'$.fields.age') BETWEEN 0 AND 150 AND json_extract(card.value,'$.fields.age')<=json_extract(?4,'$.max_age')))
 AND ((json_extract(?4,'$.date_from')='' AND json_extract(?4,'$.date_to')='') OR (
 json_type(card.value,'$.fields.'||json_extract(?4,'$.date_field'))='text'
 AND date(substr(json_extract(card.value,'$.fields.'||json_extract(?4,'$.date_field')),1,10),'+0 days')=substr(json_extract(card.value,'$.fields.'||json_extract(?4,'$.date_field')),1,10)
 AND (json_extract(?4,'$.date_from')='' OR substr(json_extract(card.value,'$.fields.'||json_extract(?4,'$.date_field')),1,10)>=json_extract(?4,'$.date_from'))
 AND (json_extract(?4,'$.date_to')='' OR substr(json_extract(card.value,'$.fields.'||json_extract(?4,'$.date_field')),1,10)<=json_extract(?4,'$.date_to'))))
 AND ((json_extract(?4,'$.field')='' AND json_extract(?4,'$.pattern')='') OR EXISTS (
 SELECT 1 FROM json_tree(CASE WHEN json_type(search_fields,'$.cards')='array' AND json_array_length(search_fields,'$.cards')>0 THEN COALESCE(json_extract(card.value,'$.fields'),'{}') ELSE search_fields END) AS leaf
 WHERE leaf.type NOT IN('object','array','null')
 AND (json_extract(?4,'$.field')='' OR CAST(leaf.key AS TEXT)=json_extract(?4,'$.field'))
 AND (json_extract(?4,'$.pattern')='' OR (CASE leaf.type WHEN 'true' THEN 'true' WHEN 'false' THEN 'false' ELSE CAST(leaf.atom AS TEXT) END) GLOB json_extract(?4,'$.pattern'))))))";
fn valid_filter_date(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = s[..4].parse::<u16>() else {
        return false;
    };
    let Ok(month) = s[5..7].parse::<usize>() else {
        return false;
    };
    let Ok(day) = s[8..].parse::<u16>() else {
        return false;
    };
    let days = [
        31,
        if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    year > 0 && (1..=12).contains(&month) && day > 0 && day <= days[month - 1]
}
pub fn results(conn: &Connection, run: &str, offset: usize) -> Result<Page> {
    Ok(filtered_results(conn, run, offset, &ResultFilter::default())?.page)
}
/// Filter before paging. Projection and counts apply the same shredding rules.
pub fn filtered_results(
    conn: &Connection,
    run: &str,
    offset: usize,
    filter: &ResultFilter,
) -> Result<ResultPage> {
    if offset > 1_000_000
        || filter.sources.len() > 100
        || filter.sources.iter().any(|s| s.is_empty() || s.len() > 128)
    {
        return Err(invalid("result filter or offset exceeds bound"));
    }
    if !matches!(
        filter.outcome.as_str(),
        "" | "found"
            | "not_found"
            | "taken"
            | "available"
            | "invalid"
            | "unknown"
            | "blocked"
            | "error"
            | "unsupported"
    ) {
        return Err(invalid("unknown research outcome filter"));
    }
    if filter.field.len() > 128
        || filter.pattern.len() > 512
        || filter.module.len() > 128
        || !matches!(filter.verified.as_str(), "" | "true" | "false" | "unknown")
        || !matches!(filter.private.as_str(), "" | "true" | "false" | "unknown")
        || !matches!(
            filter.date_field.as_str(),
            "" | "creation_date" | "last_seen"
        )
        || !valid_filter_date(&filter.date_from)
        || !valid_filter_date(&filter.date_to)
        || (!filter.date_from.is_empty()
            && !filter.date_to.is_empty()
            && filter.date_from > filter.date_to)
        || filter.min_age.is_some_and(|n| n > 150)
        || filter.max_age.is_some_and(|n| n > 150)
        || matches!((filter.min_age,filter.max_age),(Some(min),Some(max)) if min>max)
        || [&filter.field, &filter.pattern, &filter.module]
            .iter()
            .any(|s| s.chars().any(char::is_control))
    {
        return Err(invalid("invalid normalized research field filter"));
    }
    let sources =
        serde_json::to_string(&filter.sources).map_err(|_| invalid("invalid source filter"))?;
    let detailed = if filter.field.is_empty()
        && filter.pattern.is_empty()
        && filter.module.is_empty()
        && filter.verified.is_empty()
        && filter.private.is_empty()
        && filter.min_age.is_none()
        && filter.max_age.is_none()
        && filter.date_from.is_empty()
        && filter.date_to.is_empty()
    {
        "{}".to_string()
    } else {
        let mut normalized = filter.clone();
        if normalized.date_field.is_empty() {
            normalized.date_field = "creation_date".into();
        }
        serde_json::to_string(&normalized).map_err(|_| invalid("invalid detailed result filter"))?
    };
    // A read transaction keeps the page, total and facets coherent while workers commit.
    let tx = conn.unchecked_transaction()?;
    let mut source_counts = std::collections::BTreeMap::new();
    let mut outcome_counts = std::collections::BTreeMap::new();
    let mut run_total = 0;
    {
        let mut stmt = tx.prepare(&format!("{RESULT_PROJECTION} SELECT source,effective_outcome,COUNT(*) FROM projected GROUP BY source,effective_outcome"))?;
        let facets = stmt.query_map([run], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for facet in facets {
            let (source, outcome, count) = facet?;
            *source_counts.entry(source).or_insert(0) += count;
            *outcome_counts.entry(outcome).or_insert(0) += count;
            run_total += count;
        }
    }
    let total = tx.query_row(
        &format!("{RESULT_PROJECTION} SELECT COUNT(*) FROM projected WHERE {RESULT_PREDICATE}"),
        params![run, filter.outcome, sources, detailed],
        |r| r.get(0),
    )?;
    let rows = {
        let mut stmt = tx.prepare(&format!("{RESULT_PROJECTION} SELECT payload_json,readable FROM projected WHERE {RESULT_PREDICATE} ORDER BY sequence LIMIT 50 OFFSET ?5"))?;
        let rows = stmt
            .query_map(
                params![run, filter.outcome, sources, detailed, offset as i64],
                |r| {
                    let mut v = value(r.get(0)?)?;
                    if r.get::<_, i64>(1)? == 0 {
                        v["fields"] = serde_json::json!({});
                        v["url"] = "".into();
                        v["outcome"] = "unknown".into();
                        v["limitation"] =
                            "Source was shredded or is unavailable; cached fields are hidden."
                                .into();
                    }
                    Ok(v)
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    tx.commit()?;
    Ok(ResultPage {
        page: Page {
            rows,
            total,
            offset,
            limit: 50,
        },
        run_total,
        source_counts,
        outcome_counts,
    })
}
/// Explicit selected findings; every ID must belong to this case. Preserve caller order.
pub fn selected(conn: &Connection, ids: &[String]) -> Result<Vec<Value>> {
    if ids.is_empty() || ids.len() > 10_000 {
        return Err(invalid("select 1–10,000 research results"));
    }
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut bytes = 0;
    let mut query=conn.prepare("SELECT r.payload_json,CASE WHEN a.id IS NULL OR b.wrapped_key IS NULL THEN 0 ELSE 1 END FROM research_result r LEFT JOIN artifact a ON a.id=r.artifact_id LEFT JOIN blob b ON b.content_hash=a.content_hash WHERE r.id=?1")?;
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let (mut v, readable): (Value, i64) =
            query.query_row([id], |r| Ok((value(r.get(0)?)?, r.get(1)?)))?;
        if readable == 0 && !v["artifact_id"].is_null() {
            v["fields"] = serde_json::json!({});
            v["url"] = "".into();
            v["outcome"] = "unknown".into();
            v["limitation"] = "Source is shredded or unavailable; source fields withheld.".into();
        }
        bytes += encode(&v)?.len();
        if bytes > 32 * 1024 * 1024 {
            return Err(invalid(
                "selected findings exceed 32 MiB; select a smaller batch",
            ));
        }
        rows.push(v);
    }
    Ok(rows)
}
pub fn runs(conn: &Connection, offset: usize) -> Result<Page> {
    if offset > 1_000_000 {
        return Err(invalid("run offset exceeds bound"));
    }
    let total = conn.query_row("SELECT COUNT(*) FROM research_run", [], |r| r.get(0))?;
    let mut stmt=conn.prepare("SELECT id,plan_json,status,created_utc,finished_utc,expected,completed FROM research_run ORDER BY created_utc DESC,rowid DESC LIMIT 50 OFFSET ?1")?;
    let rows=stmt.query_map([offset as i64],|r|Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"plan":value(r.get(1)?)?,"status":r.get::<_,String>(2)?,"created_utc":r.get::<_,String>(3)?,"finished_utc":r.get::<_,Option<String>>(4)?,"expected":r.get::<_,i64>(5)?,"completed":r.get::<_,i64>(6)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Page {
        rows,
        total,
        offset,
        limit: 50,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub id: String,
    pub revision: i64,
    pub kind: String,
    pub name: String,
    pub payload: Value,
    pub archived: bool,
    pub updated_utc: String,
}
fn doc_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Document> {
    Ok(Document {
        id: r.get(0)?,
        revision: r.get(1)?,
        kind: r.get(2)?,
        name: r.get(3)?,
        payload: value(r.get(4)?)?,
        archived: r.get(5)?,
        updated_utc: r.get(6)?,
    })
}
pub fn document(conn: &Connection, id: &str) -> Result<Option<Document>> {
    conn.query_row("SELECT id,revision,kind,name,payload_json,archived,updated_utc FROM research_document WHERE id=?1 ORDER BY revision DESC LIMIT 1",[id],doc_row).optional().map_err(Into::into)
}
pub fn documents(conn: &Connection, kind: &str) -> Result<Vec<Document>> {
    let mut s=conn.prepare("SELECT d.id,d.revision,d.kind,d.name,d.payload_json,d.archived,d.updated_utc FROM research_document d WHERE kind=?1 AND archived=0 AND revision=(SELECT MAX(revision) FROM research_document WHERE id=d.id) ORDER BY d.updated_utc DESC,d.rowid DESC LIMIT 1000")?;
    let rows = s
        .query_map([kind], doc_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
pub fn save_document(conn: &Connection, input: &Document) -> Result<Document> {
    let encoded = encode(&input.payload)?;
    // Import previews retain up to 20,000 reviewed rows plus normalization metadata.
    // Their command boundary additionally validates row/cell/origin limits.
    let max_bytes = if input.kind == "saved_search" && input.payload["purpose"] == "import_session"
    {
        8 * 1024 * 1024
    } else {
        2 * 1024 * 1024
    };
    if !matches!(
        input.kind.as_str(),
        "collection"
            | "tool_preferences"
            | "saved_search"
            | "monitor"
            | "suppression"
            | "dossier"
            | "annotations"
            | "crawl"
            | "template"
    ) || input.id.is_empty()
        || input.id.len() > 128
        || !input
            .id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        || input.name.trim().is_empty()
        || input.name.chars().count() > 120
        || encoded.len() > max_bytes
        || input.payload["version"] != 1
        || input.revision < 0
    {
        return Err(invalid("invalid revisioned research document"));
    }
    if input.kind == "collection" {
        let ids = input.payload["result_ids"]
            .as_array()
            .ok_or_else(|| invalid("collection needs result IDs"))?;
        if ids.len() > 10_000 {
            return Err(invalid("collection exceeds 10,000 results"));
        }
        for id in ids {
            let found: i64 = conn.query_row(
                "SELECT COUNT(*) FROM research_result WHERE id=?1",
                [id.as_str().ok_or_else(|| invalid("invalid result ID"))?],
                |r| r.get(0),
            )?;
            if found != 1 {
                return Err(invalid("collection result is not in this case"));
            }
        }
    }
    let tx = conn.unchecked_transaction()?;
    let previous = document(&tx, &input.id)?;
    if previous.as_ref().map_or(0, |p| p.revision) != input.revision
        || previous.as_ref().is_some_and(|p| p.kind != input.kind)
    {
        return Err(invalid("document changed; reload before saving"));
    }
    let mut saved = input.clone();
    saved.revision += 1;
    saved.updated_utc = crate::now_utc_rfc3339();
    tx.execute("INSERT INTO research_document(id,revision,kind,name,payload_json,archived,updated_utc) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![saved.id,saved.revision,saved.kind,saved.name,encoded,saved.archived,saved.updated_utc])?;
    audit(
        &tx,
        "research_document_saved",
        &saved.id,
        &serde_json::json!({"kind":saved.kind,"revision":saved.revision}),
    )?;
    tx.commit()?;
    Ok(saved)
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::migrations::migrate(&c).unwrap();
        c
    }
    #[test]
    fn ten_thousand_case_results_keep_bounded_pages_and_selected_export() {
        let c = db();
        let start = std::time::Instant::now();
        let tx = c.unchecked_transaction().unwrap();
        for run in 0..10 {
            tx.execute("INSERT INTO research_run(id,plan_json,status,created_utc,expected,completed) VALUES(?1,'{}','completed','2026-10-20T00:00:00Z',1000,1000)",[format!("run-{run}")]).unwrap();
        }
        let ids = (0..10_000)
            .map(|n| format!("result-{n}"))
            .collect::<Vec<_>>();
        for (n, id) in ids.iter().enumerate() {
            let payload=serde_json::json!({"id":id,"run_id":format!("run-{}",n/1000),"artifact_id":null,"outcome":"unknown","fields":{},"source":"synthetic","selector":{"kind":"username","original":"synthetic","normalized":"synthetic"}}).to_string();
            tx.execute("INSERT INTO research_result(id,run_id,cache_key,outcome,payload_json,created_epoch) VALUES(?1,?2,'synthetic','unknown',?3,0)",params![id,format!("run-{}",n/1000),payload]).unwrap();
        }
        tx.commit().unwrap();
        let page = results(&c, "run-9", 950).unwrap();
        assert_eq!(page.total, 1000);
        assert_eq!(page.rows.len(), 50);
        assert_eq!(page.rows[0]["id"], "result-9950");
        let findings = selected(&c, &ids).unwrap();
        assert_eq!(findings.len(), 10_000);
        assert_eq!(findings[9999]["id"], "result-9999");
        assert!(selected(&c, &["other-case".into()]).is_err());
        eprintln!(
            "research_10k_projection_ms={} arch={} scope=in-memory-SQLite-synthetic-results",
            start.elapsed().as_millis(),
            std::env::consts::ARCH
        );
    }
    #[test]
    fn filters_cover_the_whole_run_and_facets_use_effective_outcomes() {
        let c = db();
        begin(&c, "filters", &serde_json::json!({}), 150).unwrap();
        for n in 0..150 {
            let source = if n < 100 { "github" } else { "hackernews" };
            let outcome = if n % 3 == 0 { "blocked" } else { "unknown" };
            let id = format!("r-{n}");
            append(&c,"filters",&id,"none",&serde_json::json!({"id":id,"run_id":"filters","source":source,"outcome":outcome,"artifact_id":null})).unwrap();
        }
        let filter = ResultFilter {
            outcome: "blocked".into(),
            sources: vec!["hackernews".into()],
            ..Default::default()
        };
        let page = filtered_results(&c, "filters", 0, &filter).unwrap();
        assert_eq!(page.page.total, 16);
        assert_eq!(page.run_total, 150);
        assert_eq!(page.source_counts["github"], 100);
        assert_eq!(page.outcome_counts["blocked"], 50);
        assert_eq!(page.page.rows[0]["id"], "r-102");
        assert!(page
            .page
            .rows
            .iter()
            .all(|r| r["source"] == "hackernews" && r["outcome"] == "blocked"));
        assert!(filtered_results(&c, "filters", 50, &filter)
            .unwrap()
            .page
            .rows
            .is_empty());
        assert!(filtered_results(
            &c,
            "filters",
            0,
            &ResultFilter {
                outcome: "all' OR 1=1".into(),
                sources: vec![],
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(
            filtered_results(
                &c,
                "filters",
                0,
                &ResultFilter {
                    outcome: String::new(),
                    sources: vec!["github' OR 1=1".into()],
                    ..Default::default()
                }
            )
            .unwrap()
            .page
            .total,
            0
        );
    }
    fn put_fields(c: &Connection, run: &str, id: &str, fields: Value, artifact: Option<&str>) {
        append(c,run,id,"none",&serde_json::json!({"id":id,"run_id":run,"source":"test","outcome":"unknown","artifact_id":artifact,"fields":fields})).unwrap();
    }
    #[test]
    fn normalized_filters_correlate_cards_and_preserve_unknown() {
        let c = db();
        begin(&c, "cards", &serde_json::json!({}), 4).unwrap();
        put_fields(
            &c,
            "cards",
            "split",
            serde_json::json!({"cards":[
                {"module":"alpha","fields":{"verified":true,"private":false,"age":30,"username":"Alpha","creation_date":"2020-01-02"}},
                {"module":"beta","fields":{"verified":false,"private":true,"age":60,"username":"Beta","creation_date":"2025-01-02"}}
            ]}),
            None,
        );
        put_fields(
            &c,
            "cards",
            "joined",
            serde_json::json!({"cards":[{"module":"alpha","fields":{"verified":true,"private":true,"age":60,"username":"Beta","creation_date":"2025-01-02T12:30:00Z","last_seen":"2026-10-20"}}]}),
            None,
        );
        put_fields(
            &c,
            "cards",
            "unknown",
            serde_json::json!({"cards":[{"module":"alpha","fields":{"verified":"false","private":null,"age":"60","creation_date":"2026-02-30"}}]}),
            None,
        );
        put_fields(
            &c,
            "cards",
            "legacy",
            serde_json::json!({"verified":true,"nested":{"username":"Beta"}}),
            None,
        );
        let f = ResultFilter {
            module: "alpha".into(),
            verified: "true".into(),
            private: "true".into(),
            min_age: Some(50),
            field: "username".into(),
            pattern: "B?ta".into(),
            date_from: "2024-01-01".into(),
            date_to: "2025-12-31".into(),
            ..Default::default()
        };
        let page = filtered_results(&c, "cards", 0, &f).unwrap();
        assert_eq!(page.page.total, 1);
        assert_eq!(page.page.rows[0]["id"], "joined");
        let f = ResultFilter {
            module: "alpha".into(),
            verified: "unknown".into(),
            private: "unknown".into(),
            ..Default::default()
        };
        assert_eq!(
            filtered_results(&c, "cards", 0, &f).unwrap().page.rows[0]["id"],
            "unknown"
        );
        let f = ResultFilter {
            verified: "false".into(),
            ..Default::default()
        };
        assert_eq!(filtered_results(&c, "cards", 0, &f).unwrap().page.total, 1);
        let f = ResultFilter {
            verified: "unknown".into(),
            field: "username".into(),
            pattern: "Beta".into(),
            ..Default::default()
        };
        let page = filtered_results(&c, "cards", 0, &f).unwrap();
        assert_eq!(page.page.total, 1);
        assert_eq!(page.page.rows[0]["id"], "legacy");
        let f = ResultFilter {
            date_field: "last_seen".into(),
            date_from: "2026-10-20".into(),
            ..Default::default()
        };
        assert_eq!(filtered_results(&c, "cards", 0, &f).unwrap().page.total, 1);
        let f = ResultFilter {
            date_from: "2026-02-01".into(),
            date_to: "2026-03-01".into(),
            ..Default::default()
        };
        assert_eq!(filtered_results(&c, "cards", 0, &f).unwrap().page.total, 0);
        let f = ResultFilter {
            pattern: "beta".into(),
            ..Default::default()
        };
        assert_eq!(filtered_results(&c, "cards", 0, &f).unwrap().page.total, 0);
        // GLOB is case-sensitive.
    }
    #[test]
    fn detailed_filters_page_after_matching_and_withhold_shredded_fields() {
        let c = db();
        begin(&c, "details", &serde_json::json!({}), 124).unwrap();
        for n in 0..120 {
            put_fields(
                &c,
                "details",
                &format!("r{n}"),
                serde_json::json!({"cards":[{"module":"account","fields":{"age":if n>=60 {30}else{20},"private":false}}]}),
                None,
            );
        }
        c.execute_batch("INSERT INTO blob VALUES ('filterhash',10,NULL,NULL,'2026-10-20T00:00:00Z',NULL); INSERT INTO artifact VALUES ('shredded-filter','filterhash','application/json',10,'2026-10-20T00:00:00Z');").unwrap();
        put_fields(
            &c,
            "details",
            "shredded",
            serde_json::json!({"cards":[{"module":"secret","fields":{"username":"hidden","verified":true}}]}),
            Some("shredded-filter"),
        );
        put_fields(
            &c,
            "details",
            "raw",
            serde_json::json!({"nested":{"username":"raw-user"}}),
            None,
        );
        put_fields(
            &c,
            "details",
            "malformed",
            serde_json::json!({"cards":[null,"not a card",5]}),
            None,
        );
        put_fields(
            &c,
            "details",
            "scalar-fields",
            serde_json::json!("unstructured"),
            None,
        );
        let f = ResultFilter {
            min_age: Some(30),
            max_age: Some(30),
            ..Default::default()
        };
        let first = filtered_results(&c, "details", 0, &f).unwrap();
        let last = filtered_results(&c, "details", 50, &f).unwrap();
        assert_eq!(first.page.total, 60);
        assert_eq!(first.page.rows.len(), 50);
        assert_eq!(first.page.rows[0]["id"], "r60");
        assert_eq!(last.page.rows.len(), 10);
        assert_eq!(last.page.rows[0]["id"], "r110");
        assert_eq!(last.run_total, 124);
        for f in [
            ResultFilter {
                module: "secret".into(),
                ..Default::default()
            },
            ResultFilter {
                field: "username".into(),
                pattern: "hidden".into(),
                ..Default::default()
            },
            ResultFilter {
                verified: "true".into(),
                ..Default::default()
            },
        ] {
            assert_eq!(
                filtered_results(&c, "details", 0, &f).unwrap().page.total,
                0
            );
        }
        let f = ResultFilter {
            field: "username".into(),
            pattern: "raw-*".into(),
            ..Default::default()
        };
        assert_eq!(
            filtered_results(&c, "details", 0, &f).unwrap().page.total,
            1
        );
        let f = ResultFilter {
            verified: "unknown".into(),
            ..Default::default()
        };
        let page = filtered_results(&c, "details", 100, &f).unwrap();
        assert!(page
            .page
            .rows
            .iter()
            .any(|r| r["id"] == "shredded" && r["fields"] == serde_json::json!({})));
    }
    #[test]
    fn detailed_filter_bounds_and_legacy_deserialization() {
        let c = db();
        let old: ResultFilter =
            serde_json::from_str(r#"{"outcome":"unknown","sources":[]}"#).unwrap();
        assert!(old.field.is_empty());
        for f in [
            ResultFilter {
                field: "x".repeat(129),
                ..Default::default()
            },
            ResultFilter {
                pattern: "x".repeat(513),
                ..Default::default()
            },
            ResultFilter {
                module: "x".repeat(129),
                ..Default::default()
            },
            ResultFilter {
                verified: "FALSE".into(),
                ..Default::default()
            },
            ResultFilter {
                private: "null".into(),
                ..Default::default()
            },
            ResultFilter {
                min_age: Some(151),
                ..Default::default()
            },
            ResultFilter {
                min_age: Some(40),
                max_age: Some(20),
                ..Default::default()
            },
            ResultFilter {
                date_from: "2026-02-29".into(),
                ..Default::default()
            },
            ResultFilter {
                date_from: "2024-12-31".into(),
                date_to: "2024-01-01".into(),
                ..Default::default()
            },
            ResultFilter {
                date_field: "publication_date".into(),
                ..Default::default()
            },
            ResultFilter {
                pattern: "a\n".into(),
                ..Default::default()
            },
        ] {
            assert!(filtered_results(&c, "none", 0, &f).is_err());
        }
        for date in ["2024-02-29", "2000-02-29", "2026-10-20"] {
            assert!(valid_filter_date(date));
        }
        for date in [
            "1900-02-29",
            "0000-01-01",
            "2024-00-01",
            "2024-04-31",
            "2024-13-01",
            "2024-01-00",
            "é24-01-01",
        ] {
            assert!(!valid_filter_date(date));
        }
        let f = ResultFilter {
            field: "x' OR 1=1".into(),
            pattern: "*".into(),
            ..Default::default()
        };
        assert_eq!(filtered_results(&c, "none", 0, &f).unwrap().page.total, 0);
    }
    #[test]
    fn only_import_sessions_allow_eight_mib_documents() {
        let c = db();
        let mut document = Document {
            id: "import-large".into(),
            revision: 0,
            kind: "saved_search".into(),
            name: "Reviewed import".into(),
            payload: serde_json::json!({"version":1,"purpose":"import_session","preview":"x".repeat(2*1024*1024)}),
            archived: false,
            updated_utc: String::new(),
        };
        assert!(save_document(&c, &document).is_ok());
        document.id = "too-large".into();
        document.payload["preview"] = "x".repeat(8 * 1024 * 1024).into();
        assert!(save_document(&c, &document).is_err());
        document.payload["preview"] = "x".repeat(2 * 1024 * 1024).into();
        document.payload["purpose"] = "query".into();
        assert!(save_document(&c, &document).is_err());
        document.payload["purpose"] = "import_session".into();
        document.kind = "template".into();
        assert!(save_document(&c, &document).is_err());
    }
    #[test]
    fn document_revisions_and_case_membership_are_enforced() {
        let c = db();
        let mut d = Document {
            id: "prefs".into(),
            revision: 0,
            kind: "tool_preferences".into(),
            name: "Tools".into(),
            payload: serde_json::json!({"version":1}),
            archived: false,
            updated_utc: String::new(),
        };
        assert_eq!(save_document(&c, &d).unwrap().revision, 1);
        assert!(save_document(&c, &d).is_err());
        d.id = "collection".into();
        d.kind = "collection".into();
        d.payload = serde_json::json!({"version":1,"result_ids":["other-case"]});
        assert!(save_document(&c, &d).is_err());
        assert!(c.execute("DELETE FROM research_document", []).is_err());
    }
    #[test]
    fn partial_runs_preserve_results_and_interrupted_work_is_not_retried() {
        let c = db();
        begin(&c, "run", &serde_json::json!({}), 2).unwrap();
        let result =
            serde_json::json!({"id":"r1","run_id":"run","outcome":"blocked","artifact_id":null});
        append(&c, "run", "r1", "key", &result).unwrap();
        assert_eq!(results(&c, "run", 0).unwrap().total, 1);
        assert!(cached(&c, "key").unwrap().is_none());
        recover(&c).unwrap();
        assert_eq!(runs(&c, 0).unwrap().rows[0]["status"], "interrupted");
        assert!(append(&c, "run", "r2", "key", &result).is_err());
        assert!(c
            .execute("UPDATE research_result SET outcome='found'", [])
            .is_err());
    }
}
