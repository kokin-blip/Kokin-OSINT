//! Search across a case.
//!
//! This is reference-workflow step 6. L0 through L2 all hold rows by now, and
//! until this crate existed none of them could be found without knowing an id.
//!
//! # The index is a projection, never a source of truth
//!
//! Nothing is knowable only from the index. Every row in `search_document` is
//! derived by a trigger from a row in L1 or L2 (migration 5), so the index can
//! be dropped and rebuilt from the case at any time, and [`rebuild`] does
//! exactly that. That is what makes it safe for the index to be lossy and
//! denormalised: it is an accelerator for finding evidence, not a place
//! evidence lives.
//!
//! # Relevance is not confidence
//!
//! [`Hit::relevance`] ranks *text similarity*, nothing else. A page that
//! mentions a name forty times outranks the registry filing that proves it,
//! and that is the correct behaviour for a search engine and a terrible basis
//! for a conclusion. Confidence in this product has seven dimensions and no
//! total (ADR-0007); relevance is not one of them and does not combine with
//! them. Anything rendering a hit should treat this number as an ordering, not
//! as a quality — and should not show it next to a confidence dimension where
//! the two could be read as the same kind of thing.
//!
//! # Searches are not written to the audit chain
//!
//! Deliberately. A record of every query an analyst typed is a record of every
//! suspicion they entertained and discarded, which is more sensitive than the
//! case itself and cannot be crypto-shredded per subject. Saved searches are a
//! separate, explicit act; see `docs/increments/009-search.md`.

use rusqlite::types::Value;
use rusqlite::Connection;

pub mod query;

pub use query::Query;

/// Errors this crate can produce.
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("the search index is corrupt and must be rebuilt: {0}")]
    IndexCorrupt(String),
}

pub type Result<T> = std::result::Result<T, SearchError>;

/// One search result.
#[derive(Debug, Clone)]
pub struct Hit {
    /// `observation`, `entity`, `identifier`, or `relationship` — the table the
    /// hit routes back to, so a result can always open its evidence.
    pub subject_kind: String,
    pub subject_id: String,
    /// What to show in a result list.
    pub title: String,
    /// The matching text in context, with matches bracketed.
    pub snippet: String,
    /// The row's own type: observation kind, entity type, identifier namespace.
    pub facet: String,
    /// True if an extractor rerun withdrew this observation (ADR-0006).
    ///
    /// Only ever true when the caller asked for withdrawn results. Anything
    /// displaying a hit with this set must say so: a withdrawn observation
    /// shown as a current one is the case asserting something it has retracted.
    pub superseded: bool,
    /// Text-similarity ordering only. See the crate note: this is not
    /// confidence, and lower is better (it is BM25).
    pub relevance: f64,
}

/// How to narrow a search.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    pub offset: usize,
    /// Restrict to these subject kinds. Empty means all of them.
    pub kinds: Vec<String>,
    /// Restrict to these facets. Empty means all of them.
    pub facets: Vec<String>,
    /// Include observations a rerun has withdrawn. Off by default: the common
    /// question is "what does the case say", not "what has it ever said".
    pub include_superseded: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
            kinds: Vec::new(),
            facets: Vec::new(),
            include_superseded: false,
        }
    }
}

/// Title matches are weighted above body matches: an entity named "Acme" is a
/// better answer to "acme" than a page that mentions it in passing.
const BM25: &str = "bm25(search_index, 10.0, 1.0)";

/// Run a search.
///
/// An empty or exclusion-only query returns no rows without touching the index
/// — see [`Query::to_fts5`] for why that is the only honest answer.
pub fn search(conn: &Connection, query: &Query, options: &SearchOptions) -> Result<Vec<Hit>> {
    let Some(expression) = query.to_fts5() else {
        return Ok(Vec::new());
    };

    let mut params: Vec<Value> = vec![Value::Text(expression)];
    let mut sql = format!(
        "SELECT d.subject_kind, d.subject_id, d.title,
                snippet(search_index, 1, '[', ']', '…', 12),
                d.facet, d.superseded, {BM25}
           FROM search_index
           JOIN search_document d ON d.id = search_index.rowid
          WHERE search_index MATCH ?1"
    );

    push_filters(&mut sql, &mut params, options);

    sql.push_str(&format!(" ORDER BY {BM25}"));
    let limit_at = params.len() + 1;
    sql.push_str(&format!(" LIMIT ?{} OFFSET ?{}", limit_at, limit_at + 1));
    params.push(Value::Integer(options.limit as i64));
    params.push(Value::Integer(options.offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        Ok(Hit {
            subject_kind: r.get(0)?,
            subject_id: r.get(1)?,
            title: r.get(2)?,
            snippet: r.get(3)?,
            facet: r.get(4)?,
            superseded: r.get::<_, i64>(5)? != 0,
            relevance: r.get(6)?,
        })
    })?;

    let mut hits = Vec::new();
    for hit in rows {
        hits.push(hit?);
    }
    Ok(hits)
}

