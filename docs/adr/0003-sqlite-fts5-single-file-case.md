# ADR-0003: One case is one SQLite database, with FTS5 for search

- **Status:** accepted, but **partly superseded by ADR-0016**. The choice of
  SQLite and FTS5 stands. The claim that a case is a single *file* does not: a
  case is a bundle directory. That claim was already inconsistent with this
  ADR's own consequence that blobs live outside the database, and ADR-0015's
  key hierarchy made it impossible.
- **Date:** 2026-08-12
- **Reversibility:** hard. The storage engine determines the schema, the search
  implementation, the export format, and the backup/restore story.

## Context

The product requires multiple independent cases, local-first storage with no
server, import/export/backup/restore, secure deletion, search across an entire
case, and cases of at least 100,000 observations that remain searchable.

## Alternatives considered

**PostgreSQL.** Excellent full-text search and real graph query support via
recursive CTEs. Requires a server process, which contradicts local-first,
complicates packaging enormously, and makes "a case is a file you can copy,
back up, and hand to someone" impossible.

**Embedded key-value store (sled, redb) plus a hand-rolled index.** Fast, simple
to embed. We would be reimplementing joins, full-text search, transactions
across entity types, and migrations. Rejected: the schema in ADR-0006 is
relational in nature and this would be building a worse relational engine.

**SQLite, one file per case, FTS5 for search.** A case is a single file, which
makes backup, export, restore, and "send this to a colleague" trivially
correct. FTS5 is mature, supports external-content tables (so the index does not
duplicate the data), and handles the stated scale comfortably. Transactions give
us the "a collector failure cannot corrupt the case" guarantee directly.

## Decision

> **Superseded in part by ADR-0016.** The text below is kept as the original
> record. "One encrypted file per case" is no longer accurate: a case is a
> bundle directory holding `header.json`, `case.db`, and `blobs/`. Everything
> here about SQLite, FTS5, and the index carrying `(subject_kind, subject_id,
> case_id)` still stands.

SQLite, one encrypted file per case, FTS5 as an external-content index over
observations, entities, analyst notes, and claims.

The FTS index carries `(subject_kind, subject_id, case_id)` on every row so that
a search hit always routes back to the evidence it came from — which is what
makes "search across the entire case" compatible with the evidence model rather
than a separate parallel feature.

Scale is not assumed. PoC P2 generates 100,000 observations, 300,000
relationships, and 100,000 vectors and measures p95 latency for FTS queries,
three-hop recursive CTEs, and vector KNN on the owner's hardware. If any
interactive query exceeds 200 ms at p95, the graph layer moves behind its trait
to an embedded graph engine — the reason ADR-0005 puts a trait there in the
first place.

## Consequences

- Cross-case search is not possible without opening each case. Accepted: cases
  are intentionally isolated, and a global index would be a cross-case data leak.
- Concurrent access to one case from two application instances must be
  prevented; WAL mode plus a lock file, handled in increment 3.
- Schema migrations need an explicit, tested strategy (`user_version` plus a
  golden-schema test) since there is no migration framework.
- Blobs do not live in the database. Large binary evidence goes to the
  content-addressed store (`kokin-blob`) with only its hash in SQLite, keeping
  the database small enough to open quickly.
