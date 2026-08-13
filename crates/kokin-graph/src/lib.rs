//! L2: the analytical layer, and the one rule it lives under.
//!
//! This is reference-workflow step 5, and the layer everything before it exists
//! to support. `kokin-ingest` recorded what was collected, `kokin-extract`
//! recorded what a parser read; this crate records what a person concluded.
//!
//! # Nothing here exists without evidence
//!
//! Every public function that creates an L2 row takes the evidence for it in
//! the same call, and writes the `evidence_link` **before** the row itself.
//! That ordering is not a style choice: the database refuses the row otherwise
//! (`entity_requires_evidence` and its siblings in migration 4). An API that
//! let you create an entity now and attach evidence later would make "later"
//! optional, and later never comes.
//!
//! The consequence is the acceptance criterion "graph edges open their
//! supporting evidence" — not a screen someone remembered to build, but the
//! only shape the schema permits.
//!
//! # Confidence has seven dimensions and no total
//!
//! [`assess`] writes one row per dimension, against a scale version copied into
//! the case. There is no function here that combines them, because there is no
//! honest way to: a number formed from a reliable source, a shaky identifier
//! match and an untested temporal assumption launders the weakest input behind
//! an average, and tells an analyst nothing about what to go and check.

use rusqlite::Connection;

pub mod scales;

pub use scales::{scales, seed_scales, Scale, ScaleValue, INSUFFICIENT_INFORMATION};

/// Errors this crate can produce.
///
/// The distinctions are the ones a user-facing message depends on: "that scale
/// value does not exist" is a different problem from "a rule may not assign
/// that value", and collapsing them would hide the second behind the first.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("store error: {0}")]
    Store(#[from] kokin_store::StoreError),

    #[error("could not generate an id: {0}")]
    Random(String),

    #[error("a scale file shipped in this build is malformed: {reason}")]
    MalformedScale { reason: String },

    #[error("nothing in L2 exists without evidence: {what} was created with none")]
    Ungrounded { what: &'static str },

    #[error("unknown entity type '{key}' - add it to the registry first")]
    UnknownEntityType { key: String },

    #[error("no {kind} with id {id}")]
    SubjectMissing { kind: &'static str, id: String },

    #[error("'{value_key}' is not a value of scale {dimension} version {version}")]
    UnknownScaleValue {
        dimension: String,
        version: i64,
        value_key: String,
    },

    #[error("scale {dimension} is not loaded in this case")]
    UnknownScale { dimension: String },

    #[error(
        "actor '{actor}' is automated and may not assign '{value_key}': that value requires a human"
    )]
    MachineMayNotAssign { actor: String, value_key: String },
}

pub type Result<T> = std::result::Result<T, GraphError>;

/// What an `evidence_link` may point at.
///
/// Only L0 and L1 rows: an analytical row is never evidence for another
/// analytical row, because that is how a conclusion comes to support itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    Observation,
    Artifact,
    Capture,
}

impl EvidenceKind {
    fn as_str(self) -> &'static str {
        match self {
            EvidenceKind::Observation => "observation",
            EvidenceKind::Artifact => "artifact",
            EvidenceKind::Capture => "capture",
        }
    }
}

/// How a piece of evidence bears on the row it is linked to.
///
/// `Contradicts` is the reason this is an enum and not a boolean. Evidence that
/// argues *against* a conclusion has somewhere to live, which is exactly what a
/// graph-native model cannot represent — there, the edge either exists or it
/// does not, and the case for its absence is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRole {
    Supports,
    Contradicts,
    Context,
}

impl EvidenceRole {
    fn as_str(self) -> &'static str {
        match self {
            EvidenceRole::Supports => "supports",
            EvidenceRole::Contradicts => "contradicts",
            EvidenceRole::Context => "context",
        }
    }
}

/// One piece of evidence, and what it is being offered as.
#[derive(Debug, Clone, Copy)]
pub struct Grounding<'a> {
    pub kind: EvidenceKind,
    pub id: &'a str,
    pub role: EvidenceRole,
}

