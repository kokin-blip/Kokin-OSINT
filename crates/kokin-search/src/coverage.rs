//! What the case has not read.
//!
//! # Why this lives beside search
//!
//! Coverage is a fact about ingestion and extraction, not about the index, and
//! a tidier architecture would put it in a crate of its own. It is here because
//! of where it has to be *rendered*: beside a result list, at the moment
//! somebody is deciding what a case does and does not contain. A coverage
//! figure that a caller has to remember to go and fetch from somewhere else is
//! a coverage figure that does not get fetched, and the gap it would have shown
//! stays exactly as invisible as it was before this module existed.
//!
//! # Coverage is case-scoped, and cannot be query-scoped
//!
//! The report an analyst actually wants is "your search found three hits, and
//! two documents that were never read might have held a fourth". That report
//! cannot be produced by anything, ever: deciding whether an unread document
//! matches a query requires reading it, at which point it is not unread. So the
//! honest statement is the weaker, case-scoped one — *this case holds documents
//! that no search of it can reach* — and it must be attached to every result
//! list, not only to searches that happen to return nothing. A search that
//! returns forty hits from a case that is 60% read is more misleading than one
//! that returns none, because nobody interrogates a full page of results.
//!
//! # An absent gap is a claim
//!
//! [`CoverageState::Complete`] does not mean the document was understood. It
//! means the extractor that last read it reported no gap. An extractor that
//! silently drops what it cannot handle produces a case that reports itself
//! fully covered, so the value of everything here rests on transforms recording
//! their own shortfalls honestly — see `kokin_extract::record_coverage_gaps`.

use rusqlite::Connection;

use crate::Result;

/// How completely one artifact has been read, according to the most recent run
/// that tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageState {
    /// No run has ever taken this artifact as an input. Nothing in it is
    /// searchable, and nobody has decided that it should not be.
    NeverAttempted,
    /// A run took it and did not finish. Either it is being read right now, or
    /// a process died holding it — indistinguishable from here, and both mean
    /// the case's account of this document is not final.
    InProgress,
    /// A run took it and failed, with the reason it gave. The commonest reason
    /// today is a media type this build cannot read, which makes this the
    /// "we know, and we cannot" state rather than an error.
    Failed { error_code: Option<String> },
    /// A run succeeded and reported that some of the document was not covered.
    Partial,
    /// A run succeeded and reported no shortfall. The strongest statement this
    /// module can make, and weaker than it sounds — see the module note.
    Complete,
}

impl CoverageState {
    /// Whether a search of this case can reach everything in this document.
    pub fn is_complete(&self) -> bool {
        matches!(self, CoverageState::Complete)
    }
}

/// Something a run reported it did not cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gap {
    /// A stable code, for counting: `text_truncated`, `value_oversize`.
    pub gap_kind: String,
    /// The same thing in words, for showing.
    pub detail: String,
    /// How much, in [`Gap::unit`]. `None` when the run did not know, which is
    /// different from zero and must not be rendered as it.
    pub magnitude: Option<i64>,
    pub unit: Option<String>,
}

/// One artifact and how much of it the case has.
#[derive(Debug, Clone)]
pub struct ArtifactCoverage {
    pub artifact_id: String,
    pub media_type: String,
    pub state: CoverageState,
    /// Only ever from the latest run. An earlier run's gaps describe a reading
    /// that has been replaced, and carrying them forward would report a
    /// shortfall that a rerun has already fixed.
    pub gaps: Vec<Gap>,
}

/// How much of this case is searchable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    pub artifacts: i64,
    pub never_attempted: i64,
    pub in_progress: i64,
    pub failed: i64,
    pub partial: i64,
    pub complete: i64,
}

impl Coverage {
    /// Artifacts a search of this case cannot fully reach.
    pub fn incomplete(&self) -> i64 {
        self.never_attempted + self.in_progress + self.failed + self.partial
    }

    /// Whether a result list from this case can be shown without a caveat.
    ///
    /// An empty case is complete, and that is deliberate: it holds nothing, so
    /// it hides nothing. What makes an empty case misleading is a claim about
    /// the world drawn from it, which is a different control (ADR-0007's
    /// `insufficient_information`) at a different point in the workflow.
    pub fn is_complete(&self) -> bool {
        self.incomplete() == 0
    }
}

