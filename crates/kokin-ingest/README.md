# kokin-ingest

Composition layer: turns a URL or a file into L0 provenance and L1 lineage rows.

Depends on `kokin-net` for fetching, `kokin-blob` for bytes, and `kokin-store`
for the case database. It contains no policy of its own — the guard, the
encryption, and the append-only rules all live in those crates. This crate's
only job is to write the rows in the right order and record what it did.