impl<'a> Grounding<'a> {
    /// The common case: an observation that supports the row.
    pub fn supporting_observation(id: &'a str) -> Self {
        Grounding {
            kind: EvidenceKind::Observation,
            id,
            role: EvidenceRole::Supports,
        }
    }
}

/// What an assessment or an evidence link is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectKind {
    Entity,
    Identifier,
    Relationship,
}

impl SubjectKind {
    fn as_str(self) -> &'static str {
        match self {
            SubjectKind::Entity => "entity",
            SubjectKind::Identifier => "identifier",
            SubjectKind::Relationship => "relationship",
        }
    }

    fn table(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Subject<'a> {
    pub kind: SubjectKind,
    pub id: &'a str,
}

/// The entity types a fresh case starts with.
///
/// `entity_type` is data, not schema (ADR-0006): this list is a seed, and
/// adding the rest is an `INSERT` plus a form descriptor, never a migration.
/// It is deliberately short — these are the types Phase 1's pipeline actually
/// produces or refers to, and a registry padded with types nothing can create
/// is a menu of dead ends.
const BUILTIN_ENTITY_TYPES: &[(&str, &str, &str)] = &[
    ("person", "Person", "A human being."),
    (
        "organisation",
        "Organisation",
        "A company, agency, group, or other collective actor.",
    ),
    (
        "email_address",
        "Email address",
        "An address as an actor in its own right, before it is attributed to anyone.",
    ),
    (
        "domain",
        "Domain",
        "A registered domain name, distinct from the site served on it.",
    ),
    (
        "url",
        "URL",
        "A specific document location, distinct from the domain hosting it.",
    ),
    (
        "username",
        "Username",
        "A handle on a platform. Not a person until something establishes that it is.",
    ),
    (
        "social_account",
        "Social account",
        "A specific account on a specific platform.",
    ),
    (
        "ip_address",
        "IP address",
        "A network address. Locates a registration, never a person.",
    ),
    ("phone_number", "Phone number", "A telephone number."),
    (
        "location",
        "Location",
        "A place. Precision is a separate dimension; see the geospatial scale.",
    ),
    (
        "document",
        "Document",
        "A filing, report, article, or page treated as a thing under discussion.",
    ),
    ("image", "Image", "A photograph or other still image."),
];

const REGISTRY_KEY: &str = "graph.entity_types_seeded";

/// Seed the entity type registry and the confidence scales.
///
/// Idempotent, and safe to call on every open. Called automatically by every
/// writer in this crate, so a caller cannot forget it and discover the omission
/// as a foreign-key error three layers down.
pub fn initialise(conn: &Connection) -> Result<()> {
    seed_entity_types(conn)?;
    scales::seed_scales(conn)?;
    Ok(())
}

fn seed_entity_types(conn: &Connection) -> Result<()> {
    let seeded: Option<String> = conn
        .query_row(
            "SELECT value FROM case_meta WHERE key = ?1",
            [REGISTRY_KEY],
            |r| r.get(0),
        )
        .ok();
    if seeded.is_some() {
        return Ok(());
    }

    for (key, label, description) in BUILTIN_ENTITY_TYPES {
        conn.execute(
            "INSERT OR IGNORE INTO entity_type (key, label, description, form_json, builtin)
             VALUES (?1, ?2, ?3, '{}', 1)",
            rusqlite::params![key, label, description],
        )?;
    }

    conn.execute(
        "INSERT INTO case_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![REGISTRY_KEY, BUILTIN_ENTITY_TYPES.len().to_string()],
    )?;

    Ok(())
}

/// A new analytical entity.
#[derive(Debug, Clone, Copy)]
pub struct NewEntity<'a> {
    pub type_key: &'a str,
    pub display_name: &'a str,
    pub notes: &'a str,
}

