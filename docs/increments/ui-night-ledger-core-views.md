# Night Ledger: core views and forms
Date: 2026-10-12. This phase continues selected concept 8.

## Work completed and files changed
- Extracted library presentation, artifact observations and identifier/relationship detail reads from Workbench into `ui/src/lib/features/workbench/{LibraryView,ArtifactDetail,SubjectDetail}.svelte`. Request generation guards, paging and back history remain in Workbench.
- Extracted search filters/coverage into `features/search/SearchControls.svelte` and paginated activity presentation into `features/activity/ActivityHistory.svelte`.
- Moved the existing action controller into `features/operations/OperationsPanel.svelte`; Actions is a compatibility wrapper retaining bindings and the imperative import entrypoint. CollectionForm and PackageForm contain controlled forms; handlers and drafts remain in the mounted controller.
- Styled collection, analysis and package sections as restrained outlined groups; aligned case gate, restore, observation and document palettes with semantic tokens.
- Updated `CaseGate.svelte` with explicit path help. There is no default case location: the supplied folder path is used to create the case. No default-path, picker or backend behavior was added.
- Added a draft-preservation regression to Workbench.test.ts and focused desktop screenshots to workflow.spec.mjs.

## Components and Figma references
Reused the existing Night Ledger semantic foundation, native fields, controls and disclosures. Applied the previously verified DesignCode outline/card/menu patterns and selected dark composition (23201:64056). No new source nodes, Pro component access or prototype timings were verified this phase. Existing local assets were reused; no new dependency or runtime asset fetch.

## Motion implemented
Existing control hover feedback and reduced-motion safeguards remain. Form disclosures are immediate native details elements; no new animation library or complete motion engine is claimed.

## Functionality preserved
Read coverage appears alongside results; lexical search remains explicitly submitted, with facets, kind, withdrawn and canonical-merge options. Artifact paging, withdrawn counts, gaps and source-document reads remain. Subjects open their own supporting/opposing/context links and retain all seven confidence dimensions with actor/factor attribution. Activity retains subject navigation, recorded detail, paging and the tamper-evident disclaimer.
Import, selected/batch extraction, collection consent and online mode, creation/identifiers/relationships/assessments/merge-preview/reject/split, package exclusions/verification/restore, operation status/cancellation and native selection tokens retain existing handlers and contracts. Hiding case actions keeps the controller mounted. Rust, bindings, encryption, provenance and networking source are unchanged.

## Tests executed
- Svelte check: zero errors/warnings.
- Vitest: **35 tests passed in 6 files**; added draft persistence across hiding/reopening actions.
- Apple Silicon native offline eleven-step workflow passed twice per run. Final capture run: **31.6 s**. It exercises collection/extraction, citations, two grounded entities, identifier, relationship, assessments, search/coverage, audit, graph, close/reopen and package export/verify/restore with identity/count/lineage comparisons.
- Production frontend build passed, with gallery/desktop-driver marker exclusion checked.
- Documentation cross-references and diff whitespace checks pass.
- Impeccable mechanical detector returned no findings for the changed targets.
No new full Rust suite, Windows run or exhaustive merge/split failure matrix was executed. Those handlers were preserved.

## Measurements
Same Apple M4 Pro / 24 GiB arm64 Mac as the foundation phase. Final production frontend build: 549 ms, CSS 31.09 KB (gzip 6.31), JavaScript 144.92 KB (gzip 49.11). Decorative image unchanged at 1481.36 KB. These are individual build measurements, not runtime latency or memory benchmarks. FPS, cold startup and large-case profiling remain outstanding.

## Visual verification
Native screenshots in `docs/ui-rebuild/screenshots/views-2026-10-12/` show empty evidence, analytical forms, search/relationship detail, activity and entity details with synthetic test data. Inspected in two bounded rounds; top-only captures were replaced with focused captures. Final inspection prompted a narrow search-filter CSS adjustment to stack fields and prevent checkbox shrinking; captures precede that final CSS adjustment. Tests use the established clamped Mac viewport (approximately 1487 × 853 CSS pixels), not an exact full-height Figma match.

## Known limitations and remaining work
Case paths still require manual entry; recovery acknowledgement remains mandatory. Native selects and disclosures remain. Analytical action-specific fields intentionally share one mounted controller instead of duplicating operation state. Settings/preferences, theme controls, command palette, inspector resizing, richer motion, graph interaction polish, accessibility/scaling and performance profiling, and Windows verification remain. No new release or push was performed for this phase.
