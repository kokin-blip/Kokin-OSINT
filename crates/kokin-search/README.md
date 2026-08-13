# kokin-search

Search across a case: FTS5 queries over L1 observations and L2 rows.

The index is a projection maintained by triggers in migration 5, never a source
of truth — dropping it and rebuilding costs time and nothing else. This crate
turns untrusted search-box text into an FTS5 expression that cannot become
syntax, runs it, and routes every hit back to the row it came from.

Ranking is text similarity. It is not confidence, which has seven dimensions
and no total (ADR-0007).