/// How many rows a search would return, ignoring limit and offset.
pub fn count(conn: &Connection, query: &Query, options: &SearchOptions) -> Result<i64> {
    let Some(expression) = query.to_fts5() else {
        return Ok(0);
    };

    let mut params: Vec<Value> = vec![Value::Text(expression)];
    let mut sql = String::from(
        "SELECT COUNT(*)
           FROM search_index
           JOIN search_document d ON d.id = search_index.rowid
          WHERE search_index MATCH ?1",
    );
    push_filters(&mut sql, &mut params, options);

    Ok(conn.query_row(&sql, rusqlite::params_from_iter(params), |r| r.get(0))?)
}

/// What kinds of thing matched, and how many of each.
///
/// This is what turns a result list into a workbench: an analyst usually wants
/// "seventeen observations and one entity" before they want the seventeen.
pub fn facet_counts(
    conn: &Connection,
    query: &Query,
    options: &SearchOptions,
) -> Result<Vec<(String, String, i64)>> {
    let Some(expression) = query.to_fts5() else {
        return Ok(Vec::new());
    };

    let mut params: Vec<Value> = vec![Value::Text(expression)];
    let mut sql = String::from(
        "SELECT d.subject_kind, d.facet, COUNT(*)
           FROM search_index
           JOIN search_document d ON d.id = search_index.rowid
          WHERE search_index MATCH ?1",
    );
    push_filters(&mut sql, &mut params, options);
    sql.push_str(
        " GROUP BY d.subject_kind, d.facet ORDER BY COUNT(*) DESC, d.subject_kind, d.facet",
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The filters shared by every query above, appended with numbered parameters
/// so nothing user-supplied is ever interpolated into SQL.
fn push_filters(sql: &mut String, params: &mut Vec<Value>, options: &SearchOptions) {
    if !options.include_superseded {
        sql.push_str(" AND d.superseded = 0");
    }
    push_in_clause(sql, params, "d.subject_kind", &options.kinds);
    push_in_clause(sql, params, "d.facet", &options.facets);
}

fn push_in_clause(sql: &mut String, params: &mut Vec<Value>, column: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    let start = params.len() + 1;
    let placeholders: Vec<String> = (0..values.len())
        .map(|i| format!("?{}", start + i))
        .collect();
    sql.push_str(&format!(" AND {column} IN ({})", placeholders.join(",")));
    params.extend(values.iter().map(|v| Value::Text(v.clone())));
}

/// Rebuild the index from `search_document`.
///
/// Cheap insurance rather than a routine operation: because the index is a
/// projection, losing it costs time and nothing else.
pub fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute_batch("INSERT INTO search_index(search_index) VALUES('rebuild')")?;
    Ok(())
}

/// Ask FTS5 whether the index agrees with the text it was built from.
///
/// Worth running after a crash or a restore. A drifted index does not announce
/// itself — it just stops returning things.
pub fn check(conn: &Connection) -> Result<()> {
    conn.execute_batch("INSERT INTO search_index(search_index) VALUES('integrity-check')")
        .map_err(|e| SearchError::IndexCorrupt(e.to_string()))
}

/// Documents pointing at rows that no longer exist.
///
/// Always empty if the triggers are doing their job, which is exactly why it is
/// worth being able to ask: a hit that opens nothing is a result an analyst
/// cannot check, and the projection is only trustworthy if it can be audited
/// against the rows it claims to describe.
pub fn dangling_documents(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT d.subject_kind, d.subject_id
           FROM search_document d
          WHERE (d.subject_kind = 'observation'
                 AND NOT EXISTS (SELECT 1 FROM observation o WHERE o.id = d.subject_id))
             OR (d.subject_kind = 'entity'
                 AND NOT EXISTS (SELECT 1 FROM entity e WHERE e.id = d.subject_id))
             OR (d.subject_kind = 'identifier'
                 AND NOT EXISTS (SELECT 1 FROM identifier i WHERE i.id = d.subject_id))
             OR (d.subject_kind = 'relationship'
                 AND NOT EXISTS (SELECT 1 FROM relationship r WHERE r.id = d.subject_id))
          ORDER BY d.subject_kind, d.subject_id",
    )?;

    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