/// The most recent run over each artifact, and the state it left it in.
///
/// `started_utc` alone cannot order these. It is a second-resolution ISO
/// string, and re-extracting a case runs many transforms inside one second —
/// so ties are the normal case here, not an edge one.
///
/// The tie is broken on `rowid`, which is SQLite's insertion order and is
/// therefore genuinely monotonic in the order runs were opened. `transform_run`
/// is not `WITHOUT ROWID`, so it has one. A `VACUUM` may renumber it, but only
/// by copying rows in existing rowid order, so the *ordering* — the only thing
/// read here — survives.
///
/// Breaking the tie on `id` instead looks equivalent and is not: run ids are
/// random hex with no relation to time, so that ordering is a coin flip, and
/// this report would answer with a superseded run's verdict about half the
/// time. That is worse than having no report, because it is wrong in both
/// directions — a re-read document still shown as truncated, and a truncated
/// re-read shown as complete.
const LATEST_RUN: &str = "
    WITH latest AS (
        SELECT rs.subject_id AS artifact_id,
               r.id           AS run_id,
               r.status       AS status,
               r.error_code   AS error_code,
               ROW_NUMBER() OVER (
                   PARTITION BY rs.subject_id
                   ORDER BY r.started_utc DESC, r.rowid DESC
               ) AS n
          FROM run_subject rs
          JOIN transform_run r ON r.id = rs.run_id
         WHERE rs.subject_kind = 'artifact'
    )";

/// Count how much of this case a search can reach.
///
/// Cheap enough to call alongside every search: it touches no full-text index
/// and scans lineage, which is small next to the documents it describes.
pub fn coverage(conn: &Connection) -> Result<Coverage> {
    let sql = format!(
        "{LATEST_RUN}
         SELECT COALESCE(l.status, 'never'),
                EXISTS (SELECT 1 FROM coverage_gap g WHERE g.run_id = l.run_id),
                COUNT(*)
           FROM artifact a
           LEFT JOIN latest l ON l.artifact_id = a.id AND l.n = 1
          GROUP BY 1, 2"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)? != 0,
            r.get::<_, i64>(2)?,
        ))
    })?;

    let mut out = Coverage::default();
    for row in rows {
        let (status, has_gap, count) = row?;
        out.artifacts += count;
        match (status.as_str(), has_gap) {
            ("never", _) => out.never_attempted += count,
            ("failed", _) => out.failed += count,
            ("succeeded", true) => out.partial += count,
            ("succeeded", false) => out.complete += count,
            // 'running', and anything a future transform invents. Counted as
            // unfinished rather than dropped: a status this build does not
            // recognise is the last thing that should quietly become
            // "complete".
            _ => out.in_progress += count,
        }
    }
    Ok(out)
}