/// Create an entity, grounded in the evidence given.
///
/// The `evidence` slice may not be empty. It is checked here so the caller gets
/// a named error rather than a trigger message, but the check here is a
/// courtesy and the trigger is the guarantee — see
/// `an_entity_cannot_be_created_by_going_around_this_crate`.
pub fn create_entity(
    conn: &mut Connection,
    entity: NewEntity<'_>,
    evidence: &[Grounding<'_>],
) -> Result<String> {
    if evidence.is_empty() {
        return Err(GraphError::Ungrounded { what: "an entity" });
    }
    initialise(conn)?;

    let known: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entity_type WHERE key = ?1)",
        [entity.type_key],
        |r| r.get(0),
    )?;
    if !known {
        return Err(GraphError::UnknownEntityType {
            key: entity.type_key.to_string(),
        });
    }

    let id = new_id()?;
    let now = kokin_store::now_utc_rfc3339();
    let tx = conn.transaction()?;

    link_evidence(&tx, SubjectKind::Entity, &id, evidence, &now)?;
    tx.execute(
        "INSERT INTO entity (id, type_key, display_name, notes, created_utc, updated_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        rusqlite::params![id, entity.type_key, entity.display_name, entity.notes, now],
    )?;

    tx.commit()?;
    audit(conn, "entity.created", "entity", &id, evidence.len())?;
    Ok(id)
}

