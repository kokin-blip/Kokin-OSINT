# ADR-0017: isolated media worker and scoped native notices

- Status: accepted by owner, 2026-10-13
- Scope: PDFium/Tesseract/Leptonica media worker only

Use safe high-level wrappers in a first-party worker. On Mac deny default access,
permit only fixed engine/system paths and the selected public OCR resource,
deny networking, and reject processing when initialization/containment fails.
The parent retains encrypted storage, case keys, generation checks and commits.

The owner explicitly approved FTL, IJG, libpng and 0BSD native-engine exceptions,
with all notices and required FreeType credit bundled. This does not widen
`deny.toml` for unrelated Rust dependencies, adopt a GPL alternative, or license
PR #1. Pin native artifact identities in `resources/native-media-mac-arm64.json`.
Reject unreviewed installed engine versions instead of silently copying them.

The native engines remain updateable without replacing observations. Existing
readings, transformation metadata and derivative hashes survive reprocessing.
Windows containment and visible production timing remain unverified/deferred.