/// The artifacts behind [`Coverage::incomplete`], worst first.
///
/// Ordered so that the documents nothing has read come before the ones that
/// were read imperfectly, because that is the order in which doing something
/// about them pays off.
pub fn incomplete_artifacts(conn: &Connection, limit: usize) -> Result<Vec<ArtifactCoverage>> {
    let sql = format!(
        "{LATEST_RUN}
         SELECT a.id, a.media_type, COALESCE(l.status, 'never'), l.error_code, l.run_id
           FROM artifact a
           LEFT JOIN latest l ON l.artifact_id = a.id AND l.n = 1
          WHERE l.run_id IS NULL
             OR l.status <> 'succeeded'
             OR EXISTS (SELECT 1 FROM coverage_gap g WHERE g.run_id = l.run_id)
          ORDER BY CASE COALESCE(l.status, 'never')
                     WHEN 'never'     THEN 0
                     WHEN 'failed'    THEN 1
                     WHEN 'succeeded' THEN 3
                     ELSE 2
                   END,
                   a.id
          LIMIT ?1"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([limit as i64], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;

    let mut pending = Vec::new();
    for row in rows {
        pending.push(row?);
    }

    let mut out = Vec::with_capacity(pending.len());
    for (artifact_id, media_type, status, error_code, run_id) in pending {
        let gaps = match &run_id {
            Some(id) => gaps_for_run(conn, id)?,
            None => Vec::new(),
        };
        out.push(ArtifactCoverage {
            artifact_id,
            media_type,
            state: state_of(&status, error_code, !gaps.is_empty()),
            gaps,
        });
    }
    Ok(out)
}

/// How completely one named artifact has been read.
///
/// `None` when the case holds no such artifact, which is a different answer from
/// [`CoverageState::NeverAttempted`] and must not be folded into it: one says
/// nothing has read this document, the other says there is no document.
///
/// This exists because a document panel shows one artifact, and the alternative
/// — deriving the state in the caller from a run status it queried itself — is
/// how the same rule comes to have two definitions that agree until they do not.
pub fn artifact_coverage(conn: &Connection, artifact_id: &str) -> Result<Option<ArtifactCoverage>> {
    let sql = format!(
        "{LATEST_RUN}
         SELECT a.media_type, COALESCE(l.status, 'never'), l.error_code
           FROM artifact a
           LEFT JOIN latest l ON l.artifact_id = a.id AND l.n = 1
          WHERE a.id = ?1"
    );
    let row = conn.query_row(&sql, [artifact_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    });
    let (media_type, status, error_code) = match row {
        Ok(found) => found,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(other) => return Err(other.into()),
    };
    let gaps = gaps_for_artifact(conn, artifact_id)?;
    Ok(Some(ArtifactCoverage {
        artifact_id: artifact_id.to_string(),
        media_type,
        state: state_of(&status, error_code, !gaps.is_empty()),
        gaps,
    }))
}

/// The one place a run status becomes a coverage state.
///
/// `has_gap` is a parameter rather than an assumption. `incomplete_artifacts`
/// filters to rows that are already incomplete, so inside it a succeeded run is
/// necessarily [`CoverageState::Partial`] — and that reasoning is invisible at
/// the point the mapping is written, which is exactly how it gets copied
/// somewhere the filter does not apply and starts reporting fully-read documents
/// as partly read.
fn state_of(status: &str, error_code: Option<String>, has_gap: bool) -> CoverageState {
    match (status, has_gap) {
        ("never", _) => CoverageState::NeverAttempted,
        ("failed", _) => CoverageState::Failed { error_code },
        ("succeeded", true) => CoverageState::Partial,
        ("succeeded", false) => CoverageState::Complete,
        // 'running', and any status a future transform invents. Counted as
        // unfinished rather than dropped, for the reason `coverage` gives: a
        // status this build does not recognise is the last thing that should
        // quietly become "complete".
        _ => CoverageState::InProgress,
    }
}

/// Everything one artifact's latest run reported it missed.
///
/// Returns an empty list for an artifact nothing has read, which is correct and
/// worth saying out loud: no gaps recorded is not the same as nothing missing,
/// and only [`CoverageState`] distinguishes them.
pub fn gaps_for_artifact(conn: &Connection, artifact_id: &str) -> Result<Vec<Gap>> {
    let sql = format!(
        "{LATEST_RUN}
         SELECT l.run_id FROM latest l WHERE l.artifact_id = ?1 AND l.n = 1"
    );
    let run_id: Option<String> =
        conn.query_row(&sql, [artifact_id], |r| r.get(0))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;

    match run_id {
        Some(id) => gaps_for_run(conn, &id),
        None => Ok(Vec::new()),
    }
}

fn gaps_for_run(conn: &Connection, run_id: &str) -> Result<Vec<Gap>> {
    let mut stmt = conn.prepare(
        "SELECT gap_kind, detail, magnitude, unit
           FROM coverage_gap
          WHERE run_id = ?1
          ORDER BY gap_kind",
    )?;
    let rows = stmt.query_map([run_id], |r| {
        Ok(Gap {
            gap_kind: r.get(0)?,
            detail: r.get(1)?,
            magnitude: r.get(2)?,
            unit: r.get(3)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
