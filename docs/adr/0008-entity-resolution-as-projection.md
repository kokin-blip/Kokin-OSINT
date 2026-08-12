# ADR-0008: Entity resolution is a reversible projection, never a rewrite

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard as a design; the whole point is that individual
  decisions made under it are trivially reversible.

## Context

The spec requires merge, reject, split, undo, alternate hypotheses, and
contradictory evidence, and states that a high similarity score must not
automatically merge entities.

The conventional implementation merges by rewriting: pick a surviving entity,
repoint foreign keys, delete the absorbed row. That is destructive and
effectively irreversible — once the rows are gone, "undo" means reconstructing
state that no longer exists, and any evidence that argued *against* the merge
has been discarded along with it.

## Decision

Merging changes no existing rows. `er_candidate`, `er_decision`, and
`er_merge_map` record the decision; reads resolve identity through
`er_merge_map WHERE active = 1`.

- Absorbed entities keep their rows, identifiers, and evidence links.
- **Split** is deactivating a merge-map row.
- **Undo** is a reversing decision, which leaves the original decision visible
  in history rather than erasing it.
- **Alternate hypotheses** coexist under one `hypothesis_group_id`, so "these
  might be the same person, and here are two competing readings" is
  representable rather than something an analyst holds in their head.

A `CHECK` constraint rejects actors prefixed `rule:` or `ai:` on
`action = 'merge'`. **Automated similarity cannot merge entities, at the schema
level.** A rule or a model may create an `er_candidate` and argue for it; only a
human actor can decide it. This is enforced in the database rather than in
application code because application code is where such guarantees quietly
erode.

## Consequences

- Every read of entity identity goes through the merge map. This is a join on a
  hot path and must be indexed carefully; it is the main performance cost of the
  design.
- Storage grows, since nothing is reclaimed on merge. Acceptable — entity rows
  are small, and the alternative is destroying the record of a judgement.
- Users can be shown *why* two entities are proposed as the same, and can
  disagree without losing anything. That is the whole point.