/// Attach an identifier to an entity, grounded in evidence.
///
/// `value` is stored exactly as the evidence gave it; the normalised form used
/// for equality is derived and stored alongside. Both are kept because a
/// normalisation that folds two distinct identifiers onto one string would
/// otherwise be invisible afterwards — and the identifier-match scale would
/// rate that collision as an exact match.
pub fn add_identifier(
    conn: &mut Connection,
    entity_id: &str,
    namespace: &str,
    value: &str,
    evidence: &[Grounding<'_>],
) -> Result<String> {
    if evidence.is_empty() {
        return Err(GraphError::Ungrounded {
            what: "an identifier",
        });
    }
    initialise(conn)?;
    require_exists(conn, SubjectKind::Entity, entity_id)?;

    let id = new_id()?;
    let now = kokin_store::now_utc_rfc3339();
    let normalized = normalise(namespace, value);
    let tx = conn.transaction()?;

    link_evidence(&tx, SubjectKind::Identifier, &id, evidence, &now)?;
    tx.execute(
        "INSERT INTO identifier (id, entity_id, namespace, value, normalized, created_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, entity_id, namespace, value, normalized, now],
    )?;

    tx.commit()?;
    audit(conn, "identifier.added", "identifier", &id, evidence.len())?;
    Ok(id)
}

/// Relate two entities, grounded in evidence.
///
/// The period is optional and stays optional. Most relationships are asserted
/// without one, and inventing a start date to fill the column would be a claim
/// the evidence does not make; the temporal-consistency scale exists to rate
/// that absence rather than paper over it.
pub fn relate(
    conn: &mut Connection,
    from_entity: &str,
    to_entity: &str,
    kind: &str,
    period: (Option<&str>, Option<&str>),
    evidence: &[Grounding<'_>],
) -> Result<String> {
    if evidence.is_empty() {
        return Err(GraphError::Ungrounded {
            what: "a relationship",
        });
    }
    initialise(conn)?;
    require_exists(conn, SubjectKind::Entity, from_entity)?;
    require_exists(conn, SubjectKind::Entity, to_entity)?;

    let id = new_id()?;
    let now = kokin_store::now_utc_rfc3339();
    let tx = conn.transaction()?;

    link_evidence(&tx, SubjectKind::Relationship, &id, evidence, &now)?;
    tx.execute(
        "INSERT INTO relationship (id, from_entity, to_entity, kind, started_utc, ended_utc, created_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, from_entity, to_entity, kind, period.0, period.1, now],
    )?;

    tx.commit()?;
    audit(
        conn,
        "relationship.created",
        "relationship",
        &id,
        evidence.len(),
    )?;
    Ok(id)
}

/// Record one dimension of confidence about one subject (ADR-0007).
///
/// There is exactly one row per (subject, dimension), so reassessing replaces
/// the value — the history of that change lives in the audit chain, which is
/// where the record of a judgement belongs, rather than in a pile of rows a UI
/// then has to decide between.
///
/// `contributing_factors_json` drives the "why" panel. A factor that is not
/// recorded cannot be displayed, which is what forces the reasoning to be
/// written down to have any effect on the outcome.
pub fn assess(
    conn: &mut Connection,
    subject: Subject<'_>,
    dimension: &str,
    value_key: &str,
    contributing_factors_json: &str,
    actor: &str,
) -> Result<String> {
    initialise(conn)?;
    require_exists(conn, subject.kind, subject.id)?;

    // Assess against the version this case holds, not the version this build
    // ships: a case opened by a newer build keeps assessing under the scale its
    // earlier assessments were made under until someone deliberately migrates.
    let version: i64 = conn
        .query_row(
            "SELECT MAX(version) FROM scale WHERE id = ?1",
            [dimension],
            |r| r.get::<_, Option<i64>>(0),
        )?
        .ok_or_else(|| GraphError::UnknownScale {
            dimension: dimension.to_string(),
        })?;

    let permitted: Option<bool> = conn
        .query_row(
            "SELECT machine_assignable FROM scale_value
              WHERE scale_id = ?1 AND scale_version = ?2 AND value_key = ?3",
            rusqlite::params![dimension, version, value_key],
            |r| r.get(0),
        )
        .ok();

    let Some(machine_assignable) = permitted else {
        return Err(GraphError::UnknownScaleValue {
            dimension: dimension.to_string(),
            version,
            value_key: value_key.to_string(),
        });
    };

    if is_automated(actor) && !machine_assignable {
        return Err(GraphError::MachineMayNotAssign {
            actor: actor.to_string(),
            value_key: value_key.to_string(),
        });
    }

    let id = new_id()?;
    let now = kokin_store::now_utc_rfc3339();

    conn.execute(
        "INSERT INTO assessment
            (id, subject_kind, subject_id, dimension, scale_version, value_key,
             contributing_factors_json, actor, assessed_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(subject_kind, subject_id, dimension) DO UPDATE SET
            scale_version = excluded.scale_version,
            value_key = excluded.value_key,
            contributing_factors_json = excluded.contributing_factors_json,
            actor = excluded.actor,
            assessed_utc = excluded.assessed_utc",
        rusqlite::params![
            id,
            subject.kind.as_str(),
            subject.id,
            dimension,
            version,
            value_key,
            contributing_factors_json,
            actor,
            now
        ],
    )?;

    kokin_store::append_audit_event(
        conn,
        &kokin_store::NewAuditEvent {
            actor,
            action: "assessment.recorded",
            subject_kind: Some(subject.kind.as_str()),
            subject_id: Some(subject.id),
            payload_json: &format!(
                r#"{{"dimension":"{dimension}","scale_version":{version},"value":"{value_key}"}}"#
            ),
        },
    )?;

    Ok(id)
}

/// One row of evidence under an L2 subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    pub link_id: String,
    pub evidence_kind: String,
    pub evidence_id: String,
    pub role: String,
}

/// The evidence under a subject: the read behind "open this edge and show me
/// why". Returned in insertion order so the first-cited evidence stays first.
pub fn evidence_for(conn: &Connection, subject: Subject<'_>) -> Result<Vec<EvidenceRef>> {
    let mut stmt = conn.prepare(
        "SELECT id, evidence_kind, evidence_id, role
           FROM evidence_link
          WHERE subject_kind = ?1 AND subject_id = ?2
          ORDER BY created_utc, id",
    )?;

    let rows = stmt.query_map(rusqlite::params![subject.kind.as_str(), subject.id], |r| {
        Ok(EvidenceRef {
            link_id: r.get(0)?,
            evidence_kind: r.get(1)?,
            evidence_id: r.get(2)?,
            role: r.get(3)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Every assessment recorded about a subject, one per dimension.
///
/// Returns them as a list, never a total. There is no `overall()` here and
/// there must not be one: see ADR-0007.
pub fn assessments_for(conn: &Connection, subject: Subject<'_>) -> Result<Vec<Assessment>> {
    let mut stmt = conn.prepare(
        "SELECT dimension, scale_version, value_key, contributing_factors_json, actor, assessed_utc
           FROM assessment
          WHERE subject_kind = ?1 AND subject_id = ?2
          ORDER BY dimension",
    )?;

    let rows = stmt.query_map(rusqlite::params![subject.kind.as_str(), subject.id], |r| {
        Ok(Assessment {
            dimension: r.get(0)?,
            scale_version: r.get(1)?,
            value_key: r.get(2)?,
            contributing_factors_json: r.get(3)?,
            actor: r.get(4)?,
            assessed_utc: r.get(5)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub dimension: String,
    pub scale_version: i64,
    pub value_key: String,
    pub contributing_factors_json: String,
    pub actor: String,
    pub assessed_utc: String,
}

// ---------------------------------------------------------------------------

/// Write the evidence links for a subject that does not exist yet.
///
/// Called before the row it grounds, because that is the only order the schema
/// accepts. Both statements are inside one transaction, so a failure between
/// them leaves neither.
fn link_evidence(
    tx: &rusqlite::Transaction<'_>,
    kind: SubjectKind,
    subject_id: &str,
    evidence: &[Grounding<'_>],
    now: &str,
) -> Result<()> {
    for item in evidence {
        tx.execute(
            "INSERT INTO evidence_link
                (id, subject_kind, subject_id, evidence_kind, evidence_id, role, created_utc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                new_id()?,
                kind.as_str(),
                subject_id,
                item.kind.as_str(),
                item.id,
                item.role.as_str(),
                now
            ],
        )?;
    }
    Ok(())
}

fn require_exists(conn: &Connection, kind: SubjectKind, id: &str) -> Result<()> {
    // The table name comes from a closed enum, never from a caller.
    let sql = format!(
        "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ?1)",
        kind.table()
    );
    let found: bool = conn.query_row(&sql, [id], |r| r.get(0))?;
    if found {
        Ok(())
    } else {
        Err(GraphError::SubjectMissing {
            kind: kind.as_str(),
            id: id.to_string(),
        })
    }
}

/// Whether an actor is a rule or a model rather than a person.
///
/// Same prefixes ADR-0008 uses to keep automated similarity from merging
/// entities. One convention, so an actor string means the same thing in the
/// merge map and in an assessment.
fn is_automated(actor: &str) -> bool {
    actor.starts_with("rule:") || actor.starts_with("ai:")
}

/// The normalised form an identifier is compared on.
///
/// Deliberately minimal. Aggressive folding — stripping dots, unicode
/// confusable mapping, plus-address removal — makes distinct identifiers
/// collide, and a collision here is not a near miss: it presents as an exact
/// match, which is the strongest value the identifier scale has.
///
/// Email is the one case with structure worth respecting: the domain is
/// case-insensitive by specification, the local part is not. Lowercasing the
/// whole address is near-universal and still, strictly, wrong, so only the
/// domain is folded.
fn normalise(namespace: &str, value: &str) -> String {
    let trimmed = value.trim();
    match namespace {
        "email_address" => match trimmed.rsplit_once('@') {
            Some((local, domain)) => format!("{local}@{}", domain.to_lowercase()),
            None => trimmed.to_string(),
        },
        "domain" | "url" | "ip_address" => trimmed.to_lowercase(),
        _ => trimmed.to_string(),
    }
}

fn audit(
    conn: &Connection,
    action: &str,
    subject_kind: &str,
    subject_id: &str,
    evidence_count: usize,
) -> Result<()> {
    kokin_store::append_audit_event(
        conn,
        &kokin_store::NewAuditEvent {
            actor: "user:local",
            action,
            subject_kind: Some(subject_kind),
            subject_id: Some(subject_id),
            payload_json: &format!(r#"{{"evidence_links":{evidence_count}}}"#),
        },
    )?;
    Ok(())
}

fn new_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| GraphError::Random(e.to_string()))?;
    Ok(kokin_store::blake3_hex(&bytes)[..32].to_string())
}
