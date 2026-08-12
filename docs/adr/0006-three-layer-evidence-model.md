# ADR-0006: Three layers — provenance, lineage, analysis

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard. This is the schema everything else is built on.

## Context

The spec's central requirement is that raw evidence and analytical conclusions
never collapse into each other. The competitor survey (`research/README.md`)
shows this is where the field actually fails: graph-native tools make the entity
graph the data model, so an entity and the evidence for it are the same object,
and there is nowhere to record that two sources disagree.

## Decision

Three layers in one encrypted case file.

**L0 — provenance. Append-only.** `blob` (content-addressed, BLAKE3),
`capture` (request, `redirect_chain_json`, `capture_completeness`), `artifact`,
`source`. Rows are inserted, never updated or deleted. Enforced by triggers, not
by convention.

**L1 — lineage.** `transform_run` records what code ran (transform name and
version, `code_version`, `params_hash`, environment, status, error code).
`derivation` records the edge: input kind and id, output kind and id.
`observation` carries `locator_json` — an xpath, a byte range, a page and
bounding box, or a frame and millisecond offset — so every extracted value
points at exactly where it came from.

**L2 — analysis. Mutable.** `entity`, `identifier`, `relationship`, `claim`,
`ai_suggestion`, and `evidence_link` (subject, evidence, and a `role` of
supports / contradicts / context).

**Nothing in L2 exists without an `evidence_link`.** That single constraint is
the mechanism behind the acceptance criterion "graph edges open their supporting
evidence" — the UI feature is a consequence of the schema, not a screen someone
has to remember to build.

## Reprocessing

Rerunning a parser or a model after an upgrade emits *new* observations. Old
ones gain an `observation_supersession` row; they are never edited or deleted.
Any claim resting on a superseded observation is flagged `needs_review` with a
diff.

This is the part that is easy to get wrong. The tempting design updates the
observation in place, which silently rewrites the basis of a conclusion an
analyst already accepted. Under this model, an upgrade can never change what a
human concluded without telling them.

## Consequences

- The case file grows monotonically. Reprocessing costs storage. Accepted:
  content-addressed blobs deduplicate the expensive part, and the alternative is
  losing history.
- Three layers is more schema than a naive design needs, and every write path
  must know which layer it is writing to. This is the cost of the product
  thesis; it is not incidental complexity.
- `entity_type` is data, not schema: one `entity` table plus a seeded type
  registry plus per-type form descriptors. Adding the remaining entity types is
  seed data and a form, never a migration. This is why "~25 entity types" is not
  a Phase 1 burden.
