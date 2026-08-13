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
//! # An empty result list is not a finding
//!
//! [`search`] answers with what the index holds, and the index holds only what
//! some transform managed to read. A case with an unopened PDF in it will
//! answer "no results" to a query the PDF contains, in exactly the same words
//! it uses for a query nothing in the world matches — and those are opposite
//! facts. [`coverage`] is how a caller tells them apart, and anything rendering
//! a result list is expected to render it too. See [`coverage::coverage`] for
//! why the figure is necessarily about the case rather than about the query.
//!
//! # Results are people, not rows
//!
//! An entity an analyst has merged into another one is still a row, still
//! indexed, and still matches its own name — because ADR-0008 never rewrites
//! it. Listing it beside its canonical entity would assert that two people
//! exist, which is exactly the reading the analyst rejected, so [`search`]
//! collapses a cluster to one hit and reports the survivor in
//! [`Hit::canonical_id`]. [`count`] and [`facet_counts`] collapse the same way;
//! a total that disagrees with the list under it is worse than no total.
//!
//! The collapse is a `LEFT JOIN` on `er_merge_map`, one indexed lookup per hit.
//! That is affordable only because migration 7 makes the map exactly one level
//! deep (D-020) — a chain would put a recursive walk on the hot path of every
//! query in this crate.
//!
//! # Searches are not written to the audit chain
//!
//! Deliberately. A record of every query an analyst typed is a record of every
//! suspicion they entertained and discarded, which is more sensitive than the
//! case itself and cannot be crypto-shredded per subject. Saved searches are a
//! separate, explicit act; see `docs/increments/009-search.md`.

use rusqlite::types::Value;
use rusqlite::Connection;

pub mod coverage;
pub mod query;

pub use coverage::{ArtifactCoverage, Coverage, CoverageState, Gap};
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
    /// Set when this hit is an entity that has been merged into another one,
    /// and holds the entity it now resolves to (ADR-0008).
    ///
    /// The hit still reports its own `subject_id`, because that is the row the
    /// text actually matched and the row whose evidence argued for the merge.
    /// A UI is expected to open the canonical entity and to say that it did:
    /// silently redirecting is how an analyst loses track of which of two
    /// records a name came from.
    pub canonical_id: Option<String>,
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
    /// Return one hit per resolved entity rather than one per entity row.
    ///
    /// **On by default**, and that is a judgement rather than a convenience.
    /// Two rows an analyst has already decided are one person, listed twice,
    /// assert that two people exist — and it is the reading the analyst
    /// explicitly rejected. Turning this off is the specialised request ("show
    /// me the rows, not the people"), so it is the one that must be asked for.
    pub resolve_merged: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
            kinds: Vec::new(),
            facets: Vec::new(),
            include_superseded: false,
            resolve_merged: true,
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
    let mut inner = matched_documents();
    push_filters(&mut inner, &mut params, options);

    let body = one_row_per(
        &inner,
        "subject_kind, subject_id, title, snippet, facet, superseded,
         relevance, resolved_id",
        options,
    );

    let limit_at = params.len() + 1;
    // `subject_id` breaks relevance ties. Unlike the run ordering in
    // `coverage`, an arbitrary winner here is not a wrong answer — two aliases
    // with identical BM25 are equally good representatives — so any stable key
    // will do, and stability is the whole requirement: without it the same
    // query returns a different alias each time it is run.
    let sql = format!(
        "{body} ORDER BY relevance, subject_id LIMIT ?{} OFFSET ?{}",
        limit_at,
        limit_at + 1
    );
    params.push(Value::Integer(options.limit as i64));
    params.push(Value::Integer(options.offset as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        let subject_id: String = r.get(1)?;
        let resolved_id: String = r.get(7)?;
        Ok(Hit {
            subject_kind: r.get(0)?,
            title: r.get(2)?,
            snippet: r.get(3)?,
            facet: r.get(4)?,
            superseded: r.get::<_, i64>(5)? != 0,
            relevance: r.get(6)?,
            canonical_id: (resolved_id != subject_id).then_some(resolved_id),
            subject_id,
        })
    })?;

    let mut hits = Vec::new();
    for hit in rows {
        hits.push(hit?);
    }
    Ok(hits)
}

