//! Schema migrations.
//!
//! SQLite has no migration framework, so this is an explicit, tested strategy
//! (ADR-0003): `PRAGMA user_version` is the schema version, migrations are an
//! ordered list applied in one transaction each, and a golden-schema test pins
//! the result so a migration cannot drift from what a fresh database produces.
//!
//! A case written by a *newer* build is refused by name rather than opened and
//! silently misread — the alternative is an older build writing rows a newer
//! schema expects to be shaped differently.

use rusqlite::Connection;

use crate::{Result, StoreError};

/// Each entry is applied once, in order, and its index+1 becomes the resulting
/// `user_version`. **Never edit a migration that has shipped** — append a new
/// one, or existing cases diverge from new ones with no way to tell.
const MIGRATIONS: &[&str] = &[
    // 1: the minimum a case needs to identify itself. The three-layer evidence
    // schema (ADR-0006) lands in the increment that has something to store.
    r#"
    CREATE TABLE case_meta (
        key   TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL
    ) STRICT;

    -- Append-only record of what happened in this case, chained by
    -- BLAKE3(prev_event_hash || canonical_cbor(payload)). Tamper-EVIDENT only:
    -- see crates/kokin-store/src/audit.rs and ADR-0014 for what that does and
    -- does not mean.
    CREATE TABLE audit_event (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        occurred_utc    TEXT NOT NULL,
        actor           TEXT NOT NULL,
        action          TEXT NOT NULL,
        subject_kind    TEXT,
        subject_id      TEXT,
        payload_json    TEXT NOT NULL DEFAULT '{}',
        prev_event_hash BLOB NOT NULL,
        event_hash      BLOB NOT NULL
    ) STRICT;

    -- The chain is only meaningful if hashes are unique; a duplicate would
    -- make two different histories verify.
    CREATE UNIQUE INDEX audit_event_hash ON audit_event(event_hash);

    CREATE INDEX audit_event_occurred ON audit_event(occurred_utc);
    "#,
    // 2: layers L0 (provenance) and L1 (lineage) from ADR-0006.
    //
    // L0 rows are facts about what was collected and are APPEND-ONLY, enforced
    // by triggers rather than by convention. L2 (entity, relationship, claim)
    // is mutable and arrives with the increment that creates entities.
    r#"
    -- ---------------------------------------------------------------------
    -- L0: provenance. Append-only. Never updated, never deleted.
    -- ---------------------------------------------------------------------

    -- Where evidence came from. canonical_locator is the identity used for
    -- deduplication; raw_locator is verbatim and is never edited, because
    -- editing it would falsify the record of what was actually fetched.
    CREATE TABLE source (
        id                TEXT PRIMARY KEY NOT NULL,
        kind              TEXT NOT NULL,
        raw_locator       TEXT NOT NULL,
        canonical_locator TEXT NOT NULL,
        first_seen_utc    TEXT NOT NULL
    ) STRICT;

    CREATE INDEX source_canonical ON source(canonical_locator);

    -- Content-addressed bytes. The bytes live in blobs/ (kokin-blob); this row
    -- carries the hash, the size, and the wrapped per-blob key.
    --
    -- wrapped_key is nullable ON PURPOSE: crypto-shredding sets it to NULL.
    -- The row, hash, size, and every derivation edge survive, so the record
    -- that this evidence existed is preserved while the bytes become
    -- permanently unreadable. See docs/limitations/deletion.md.
    CREATE TABLE blob (
        content_hash    TEXT PRIMARY KEY NOT NULL,
        size_bytes      INTEGER NOT NULL,
        wrapped_key     BLOB,
        wrapped_nonce   BLOB,
        stored_utc      TEXT NOT NULL,
        shredded_utc    TEXT
    ) STRICT;

    -- One retrieval. capture_completeness records what KIND of snapshot this
    -- is, so a static-HTML-only fetch of a JavaScript-heavy page is never
    -- mistaken for the full page a user would have seen.
    CREATE TABLE capture (
        id                   TEXT PRIMARY KEY NOT NULL,
        source_id            TEXT NOT NULL REFERENCES source(id),
        content_hash         TEXT REFERENCES blob(content_hash),
        requested_utc        TEXT NOT NULL,
        http_status          INTEGER,
        request_json         TEXT NOT NULL DEFAULT '{}',
        response_headers_json TEXT NOT NULL DEFAULT '{}',
        redirect_chain_json  TEXT NOT NULL DEFAULT '[]',
        capture_completeness TEXT NOT NULL,
        from_fixture         INTEGER NOT NULL DEFAULT 0,
        connector            TEXT NOT NULL,
        job_id               TEXT NOT NULL
    ) STRICT;

    CREATE INDEX capture_source ON capture(source_id);

    -- A typed thing derived from a capture or a file: an HTML document, an
    -- image, a PDF. Media type is what was observed, not what was claimed.
    CREATE TABLE artifact (
        id            TEXT PRIMARY KEY NOT NULL,
        content_hash  TEXT NOT NULL REFERENCES blob(content_hash),
        media_type    TEXT NOT NULL,
        byte_length   INTEGER NOT NULL,
        created_utc   TEXT NOT NULL
    ) STRICT;

    CREATE INDEX artifact_hash ON artifact(content_hash);

    -- ---------------------------------------------------------------------
    -- L1: lineage. Which code produced what, from what.
    -- ---------------------------------------------------------------------

    -- One execution of one transform. code_version and params_hash are what
    -- make a rerun after an upgrade distinguishable from the original run,
    -- which is the whole basis of observation supersession (ADR-0006).
    CREATE TABLE transform_run (
        id              TEXT PRIMARY KEY NOT NULL,
        transform_name  TEXT NOT NULL,
        transform_version TEXT NOT NULL,
        code_version    TEXT NOT NULL,
        params_hash     TEXT NOT NULL,
        started_utc     TEXT NOT NULL,
        finished_utc    TEXT,
        status          TEXT NOT NULL,
        error_code      TEXT,
        error_detail    TEXT
    ) STRICT;

    -- The lineage edge: this output came from that input, via that run.
    CREATE TABLE derivation (
        id             TEXT PRIMARY KEY NOT NULL,
        run_id         TEXT NOT NULL REFERENCES transform_run(id),
        input_kind     TEXT NOT NULL,
        input_id       TEXT NOT NULL,
        output_kind    TEXT NOT NULL,
        output_id      TEXT NOT NULL
    ) STRICT;

    CREATE INDEX derivation_output ON derivation(output_kind, output_id);
    CREATE INDEX derivation_input ON derivation(input_kind, input_id);

    -- ---------------------------------------------------------------------
    -- Append-only enforcement.
    --
    -- In the database, not in application code, because application code is
    -- where this kind of guarantee quietly erodes. A future contributor who
    -- "just needs to fix one row" gets an error naming the reason instead of
    -- silently rewriting provenance.
    --
    -- blob is the deliberate exception: crypto-shredding must be able to clear
    -- wrapped_key. The trigger below permits exactly that transition and
    -- nothing else.
    -- ---------------------------------------------------------------------

    CREATE TRIGGER source_is_append_only BEFORE UPDATE ON source
    BEGIN
        SELECT RAISE(ABORT, 'source is append-only: provenance cannot be edited');
    END;

    CREATE TRIGGER source_no_delete BEFORE DELETE ON source
    BEGIN
        SELECT RAISE(ABORT, 'source is append-only: provenance cannot be deleted');
    END;

    CREATE TRIGGER capture_is_append_only BEFORE UPDATE ON capture
    BEGIN
        SELECT RAISE(ABORT, 'capture is append-only: provenance cannot be edited');
    END;

    CREATE TRIGGER capture_no_delete BEFORE DELETE ON capture
    BEGIN
        SELECT RAISE(ABORT, 'capture is append-only: provenance cannot be deleted');
    END;

    CREATE TRIGGER artifact_is_append_only BEFORE UPDATE ON artifact
    BEGIN
        SELECT RAISE(ABORT, 'artifact is append-only: provenance cannot be edited');
    END;

    CREATE TRIGGER artifact_no_delete BEFORE DELETE ON artifact
    BEGIN
        SELECT RAISE(ABORT, 'artifact is append-only: provenance cannot be deleted');
    END;

    CREATE TRIGGER derivation_is_append_only BEFORE UPDATE ON derivation
    BEGIN
        SELECT RAISE(ABORT, 'derivation is append-only: lineage cannot be edited');
    END;

    CREATE TRIGGER derivation_no_delete BEFORE DELETE ON derivation
    BEGIN
        SELECT RAISE(ABORT, 'derivation is append-only: lineage cannot be deleted');
    END;

    -- A blob row may only ever change by being shredded. Any other update is
    -- rewriting the record of what was collected.
    CREATE TRIGGER blob_only_shred BEFORE UPDATE ON blob
    WHEN NOT (
        NEW.content_hash = OLD.content_hash
        AND NEW.size_bytes = OLD.size_bytes
        AND NEW.stored_utc = OLD.stored_utc
        AND NEW.wrapped_key IS NULL
        AND NEW.wrapped_nonce IS NULL
    )
    BEGIN
        SELECT RAISE(ABORT, 'blob is append-only except for crypto-shredding');
    END;

    CREATE TRIGGER blob_no_delete BEFORE DELETE ON blob
    BEGIN
        SELECT RAISE(ABORT, 'blob rows survive shredding: delete the key, not the row');
    END;

    -- transform_run is the one L1 table that legitimately mutates, because a
    -- run starts as 'running' and later becomes 'succeeded' or 'failed'. Only
    -- that completion is allowed; the identity of what ran is fixed.
    CREATE TRIGGER transform_run_only_completes BEFORE UPDATE ON transform_run
    WHEN NOT (
        NEW.id = OLD.id
        AND NEW.transform_name = OLD.transform_name
        AND NEW.transform_version = OLD.transform_version
        AND NEW.code_version = OLD.code_version
        AND NEW.params_hash = OLD.params_hash
        AND NEW.started_utc = OLD.started_utc
        AND OLD.status = 'running'
    )
    BEGIN
        SELECT RAISE(ABORT, 'a transform_run may only be completed, not rewritten');
    END;
    "#,
    // 3: the rest of L1 — the observation itself (ADR-0006).
    //
    // Migration 2 recorded that a transform ran and what it consumed. This
    // records what it *said*. Kept separate because it shipped separately, and
    // an applied migration is never edited.
    r#"
    -- A single extracted value, and exactly where in the artifact it came
    -- from. locator_json is what makes an observation checkable by a human:
    -- without it the value is an assertion, with it the reader can go and look.
    --
    -- value is stored verbatim, including hostile content. Nothing in the
    -- storage layer sanitises it, because sanitising on write would destroy the
    -- evidence; escaping is the renderer's job, at the point of display.
    CREATE TABLE observation (
        id            TEXT PRIMARY KEY NOT NULL,
        artifact_id   TEXT NOT NULL REFERENCES artifact(id),
        run_id        TEXT NOT NULL REFERENCES transform_run(id),
        kind          TEXT NOT NULL,
        value         TEXT NOT NULL,
        locator_json  TEXT NOT NULL,
        observed_utc  TEXT NOT NULL
    ) STRICT;

    CREATE INDEX observation_artifact ON observation(artifact_id);
    CREATE INDEX observation_run ON observation(run_id);
    CREATE INDEX observation_kind ON observation(kind, value);

    -- Rerunning a parser after an upgrade emits NEW observations; the old ones
    -- gain a row here (ADR-0006). They are never edited or deleted, because an
    -- upgrade must not be able to silently rewrite the basis of a conclusion an
    -- analyst already accepted.
    --
    -- superseded_by is nullable on purpose: a newer extractor that no longer
    -- makes an observation the old one made is withdrawing it, and "this is no
    -- longer observed" is a different fact from "this was replaced by that".
    CREATE TABLE observation_supersession (
        superseded_id  TEXT PRIMARY KEY NOT NULL REFERENCES observation(id),
        superseded_by  TEXT REFERENCES observation(id),
        run_id         TEXT NOT NULL REFERENCES transform_run(id),
        recorded_utc   TEXT NOT NULL
    ) STRICT;

    CREATE INDEX observation_supersession_by ON observation_supersession(superseded_by);

    -- Observations are L1 facts about what a parser saw, so they are
    -- append-only for the same reason provenance is.
    CREATE TRIGGER observation_is_append_only BEFORE UPDATE ON observation
    BEGIN
        SELECT RAISE(ABORT, 'observation is append-only: rerun the extractor, do not edit what it saw');
    END;

    CREATE TRIGGER observation_no_delete BEFORE DELETE ON observation
    BEGIN
        SELECT RAISE(ABORT, 'observation is append-only: supersede it, do not delete it');
    END;

    CREATE TRIGGER observation_supersession_is_append_only
    BEFORE UPDATE ON observation_supersession
    BEGIN
        SELECT RAISE(ABORT, 'a supersession is a historical fact and cannot be edited');
    END;

    CREATE TRIGGER observation_supersession_no_delete
    BEFORE DELETE ON observation_supersession
    BEGIN
        SELECT RAISE(ABORT, 'a supersession is a historical fact and cannot be deleted');
    END;
    "#,
    // 4: L2 — analysis (ADR-0006), and the assessment machinery of ADR-0007.
    //
    // L2 is MUTABLE, and that is the point: an analyst's reading of the evidence
    // is supposed to change as more evidence arrives. What may not change is the
    // evidence underneath it, which L0 and L1 already guarantee.
    //
    // The one rule this layer enforces for itself is ADR-0006's: nothing in L2
    // exists without an evidence_link. It is a trigger rather than a convention
    // because it is the mechanism behind "graph edges open their supporting
    // evidence" — that acceptance criterion is a consequence of the schema, not
    // a screen someone has to remember to build.
    r#"
    -- ---------------------------------------------------------------------
    -- The entity type registry.
    --
    -- entity_type is DATA, not schema (ADR-0006). Adding the remaining ~25
    -- entity types is an INSERT and a form descriptor, never a migration, which
    -- is the whole reason "25 entity types" is not a Phase 1 burden. The
    -- alternative - a table per type, or a type enum in a CHECK - makes every
    -- new type a schema change and a table rebuild.
    -- ---------------------------------------------------------------------
    CREATE TABLE entity_type (
        key          TEXT PRIMARY KEY NOT NULL,
        label        TEXT NOT NULL,
        description  TEXT NOT NULL,
        -- Describes the per-type fields a UI should offer. Empty until there
        -- is a UI; the column exists so adding one is still not a migration.
        form_json    TEXT NOT NULL DEFAULT '{}',
        builtin      INTEGER NOT NULL DEFAULT 0
    ) STRICT;

    -- ---------------------------------------------------------------------
    -- The scales an assessment may use (ADR-0007).
    --
    -- Seeded from docs/data-model/scales/*.yaml, and stored IN THE CASE so the
    -- case is self-describing: an assessment made under version 1 of a scale
    -- still renders correctly when opened by a build that ships version 2,
    -- because the definitions it was made under travel with it. Without this,
    -- editing a scale silently reinterprets every existing assessment.
    -- ---------------------------------------------------------------------
    CREATE TABLE scale (
        id              TEXT NOT NULL,
        version         INTEGER NOT NULL,
        title           TEXT NOT NULL,
        status          TEXT NOT NULL,
        question        TEXT NOT NULL,
        -- The authored file, verbatim. Calibration examples, threshold
        -- rationale and known failure modes are what make a scale meaningful
        -- rather than an opinion with a number attached, so they are carried
        -- rather than summarised.
        source_yaml     TEXT NOT NULL,
        loaded_utc      TEXT NOT NULL,
        PRIMARY KEY (id, version)
    ) STRICT;

    CREATE TABLE scale_value (
        scale_id           TEXT NOT NULL,
        scale_version      INTEGER NOT NULL,
        value_key          TEXT NOT NULL,
        label              TEXT NOT NULL,
        definition         TEXT NOT NULL,
        -- Authored order, so a UI renders the scale as it was written. Note
        -- that insufficient_information is deliberately FIRST and outside the
        -- ordering: "we have not established this" is not a weak value.
        ordinal            INTEGER NOT NULL,
        machine_assignable INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (scale_id, scale_version, value_key),
        FOREIGN KEY (scale_id, scale_version) REFERENCES scale(id, version)
    ) STRICT;

    -- ---------------------------------------------------------------------
    -- L2: analysis.
    -- ---------------------------------------------------------------------

    CREATE TABLE entity (
        id           TEXT PRIMARY KEY NOT NULL,
        type_key     TEXT NOT NULL REFERENCES entity_type(key),
        display_name TEXT NOT NULL,
        notes        TEXT NOT NULL DEFAULT '',
        created_utc  TEXT NOT NULL,
        updated_utc  TEXT NOT NULL
    ) STRICT;

    CREATE INDEX entity_by_type ON entity(type_key);

    -- value is what the evidence said; normalized is what equality is judged
    -- on. Both are kept, because a normalisation that turns two distinct
    -- identifiers into one string would otherwise be undetectable after the
    -- fact - and identifier_match rates such a collision as an exact match.
    CREATE TABLE identifier (
        id           TEXT PRIMARY KEY NOT NULL,
        entity_id    TEXT NOT NULL REFERENCES entity(id),
        namespace    TEXT NOT NULL,
        value        TEXT NOT NULL,
        normalized   TEXT NOT NULL,
        created_utc  TEXT NOT NULL
    ) STRICT;

    CREATE INDEX identifier_by_entity ON identifier(entity_id);
    CREATE INDEX identifier_lookup ON identifier(namespace, normalized);

    -- started_utc and ended_utc are nullable because most relationships are
    -- asserted without a period, and inventing one would be a claim the
    -- evidence does not support. temporal_consistency rates that absence.
    CREATE TABLE relationship (
        id           TEXT PRIMARY KEY NOT NULL,
        from_entity  TEXT NOT NULL REFERENCES entity(id),
        to_entity    TEXT NOT NULL REFERENCES entity(id),
        kind         TEXT NOT NULL,
        started_utc  TEXT,
        ended_utc    TEXT,
        created_utc  TEXT NOT NULL
    ) STRICT;

    CREATE INDEX relationship_from ON relationship(from_entity);
    CREATE INDEX relationship_to ON relationship(to_entity);

    -- The join between an analytical row and the evidence under it.
    --
    -- subject is polymorphic and therefore cannot carry a foreign key. It is
    -- deliberately NOT constrained to a list of kinds: each L2 table enforces
    -- its own requirement below by naming its own kind, so a future table adds
    -- a trigger rather than forcing a rebuild of this one.
    --
    -- role is CHECKed because ADR-0006 fixes the three values. 'contradicts' is
    -- the one that matters: evidence arguing AGAINST a conclusion has a place
    -- to live, which is precisely what graph-native tools cannot represent.
    CREATE TABLE evidence_link (
        id            TEXT PRIMARY KEY NOT NULL,
        subject_kind  TEXT NOT NULL,
        subject_id    TEXT NOT NULL,
        evidence_kind TEXT NOT NULL CHECK (evidence_kind IN ('observation', 'artifact', 'capture')),
        evidence_id   TEXT NOT NULL,
        role          TEXT NOT NULL CHECK (role IN ('supports', 'contradicts', 'context')),
        created_utc   TEXT NOT NULL
    ) STRICT;

    CREATE INDEX evidence_link_subject ON evidence_link(subject_kind, subject_id);
    CREATE INDEX evidence_link_evidence ON evidence_link(evidence_kind, evidence_id);

    -- One row per dimension per subject (ADR-0007).
    --
    -- There is NO composite column here, and there must never be one, because a
    -- column that exists will eventually be displayed. A single number formed
    -- from a reliable source, a shaky identifier match and an untested temporal
    -- assumption tells an analyst nothing about which one to go and check.
    --
    -- The composite foreign key is what makes the scales load-bearing rather
    -- than decorative: a value that is not in the scale it claims cannot be
    -- written at all.
    CREATE TABLE assessment (
        id                        TEXT PRIMARY KEY NOT NULL,
        subject_kind              TEXT NOT NULL,
        subject_id                TEXT NOT NULL,
        dimension                 TEXT NOT NULL,
        scale_version             INTEGER NOT NULL,
        value_key                 TEXT NOT NULL,
        -- Drives the UI's "why" panel. A factor that is not recorded cannot be
        -- displayed, which forces the scoring rules to be honest: anything that
        -- influenced the assessment has to be written down to have any effect.
        contributing_factors_json TEXT NOT NULL DEFAULT '[]',
        actor                     TEXT NOT NULL,
        assessed_utc              TEXT NOT NULL,
        FOREIGN KEY (dimension, scale_version, value_key)
            REFERENCES scale_value(scale_id, scale_version, value_key)
    ) STRICT;

    CREATE UNIQUE INDEX assessment_one_per_dimension
        ON assessment(subject_kind, subject_id, dimension);

    -- ---------------------------------------------------------------------
    -- Nothing in L2 exists without an evidence_link (ADR-0006).
    --
    -- Enforced at insert, which forces the caller to have decided what the
    -- evidence IS before creating the row - the link is written first, then the
    -- row it grounds. That ordering is the point: an entity whose evidence is
    -- "to be filled in later" is exactly the ungrounded assertion this schema
    -- exists to prevent, and later never comes.
    -- ---------------------------------------------------------------------

    CREATE TRIGGER entity_requires_evidence BEFORE INSERT ON entity
    WHEN NOT EXISTS (
        SELECT 1 FROM evidence_link
         WHERE subject_kind = 'entity' AND subject_id = NEW.id
    )
    BEGIN
        SELECT RAISE(ABORT, 'nothing in L2 exists without evidence: write the evidence_link before the entity');
    END;

    CREATE TRIGGER identifier_requires_evidence BEFORE INSERT ON identifier
    WHEN NOT EXISTS (
        SELECT 1 FROM evidence_link
         WHERE subject_kind = 'identifier' AND subject_id = NEW.id
    )
    BEGIN
        SELECT RAISE(ABORT, 'nothing in L2 exists without evidence: write the evidence_link before the identifier');
    END;

    CREATE TRIGGER relationship_requires_evidence BEFORE INSERT ON relationship
    WHEN NOT EXISTS (
        SELECT 1 FROM evidence_link
         WHERE subject_kind = 'relationship' AND subject_id = NEW.id
    )
    BEGIN
        SELECT RAISE(ABORT, 'nothing in L2 exists without evidence: write the evidence_link before the relationship');
    END;

    -- The same rule read backwards: an L2 row may not be stripped of its last
    -- piece of evidence while it still exists. Removing a link is how the rule
    -- would otherwise be defeated a moment after insert.
    CREATE TRIGGER evidence_link_keeps_l2_grounded BEFORE DELETE ON evidence_link
    WHEN (
        SELECT COUNT(*) FROM evidence_link
         WHERE subject_kind = OLD.subject_kind AND subject_id = OLD.subject_id
    ) <= 1
    AND (
        (OLD.subject_kind = 'entity'
            AND EXISTS (SELECT 1 FROM entity WHERE id = OLD.subject_id))
     OR (OLD.subject_kind = 'identifier'
            AND EXISTS (SELECT 1 FROM identifier WHERE id = OLD.subject_id))
     OR (OLD.subject_kind = 'relationship'
            AND EXISTS (SELECT 1 FROM relationship WHERE id = OLD.subject_id))
    )
    BEGIN
        SELECT RAISE(ABORT, 'this is the last evidence for a row that still exists: delete the row first');
    END;

    -- A scale definition in a case is the record of what an assessment MEANT.
    -- Editing it in place would reinterpret every assessment already made under
    -- it, which is the exact failure ADR-0007's versioning exists to prevent.
    CREATE TRIGGER scale_is_immutable BEFORE UPDATE ON scale
    BEGIN
        SELECT RAISE(ABORT, 'a scale version is immutable: publish a new version, do not rewrite this one');
    END;

    CREATE TRIGGER scale_value_is_immutable BEFORE UPDATE ON scale_value
    BEGIN
        SELECT RAISE(ABORT, 'a scale version is immutable: publish a new version, do not rewrite this one');
    END;

    -- An automated actor may only assign the values its scale marks as
    -- machine-assignable.
    --
    -- This is what makes machine_assignable in the YAML a control rather than a
    -- comment. identifier_match is the case that matters: until PoC P8 measures
    -- the username sweep's false-positive rate, a rule may say two identifiers
    -- LOOK alike and may not say they belong to the same actor. Enforced here
    -- because "the code will only ever assign safe values" is the kind of
    -- promise that survives exactly until someone adds a heuristic.
    CREATE TRIGGER assessment_respects_machine_limits BEFORE INSERT ON assessment
    WHEN (NEW.actor LIKE 'rule:%' OR NEW.actor LIKE 'ai:%')
     AND NOT EXISTS (
        SELECT 1 FROM scale_value
         WHERE scale_id = NEW.dimension
           AND scale_version = NEW.scale_version
           AND value_key = NEW.value_key
           AND machine_assignable = 1
     )
    BEGIN
        SELECT RAISE(ABORT, 'an automated actor may not assign this value: a human must decide it');
    END;

    CREATE TRIGGER assessment_update_respects_machine_limits BEFORE UPDATE ON assessment
    WHEN (NEW.actor LIKE 'rule:%' OR NEW.actor LIKE 'ai:%')
     AND NOT EXISTS (
        SELECT 1 FROM scale_value
         WHERE scale_id = NEW.dimension
           AND scale_version = NEW.scale_version
           AND value_key = NEW.value_key
           AND machine_assignable = 1
     )
    BEGIN
        SELECT RAISE(ABORT, 'an automated actor may not assign this value: a human must decide it');
    END;
    "#,
    // 5: search (ADR-0003).
    //
    // The index is a PROJECTION, never a source of truth. Every row in it is
    // derived from a row in L1 or L2 by a trigger, and dropping the whole thing
    // and rebuilding it from those rows must produce the same index. That is
    // what makes it safe for an index to be lossy, denormalised, and tuned for
    // retrieval: nothing is knowable only from here.
    //
    // Two consequences are load-bearing.
    //
    // First, a document carries only the text its own row OWNS. A relationship
    // is indexed by its kind, not by the display names of the entities it
    // joins, tempting as that is - "Acme director" is exactly what an analyst
    // would type. Borrowing another row's text makes this document's
    // correctness depend on a row it does not control, so renaming an entity
    // would silently leave stale names searchable, and stale names in an
    // investigation tool are worse than absent ones. Reaching a relationship
    // means finding an endpoint and following the edge.
    //
    // Second, superseded observations are FLAGGED, not removed. An extractor
    // rerun withdraws an observation (ADR-0006); deleting its document would
    // erase the ability to ask what the case used to say, which is a question
    // investigations genuinely need to answer. Search hides them by default and
    // labels them when asked for them, which is a different thing from
    // pretending they were never there.
    r#"
    -- One row per searchable thing. `id` is an explicit INTEGER PRIMARY KEY
    -- because FTS5 external content addresses its content table by rowid, and
    -- naming it is clearer than relying on the implicit alias.
    --
    -- subject_kind is deliberately NOT constrained to a list, for the same
    -- reason evidence_link.subject_kind is not: SQLite cannot ALTER a CHECK, so
    -- a list here would make indexing a new kind of thing - artifact body text,
    -- claims, analyst notes - a table rebuild instead of one more trigger.
    CREATE TABLE search_document (
        id           INTEGER PRIMARY KEY,
        subject_kind TEXT NOT NULL,
        subject_id   TEXT NOT NULL,
        -- What a result list shows.
        title        TEXT NOT NULL,
        -- What is actually matched against, and what snippets come from.
        body         TEXT NOT NULL,
        -- The row's own type, for filtering without teaching the user FTS5
        -- column syntax: observation kind, entity type, identifier namespace.
        facet        TEXT NOT NULL,
        superseded   INTEGER NOT NULL DEFAULT 0,
        indexed_utc  TEXT NOT NULL
    ) STRICT;

    -- One document per subject. Also what makes a re-projection idempotent.
    CREATE UNIQUE INDEX search_document_subject
        ON search_document(subject_kind, subject_id);
    CREATE INDEX search_document_facet
        ON search_document(subject_kind, facet);

    -- External content: the index stores terms, and reads text back from
    -- search_document when it needs it (snippets, deletes). The alternative,
    -- a contentless index, is smaller but cannot produce snippets at all, and
    -- a result you cannot see the matching text of is a result an analyst has
    -- to open blind.
    --
    -- remove_diacritics 2 is not cosmetic. Names in an OSINT case are
    -- international and are transliterated inconsistently by the sources that
    -- publish them; without folding, a case holding "Müller" does not answer a
    -- search for "Muller", and the analyst concludes the case is empty rather
    -- than that the query was spelled differently. Version 2 rather than 1
    -- because 1 does not fold characters outside Latin-1.
    CREATE VIRTUAL TABLE search_index USING fts5(
        title,
        body,
        content = 'search_document',
        content_rowid = 'id',
        tokenize = "unicode61 remove_diacritics 2"
    );

    -- Keeping an external-content index in step is the caller's job; these
    -- three triggers are that job, done once, where no write path can skip it.
    CREATE TRIGGER search_document_indexed AFTER INSERT ON search_document
    BEGIN
        INSERT INTO search_index(rowid, title, body)
        VALUES (NEW.id, NEW.title, NEW.body);
    END;

    -- FTS5 needs the OLD text to retract the OLD terms. Passing NEW values to
    -- 'delete' leaves the previous terms in the index for ever, which reads as
    -- a search that keeps finding text the case no longer contains.
    CREATE TRIGGER search_document_reindexed AFTER UPDATE ON search_document
    BEGIN
        INSERT INTO search_index(search_index, rowid, title, body)
        VALUES ('delete', OLD.id, OLD.title, OLD.body);
        INSERT INTO search_index(rowid, title, body)
        VALUES (NEW.id, NEW.title, NEW.body);
    END;

    CREATE TRIGGER search_document_unindexed AFTER DELETE ON search_document
    BEGIN
        INSERT INTO search_index(search_index, rowid, title, body)
        VALUES ('delete', OLD.id, OLD.title, OLD.body);
    END;

    -- ---------------------------------------------------------------------
    -- L1 -> the projection.
    -- ---------------------------------------------------------------------

    -- Observations are append-only, so there is no update path to mirror.
    CREATE TRIGGER observation_is_searchable AFTER INSERT ON observation
    BEGIN
        INSERT INTO search_document
            (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
        VALUES
            ('observation', NEW.id, NEW.value, NEW.value, NEW.kind, 0, NEW.observed_utc);
    END;

    -- Withdrawn, not deleted. See the note above this migration.
    CREATE TRIGGER observation_supersession_marks_the_index
    AFTER INSERT ON observation_supersession
    BEGIN
        UPDATE search_document
           SET superseded = 1
         WHERE subject_kind = 'observation'
           AND subject_id = NEW.superseded_id;
    END;

    -- ---------------------------------------------------------------------
    -- L2 -> the projection. L2 is mutable, so all three paths are covered.
    -- ---------------------------------------------------------------------

    CREATE TRIGGER entity_is_searchable AFTER INSERT ON entity
    BEGIN
        INSERT INTO search_document
            (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
        VALUES
            ('entity', NEW.id, NEW.display_name,
             NEW.display_name || ' ' || NEW.notes, NEW.type_key, 0, NEW.created_utc);
    END;

    CREATE TRIGGER entity_reindexed AFTER UPDATE ON entity
    BEGIN
        UPDATE search_document
           SET title = NEW.display_name,
               body  = NEW.display_name || ' ' || NEW.notes,
               facet = NEW.type_key,
               indexed_utc = NEW.updated_utc
         WHERE subject_kind = 'entity' AND subject_id = OLD.id;
    END;

    CREATE TRIGGER entity_unindexed AFTER DELETE ON entity
    BEGIN
        DELETE FROM search_document
         WHERE subject_kind = 'entity' AND subject_id = OLD.id;
    END;

    -- The normalised form is indexed alongside the raw one so that a search
    -- for a lowercased domain finds the identifier that was captured shouting.
    CREATE TRIGGER identifier_is_searchable AFTER INSERT ON identifier
    BEGIN
        INSERT INTO search_document
            (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
        VALUES
            ('identifier', NEW.id, NEW.value,
             NEW.value || ' ' || NEW.normalized, NEW.namespace, 0, NEW.created_utc);
    END;

    CREATE TRIGGER identifier_reindexed AFTER UPDATE ON identifier
    BEGIN
        UPDATE search_document
           SET title = NEW.value,
               body  = NEW.value || ' ' || NEW.normalized,
               facet = NEW.namespace
         WHERE subject_kind = 'identifier' AND subject_id = OLD.id;
    END;

    CREATE TRIGGER identifier_unindexed AFTER DELETE ON identifier
    BEGIN
        DELETE FROM search_document
         WHERE subject_kind = 'identifier' AND subject_id = OLD.id;
    END;

    CREATE TRIGGER relationship_is_searchable AFTER INSERT ON relationship
    BEGIN
        INSERT INTO search_document
            (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
        VALUES
            ('relationship', NEW.id, NEW.kind, NEW.kind, NEW.kind, 0, NEW.created_utc);
    END;

    CREATE TRIGGER relationship_reindexed AFTER UPDATE ON relationship
    BEGIN
        UPDATE search_document
           SET title = NEW.kind, body = NEW.kind, facet = NEW.kind
         WHERE subject_kind = 'relationship' AND subject_id = OLD.id;
    END;

    CREATE TRIGGER relationship_unindexed AFTER DELETE ON relationship
    BEGIN
        DELETE FROM search_document
         WHERE subject_kind = 'relationship' AND subject_id = OLD.id;
    END;

    -- ---------------------------------------------------------------------
    -- Backfill.
    --
    -- Triggers only fire on writes that happen after they exist, so without
    -- this an upgraded case would hold rows that search cannot see, and the
    -- failure is silent: search works, returns fewer results than the truth,
    -- and an analyst reads the gap as an answer. A fresh case backfills
    -- nothing because there is nothing there yet, which is exactly why this
    -- is easy to leave out and impossible to notice afterwards.
    -- ---------------------------------------------------------------------
    INSERT INTO search_document
        (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
    SELECT 'observation', o.id, o.value, o.value, o.kind,
           CASE WHEN EXISTS (
               SELECT 1 FROM observation_supersession s WHERE s.superseded_id = o.id
           ) THEN 1 ELSE 0 END,
           o.observed_utc
      FROM observation o;

    INSERT INTO search_document
        (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
    SELECT 'entity', e.id, e.display_name, e.display_name || ' ' || e.notes,
           e.type_key, 0, e.updated_utc
      FROM entity e;

    INSERT INTO search_document
        (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
    SELECT 'identifier', i.id, i.value, i.value || ' ' || i.normalized,
           i.namespace, 0, i.created_utc
      FROM identifier i;

    INSERT INTO search_document
        (subject_kind, subject_id, title, body, facet, superseded, indexed_utc)
    SELECT 'relationship', r.id, r.kind, r.kind, r.kind, 0, r.created_utc
      FROM relationship r;
    "#,
    // 6: coverage. What the case knows it has not read.
    //
    // Increment 10 gave documents a prose budget and, past it, truncated. The
    // truncation was counted and audited and then went nowhere an analyst
    // looks, which left the case able to answer a search with less than it
    // holds while looking like it answered with all of it. That is the failure
    // this product exists to prevent, so it gets schema rather than a log line.
    //
    // Two tables, because two different things were missing.
    r#"
    -- What a run was ABOUT.
    --
    -- `derivation` records what a run PRODUCED, which is not the same question
    -- and cannot answer this one: a run that produced nothing writes no
    -- derivation edge, and "this artifact was read and yielded nothing" is
    -- precisely the state that must not be confused with "this artifact was
    -- never read". Without this table those two are indistinguishable, and the
    -- second one is a job still to do while the first one is finished work.
    --
    -- Written for failed runs too. An artifact the extractor refused - a PDF,
    -- today - is a document this case cannot answer questions about, and that
    -- is worth more to an analyst than most things it can.
    CREATE TABLE run_subject (
        run_id       TEXT NOT NULL REFERENCES transform_run(id),
        subject_kind TEXT NOT NULL,
        subject_id   TEXT NOT NULL,
        PRIMARY KEY (run_id, subject_kind, subject_id)
    ) STRICT;

    CREATE INDEX run_subject_subject ON run_subject(subject_kind, subject_id);

    -- What a run that SUCCEEDED knows it did not cover.
    --
    -- Only partial success needs a row here. A run that failed outright already
    -- says so in transform_run.status and error_code, and duplicating that into
    -- a second table would create two places to disagree about the same run.
    -- The gap this table exists for is the quiet one: the run reports success,
    -- the observations look complete, and some of the document was never read.
    CREATE TABLE coverage_gap (
        id           TEXT PRIMARY KEY NOT NULL,
        run_id       TEXT NOT NULL REFERENCES transform_run(id),
        -- What is incomplete. 'artifact' today; unconstrained for the same
        -- reason as evidence_link.subject_kind, since SQLite cannot ALTER a
        -- CHECK and the next thing with a coverage story is not known yet.
        subject_kind TEXT NOT NULL,
        subject_id   TEXT NOT NULL,
        -- A stable code, because these are counted and grouped:
        -- 'text_truncated' today.
        gap_kind     TEXT NOT NULL,
        -- Said in words, for a human reading one row rather than a chart.
        detail       TEXT NOT NULL,
        -- How much was missed, in `unit`. NULL when the run genuinely does not
        -- know, which is a real answer and not a zero: a parser that stopped
        -- early cannot report how much was left.
        magnitude    INTEGER,
        unit         TEXT,
        recorded_utc TEXT NOT NULL,
        -- A magnitude without a unit gets read as whatever the reader expects,
        -- and the reader expects the smaller number.
        CHECK ((magnitude IS NULL) = (unit IS NULL))
    ) STRICT;

    CREATE INDEX coverage_gap_subject ON coverage_gap(subject_kind, subject_id);
    CREATE INDEX coverage_gap_run ON coverage_gap(run_id);

    -- L1 lineage, so append-only on the same grounds as derivation: an account
    -- of what a run did that can be edited afterwards is not an account.
    CREATE TRIGGER run_subject_is_append_only BEFORE UPDATE ON run_subject
    BEGIN
        SELECT RAISE(ABORT, 'run_subject is append-only: lineage cannot be edited');
    END;

    CREATE TRIGGER run_subject_no_delete BEFORE DELETE ON run_subject
    BEGIN
        SELECT RAISE(ABORT, 'run_subject is append-only: lineage cannot be deleted');
    END;

    CREATE TRIGGER coverage_gap_is_append_only BEFORE UPDATE ON coverage_gap
    BEGIN
        SELECT RAISE(ABORT, 'coverage_gap is append-only: a gap is superseded by a later run, not edited');
    END;

    CREATE TRIGGER coverage_gap_no_delete BEFORE DELETE ON coverage_gap
    BEGIN
        SELECT RAISE(ABORT, 'coverage_gap is append-only: deleting a gap is how a case forgets what it cannot answer');
    END;

    -- ---------------------------------------------------------------------
    -- Backfill.
    --
    -- Every run that produced anything can be recovered from its derivation
    -- edges. Runs that produced nothing cannot be - they left no trace tying
    -- them to an input, which is the whole reason this table now exists - so an
    -- upgraded case will under-report attempts until its artifacts are
    -- re-extracted. Stated in docs/increments/011-coverage.md rather than
    -- papered over, because the direction of the error matters: it reports work
    -- as still to do, never as already done.
    --
    -- coverage_gap gets no backfill at all. Truncation was previously recorded
    -- only in an audit payload, which carries no run_id, and a gap that cannot
    -- name the run accountable for it is not evidence of anything.
    -- ---------------------------------------------------------------------
    INSERT INTO run_subject (run_id, subject_kind, subject_id)
    SELECT DISTINCT d.run_id, d.input_kind, d.input_id
      FROM derivation d;
    "#,
];

/// The schema version this build writes and understands.
pub fn target_version() -> i64 {
    MIGRATIONS.len() as i64
}

/// Bring a database up to `target_version()`, or fail if it is newer.
pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    if current > target_version() {
        return Err(StoreError::SchemaFromTheFuture {
            found: current,
            supported: target_version(),
        });
    }

    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = index as i64 + 1;
        if version <= current {
            continue;
        }

        // Each migration is one transaction: a failure leaves user_version
        // untouched, so the next open retries from the same point rather than
        // finding a half-applied schema.
        conn.execute_batch("BEGIN")?;
        match conn.execute_batch(sql) {
            Ok(()) => {
                // user_version does not accept a bound parameter.
                conn.execute_batch(&format!("PRAGMA user_version = {version};"))?;
                conn.execute_batch("COMMIT")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(StoreError::Sqlite(e));
            }
        }
    }

    Ok(())
}

/// A stable, sorted description of the schema, for the golden test.
///
/// Reads `sqlite_master` rather than the migration text, so it describes what
/// the database actually is, not what we believe we asked for.
pub fn schema_fingerprint(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT type, name, COALESCE(sql, '')
           FROM sqlite_master
          WHERE name NOT LIKE 'sqlite_%'
          ORDER BY type, name",
    )?;

    let rows = stmt.query_map([], |r| {
        Ok(format!(
            "{}\t{}\t{}",
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            // Normalise whitespace so reindenting a migration does not read as
            // a schema change, while a real structural change still does.
            r.get::<_, String>(2)?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        ))
    })?;

    let mut lines = Vec::new();
    for row in rows {
        lines.push(row?);
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn memory_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn migrating_a_fresh_database_reaches_the_target_version() {
        let conn = memory_db();
        migrate(&conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, target_version());
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        let first = schema_fingerprint(&conn).unwrap();

        // Must not fail by trying to CREATE TABLE a second time.
        migrate(&conn).unwrap();
        assert_eq!(schema_fingerprint(&conn).unwrap(), first);
    }

    #[test]
    fn a_newer_schema_is_refused_by_name() {
        let conn = memory_db();
        conn.execute_batch("PRAGMA user_version = 9999;").unwrap();

        let err = migrate(&conn).unwrap_err();
        assert!(
            matches!(err, StoreError::SchemaFromTheFuture { found: 9999, .. }),
            "expected SchemaFromTheFuture, got: {err}"
        );
    }

    /// Seed one row in each L0 table so the triggers have something to refuse.
    fn seed_l0(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO source VALUES ('s1','url','https://x/?utm_source=a','https://x/','2026-08-12T00:00:00Z');
             INSERT INTO blob VALUES ('hash1', 10, x'00', x'01', '2026-08-12T00:00:00Z', NULL);
             INSERT INTO capture VALUES ('c1','s1','hash1','2026-08-12T00:00:00Z',200,'{}','{}','[]','static_html_only',1,'http_fetch','job1');
             INSERT INTO artifact VALUES ('a1','hash1','text/html',10,'2026-08-12T00:00:00Z');
             INSERT INTO transform_run VALUES ('r1','fetch','1','abc','p1','2026-08-12T00:00:00Z',NULL,'running',NULL,NULL);
             INSERT INTO derivation VALUES ('d1','r1','capture','c1','artifact','a1');",
        )
        .unwrap();
    }

    /// Append-only is enforced by the database, not by application discipline.
    /// A trigger nobody attempts to violate is an untested trigger.
    #[test]
    fn provenance_tables_refuse_updates_and_deletes() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l0(&conn);

        let attempts = [
            (
                "UPDATE source SET raw_locator = 'edited' WHERE id = 's1'",
                "source update",
            ),
            ("DELETE FROM source WHERE id = 's1'", "source delete"),
            (
                "UPDATE capture SET http_status = 404 WHERE id = 'c1'",
                "capture update",
            ),
            ("DELETE FROM capture WHERE id = 'c1'", "capture delete"),
            (
                "UPDATE artifact SET media_type = 'text/plain' WHERE id = 'a1'",
                "artifact update",
            ),
            ("DELETE FROM artifact WHERE id = 'a1'", "artifact delete"),
            (
                "UPDATE derivation SET output_id = 'other' WHERE id = 'd1'",
                "derivation update",
            ),
            (
                "DELETE FROM derivation WHERE id = 'd1'",
                "derivation delete",
            ),
            (
                "DELETE FROM blob WHERE content_hash = 'hash1'",
                "blob delete",
            ),
        ];

        for (sql, what) in attempts {
            assert!(
                conn.execute_batch(sql).is_err(),
                "{what} was permitted - provenance can be rewritten"
            );
        }
    }

    /// Crypto-shredding is the single permitted mutation of a blob row: the
    /// key is destroyed, everything that records the blob existed survives.
    #[test]
    fn a_blob_may_be_shredded_but_not_otherwise_changed() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l0(&conn);

        conn.execute_batch(
            "UPDATE blob SET wrapped_key = NULL, wrapped_nonce = NULL,
                             shredded_utc = '2026-08-12T01:00:00Z'
             WHERE content_hash = 'hash1'",
        )
        .unwrap();

        // The record that this evidence existed is intact.
        let (size, key_is_null): (i64, bool) = conn
            .query_row(
                "SELECT size_bytes, wrapped_key IS NULL FROM blob WHERE content_hash = 'hash1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(size, 10);
        assert!(key_is_null);

        // But the hash or size cannot be rewritten under cover of a shred.
        assert!(conn
            .execute_batch("UPDATE blob SET size_bytes = 999 WHERE content_hash = 'hash1'")
            .is_err());
    }

    /// A run legitimately completes; it must not be able to change what ran.
    #[test]
    fn a_transform_run_may_complete_but_not_be_rewritten() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l0(&conn);

        conn.execute_batch(
            "UPDATE transform_run SET status = 'succeeded',
                    finished_utc = '2026-08-12T00:00:01Z' WHERE id = 'r1'",
        )
        .unwrap();

        // Changing which code ran, or re-completing a finished run, is refused.
        assert!(conn
            .execute_batch("UPDATE transform_run SET code_version = 'xyz' WHERE id = 'r1'")
            .is_err());
        assert!(conn
            .execute_batch("UPDATE transform_run SET status = 'failed' WHERE id = 'r1'")
            .is_err());
    }

    /// Seed the minimum L2 needs: a type, a scale, and one piece of evidence
    /// to point at. Deliberately writes the evidence_link *before* the entity,
    /// because that is the only order the schema permits.
    fn seed_l2(conn: &Connection) {
        seed_l0(conn);
        conn.execute_batch(
            "INSERT INTO observation VALUES ('o1','a1','r1','page.title','Acme','{}','2026-08-12T00:00:00Z');
             INSERT INTO entity_type VALUES ('person','Person','A human being','{}',1);
             INSERT INTO scale VALUES ('review_status',1,'Review status','active','q','yaml','2026-08-12T00:00:00Z');
             INSERT INTO scale_value VALUES ('review_status',1,'insufficient_information','Insufficient information','d',0,0);
             INSERT INTO scale_value VALUES ('review_status',1,'unreviewed','Unreviewed','d',1,1);
             INSERT INTO evidence_link VALUES ('el1','entity','e1','observation','o1','supports','2026-08-12T00:00:00Z');
             INSERT INTO entity VALUES ('e1','person','Acme','','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');",
        )
        .unwrap();
    }

    /// Apply migrations up to and including `version`, leaving the database
    /// exactly as an older build would have left it.
    fn migrate_to(conn: &Connection, version: usize) {
        for sql in MIGRATIONS.iter().take(version) {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(&format!("PRAGMA user_version = {version};"))
            .unwrap();
    }

    /// The projection is maintained by triggers, and a trigger cannot fire for
    /// a row that was written before it existed. So an upgraded case starts
    /// with rows that search cannot see — and the failure is silent, which is
    /// what makes it dangerous: search still works, still returns results, and
    /// simply omits everything collected before the upgrade. An analyst reads
    /// "no results" as a finding about the world rather than about the index.
    ///
    /// A fresh case cannot catch this, because at migration time it holds
    /// nothing to backfill and every later write goes through the triggers.
    /// That is precisely why the backfill is easy to omit: every test that
    /// starts from `migrate()` passes without it.
    #[test]
    fn upgrading_a_populated_case_makes_its_existing_rows_searchable() {
        let conn = memory_db();

        // A case as an older build left it: schema at 4, rows already in it.
        // `migrate` then takes it all the way to current, which is what an
        // upgrade actually does; migration 5's backfill is what is under test.
        migrate_to(&conn, 4);
        seed_l2(&conn);
        conn.execute_batch(
            "INSERT INTO observation VALUES ('o2','a1','r1','page.email','press@acme.example','{}','2026-08-12T00:00:00Z');
             INSERT INTO observation VALUES ('o3','a1','r1','page.email','stale@acme.example','{}','2026-08-12T00:00:00Z');
             -- A rerun withdrew o3 without replacing it.
             INSERT INTO observation_supersession VALUES ('o3', NULL, 'r1', '2026-08-12T01:00:00Z');
             INSERT INTO evidence_link VALUES ('el2','identifier','i1','observation','o2','supports','2026-08-12T00:00:00Z');
             INSERT INTO identifier VALUES ('i1','e1','email_address','Press@ACME.example','press@acme.example','2026-08-12T00:00:00Z');
             INSERT INTO evidence_link VALUES ('el3','entity','e2','observation','o1','supports','2026-08-12T00:00:00Z');
             INSERT INTO entity VALUES ('e2','person','Jürgen Müller','a note','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z');
             INSERT INTO evidence_link VALUES ('el4','relationship','rel1','observation','o1','supports','2026-08-12T00:00:00Z');
             INSERT INTO relationship VALUES ('rel1','e1','e2','mentioned_alongside',NULL,NULL,'2026-08-12T00:00:00Z');",
        )
        .unwrap();

        // Now the upgrade.
        migrate(&conn).unwrap();

        // Every pre-existing row has a document. Counted per table rather than
        // spot-checked, because the way this goes wrong is one table being left
        // out of the backfill, not the backfill being absent altogether.
        for (kind, table) in [
            ("observation", "observation"),
            ("entity", "entity"),
            ("identifier", "identifier"),
            ("relationship", "relationship"),
        ] {
            let rows: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            let docs: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM search_document WHERE subject_kind = ?1",
                    [kind],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                rows, docs,
                "{kind}: {rows} rows backfilled to {docs} documents"
            );
        }

        // The index, not just the projection, actually answers.
        let hit: String = conn
            .query_row(
                "SELECT d.subject_id
                   FROM search_index i JOIN search_document d ON d.id = i.rowid
                  WHERE search_index MATCH '\"press@acme.example\"' AND d.subject_kind = 'observation'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, "o2");

        // A withdrawn observation is carried as withdrawn, not as current and
        // not as missing.
        let superseded: i64 = conn
            .query_row(
                "SELECT superseded FROM search_document WHERE subject_id = 'o3'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            superseded, 1,
            "a superseded observation was backfilled as current"
        );
    }

    /// Migration 6 recovers what a run was about from the edges it produced.
    ///
    /// The assertion that matters most here is the *negative* one. A run that
    /// produced nothing left no derivation edge, so the backfill cannot see it,
    /// and an upgraded case will therefore report that artifact as never
    /// attempted. That is a real inaccuracy and it is pinned deliberately: the
    /// error points at more work to do, never at work already done, and a later
    /// change that quietly reversed that direction would be much worse than the
    /// gap itself.
    #[test]
    fn upgrading_a_case_recovers_which_artifacts_a_run_had_read() {
        let conn = memory_db();

        assert_eq!(
            target_version(),
            6,
            "this test pins the v5 -> v6 upgrade; a later migration needs its own"
        );

        migrate_to(&conn, 5);
        seed_l0(&conn);
        conn.execute_batch(
            "INSERT INTO artifact VALUES ('a2','hash1','application/pdf',10,'2026-08-12T00:00:00Z');
             INSERT INTO artifact VALUES ('a3','hash1','text/html',10,'2026-08-12T00:00:00Z');
             -- r2 read a1 and produced an observation, so it is recoverable.
             INSERT INTO transform_run VALUES ('r2','extract.html','1','abc','p1','2026-08-12T00:01:00Z','2026-08-12T00:01:01Z','succeeded',NULL,NULL);
             INSERT INTO observation VALUES ('o9','a1','r2','page.title','Acme','{}','2026-08-12T00:01:00Z');
             INSERT INTO derivation VALUES ('d9','r2','artifact','a1','observation','o9');
             -- r3 read a3 and found nothing in it. No edge, so no trace.
             INSERT INTO transform_run VALUES ('r3','extract.html','1','abc','p1','2026-08-12T00:02:00Z','2026-08-12T00:02:01Z','succeeded',NULL,NULL);",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let subjects: Vec<String> = conn
            .prepare("SELECT run_id || ':' || subject_id FROM run_subject ORDER BY 1")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();

        assert_eq!(
            subjects,
            vec!["r1:c1".to_string(), "r2:a1".to_string()],
            "the backfill recovered the wrong set of run subjects"
        );

        // The named limitation, asserted rather than assumed. a3 was read and
        // is about to be reported as unread.
        assert!(
            !subjects.iter().any(|s| s.ends_with(":a3")),
            "a run that produced nothing became recoverable, which means the \
             backfill found a trace this test believes does not exist - good \
             news, but docs/increments/011-coverage.md now says something false"
        );

        // Both new tables are lineage, so both refuse to be rewritten.
        conn.execute_batch(
            "INSERT INTO coverage_gap
             VALUES ('g1','r2','artifact','a1','text_truncated','4 KiB of prose',4096,'bytes','2026-08-12T00:01:01Z')",
        )
        .unwrap();
        for attempt in [
            "UPDATE run_subject SET subject_id = 'a2' WHERE run_id = 'r2'",
            "DELETE FROM run_subject WHERE run_id = 'r2'",
            "UPDATE coverage_gap SET magnitude = 0 WHERE id = 'g1'",
            "DELETE FROM coverage_gap WHERE id = 'g1'",
        ] {
            assert!(
                conn.execute_batch(attempt).is_err(),
                "lineage accepted `{attempt}`"
            );
        }

        // A magnitude with no unit is a number the reader supplies a unit for,
        // and they supply the flattering one.
        assert!(conn
            .execute_batch(
                "INSERT INTO coverage_gap
                 VALUES ('g2','r2','artifact','a1','text_truncated','some',4096,NULL,'2026-08-12T00:01:01Z')"
            )
            .is_err());
    }

    /// ADR-0006's single L2 constraint, tested by trying to break it.
    ///
    /// An entity with no evidence is an assertion, and a tool that lets you
    /// record assertions next to evidence and cannot tell them apart is the
    /// failure this whole schema exists to avoid.
    #[test]
    fn nothing_in_l2_can_be_created_without_evidence() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l2(&conn);

        let ungrounded = [
            (
                "INSERT INTO entity VALUES ('e2','person','Ungrounded','','2026-08-12T00:00:00Z','2026-08-12T00:00:00Z')",
                "entity",
            ),
            (
                "INSERT INTO identifier VALUES ('i1','e1','email','a@b.test','a@b.test','2026-08-12T00:00:00Z')",
                "identifier",
            ),
            (
                "INSERT INTO relationship VALUES ('rel1','e1','e1','knows',NULL,NULL,'2026-08-12T00:00:00Z')",
                "relationship",
            ),
        ];

        for (sql, what) in ungrounded {
            assert!(
                conn.execute_batch(sql).is_err(),
                "an ungrounded {what} was accepted - L2 can hold assertions with no evidence"
            );
        }

        // And the same rows succeed once their evidence exists.
        conn.execute_batch(
            "INSERT INTO evidence_link VALUES ('el2','identifier','i1','observation','o1','supports','2026-08-12T00:00:00Z');
             INSERT INTO identifier VALUES ('i1','e1','email','a@b.test','a@b.test','2026-08-12T00:00:00Z');",
        )
        .unwrap();
    }

    /// The rule read backwards. Enforcing it only at insert would leave it
    /// defeatable one statement later.
    #[test]
    fn the_last_evidence_for_a_live_row_cannot_be_removed() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l2(&conn);

        assert!(
            conn.execute_batch("DELETE FROM evidence_link WHERE id = 'el1'")
                .is_err(),
            "an entity was stripped of its last evidence and survived"
        );

        // A second link makes the first removable: the row stays grounded.
        conn.execute_batch(
            "INSERT INTO evidence_link VALUES ('el1b','entity','e1','artifact','a1','context','2026-08-12T00:00:00Z')",
        )
        .unwrap();
        conn.execute_batch("DELETE FROM evidence_link WHERE id = 'el1'")
            .unwrap();
    }

    /// The scales are load-bearing, not decorative: a value that is not in the
    /// scale it names cannot be written at all.
    #[test]
    fn an_assessment_cannot_use_a_value_outside_its_scale() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        seed_l2(&conn);

        assert!(
            conn.execute_batch(
                "INSERT INTO assessment VALUES ('as1','entity','e1','review_status',1,'looks_about_right','[]','user:local','2026-08-12T00:00:00Z')"
            )
            .is_err(),
            "an invented scale value was accepted"
        );

        // A version that does not exist is refused for the same reason, which
        // is what keeps an old assessment pinned to the scale it was made under.
        assert!(conn
            .execute_batch(
                "INSERT INTO assessment VALUES ('as2','entity','e1','review_status',2,'unreviewed','[]','user:local','2026-08-12T00:00:00Z')"
            )
            .is_err());

        conn.execute_batch(
            "INSERT INTO assessment VALUES ('as3','entity','e1','review_status',1,'unreviewed','[]','user:local','2026-08-12T00:00:00Z')",
        )
        .unwrap();
    }

    /// A rule or a model may argue; only a human may conclude. The scale files
    /// say which values are machine-assignable, and this is where saying so
    /// starts to mean something.
    #[test]
    fn an_automated_actor_cannot_assign_a_value_reserved_for_humans() {
        let conn = memory_db();
        migrate(&conn).unwrap();
        seed_l2(&conn);

        // 'unreviewed' is machine-assignable in the seed; 'insufficient_information'
        // is not, and standing in here for identifier_match's unmeasured tiers.
        assert!(
            conn.execute_batch(
                "INSERT INTO assessment VALUES ('as1','entity','e1','review_status',1,'insufficient_information','[]','rule:username_sweep','2026-08-12T00:00:00Z')"
            )
            .is_err(),
            "a rule assigned a value reserved for a human"
        );
        assert!(conn
            .execute_batch(
                "INSERT INTO assessment VALUES ('as2','entity','e1','review_status',1,'insufficient_information','[]','ai:local_model','2026-08-12T00:00:00Z')"
            )
            .is_err());

        // The same rule may assign a value the scale permits it,
        conn.execute_batch(
            "INSERT INTO assessment VALUES ('as3','entity','e1','review_status',1,'unreviewed','[]','rule:username_sweep','2026-08-12T00:00:00Z')",
        )
        .unwrap();

        // and a human may assign either.
        conn.execute_batch("DELETE FROM assessment").unwrap();
        conn.execute_batch(
            "INSERT INTO assessment VALUES ('as4','entity','e1','review_status',1,'insufficient_information','[]','user:local','2026-08-12T00:00:00Z')",
        )
        .unwrap();

        // Nor can a rule sneak past by writing as a human and then updating.
        assert!(conn
            .execute_batch("UPDATE assessment SET actor = 'rule:username_sweep' WHERE id = 'as4'")
            .is_err());
    }

    /// ADR-0007 forbids a composite confidence score, and the reason it gives
    /// is that *a column that exists will eventually be displayed*. So the
    /// prohibition is tested against the schema itself rather than trusted to
    /// reviewer memory: no column may look like an overall score.
    #[test]
    fn the_schema_has_no_composite_confidence_column() {
        let conn = memory_db();
        migrate(&conn).unwrap();

        let mut tables = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        let names: Vec<String> = tables
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        for table in names {
            let mut cols = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            let columns: Vec<String> = cols
                .query_map([], |r| r.get(1))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();

            for column in columns {
                let c = column.to_lowercase();
                assert!(
                    !(c.contains("score")
                        || c.contains("composite")
                        || c.contains("overall")
                        || c == "confidence"),
                    "{table}.{column} looks like a composite confidence score, which ADR-0007 forbids"
                );
            }
        }
    }

    /// Golden-schema test.
    ///
    /// Pins the exact schema the migrations produce. It fails on any structural
    /// change, which is the point: editing a shipped migration would leave
    /// existing cases on a different schema from new ones, with nothing to
    /// detect the divergence. When this fails, either append a migration, or
    /// update this constant *and* be certain the change is additive.
    #[test]
    fn schema_matches_the_golden_fingerprint() {
        let conn = memory_db();
        migrate(&conn).unwrap();

        let actual = schema_fingerprint(&conn).unwrap();

        if std::env::var("KOKIN_BLESS_SCHEMA").is_ok() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/golden_schema.txt");
            std::fs::write(
                path,
                format!(
                    "{actual}
"
                ),
            )
            .unwrap();
            println!("blessed {path}");
            return;
        }

        assert_eq!(
            actual,
            GOLDEN_SCHEMA.trim_end(),
            "the schema changed. If that is intended, append a migration (never              edit a shipped one) and re-bless with KOKIN_BLESS_SCHEMA=1."
        );
    }

    /// The expected schema, kept in a file rather than a string literal.
    ///
    /// Regenerate with:
    ///
    /// ```text
    /// KOKIN_BLESS_SCHEMA=1 cargo test -p kokin-store schema_matches
    /// ```
    ///
    /// The first version of this was a hand-written string literal, and it was
    /// wrong - it listed a table SQLite does not create until the first
    /// autoincrement insert. Hand-writing an expected value is exactly the
    /// mistake a golden test exists to catch, so the expected value is now
    /// copied from the database and never typed.
    const GOLDEN_SCHEMA: &str = include_str!("golden_schema.txt");
}