/// The matched rows, with each one's current identity attached.
///
/// The `LEFT JOIN` is what makes an entity's identity a *read* rather than a
/// property of its row (ADR-0008). It costs one indexed lookup per hit, which
/// is affordable only because migration 7 guarantees the merge map is one level
/// deep — a chain would make this a recursive walk on the hot path.
fn matched_documents() -> String {
    format!(
        "SELECT d.subject_kind AS subject_kind,
                d.subject_id   AS subject_id,
                d.title        AS title,
                snippet(search_index, 1, '[', ']', '…', 12) AS snippet,
                d.facet        AS facet,
                d.superseded   AS superseded,
                {BM25}         AS relevance,
                COALESCE(m.canonical_id, d.subject_id) AS resolved_id
           FROM search_index
           JOIN search_document d ON d.id = search_index.rowid
           LEFT JOIN er_merge_map m
                  ON m.entity_id = d.subject_id
                 AND m.active = 1
                 AND d.subject_kind = 'entity'
          WHERE search_index MATCH ?1"
    )
}

/// The same match and the same join, without the parts only a result list
/// needs. Counting does not need a snippet, and building one per match is the
/// expensive half of a search.
fn matched_keys() -> String {
    format!(
        "SELECT d.subject_kind AS subject_kind,
                d.subject_id   AS subject_id,
                d.facet        AS facet,
                {BM25}         AS relevance,
                COALESCE(m.canonical_id, d.subject_id) AS resolved_id
           FROM search_index
           JOIN search_document d ON d.id = search_index.rowid
           LEFT JOIN er_merge_map m
                  ON m.entity_id = d.subject_id
                 AND m.active = 1
                 AND d.subject_kind = 'entity'
          WHERE search_index MATCH ?1"
    )
}

/// Reduce matched rows to one per resolved identity, keeping the best-ranked
/// alias as the representative.
///
/// Every read in this crate goes through here, and it has to: a `count` that
/// disagrees with the list it counts is worse than no count, and both numbers
/// are read by the same person in the same glance. `resolve_merged` off makes
/// this the identity function rather than a second code path.
///
/// Collapsing is done here, in SQL, rather than on the page `search` returns,
/// because doing it afterwards would silently shrink pages — ask for fifty and
/// get forty-seven because three were aliases of each other.
fn one_row_per(inner: &str, columns: &str, options: &SearchOptions) -> String {
    if !options.resolve_merged {
        return format!("WITH matched AS ({inner}) SELECT {columns} FROM matched");
    }
    format!(
        "WITH matched AS ({inner}),
              ranked AS (
                  SELECT *, ROW_NUMBER() OVER (
                      PARTITION BY subject_kind, resolved_id
                      ORDER BY relevance, subject_id
                  ) AS alias_rank
                    FROM matched
              )
         SELECT {columns} FROM ranked WHERE alias_rank = 1"
    )
}

/// How many rows a search would return, ignoring limit and offset.
pub fn count(conn: &Connection, query: &Query, options: &SearchOptions) -> Result<i64> {
    let Some(expression) = query.to_fts5() else {
        return Ok(0);
    };

    let mut params: Vec<Value> = vec![Value::Text(expression)];
    let mut inner = matched_keys();
    push_filters(&mut inner, &mut params, options);
    let sql = one_row_per(&inner, "COUNT(*)", options);

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
    let mut inner = matched_keys();
    push_filters(&mut inner, &mut params, options);
    // Counts the representatives, not the rows, so these sum to `count` and to
    // the length of a fully paged result list. A cluster is filed under its
    // representative's facet; where a merge spans facets that is a real
    // ambiguity, and picking the same row the list shows is the least
    // surprising of the available wrong-ish answers.
    let mut sql = one_row_per(&inner, "subject_kind, facet, COUNT(*)", options);
    sql.push_str(" GROUP BY subject_kind, facet ORDER BY COUNT(*) DESC, subject_kind, facet");

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
