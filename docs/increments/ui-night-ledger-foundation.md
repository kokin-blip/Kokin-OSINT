# Night Ledger foundation and entity workflow
Date: 2026-10-08. Selected concept: **8 — Night Ledger**. Scope: design foundation, initial desktop shell and entity-to-evidence vertical slice. The complete frontend rebuild remains in progress.

## Completed work and files
- Audit, Figma inventory, component mapping, motion specification and phased roadmap: `docs/ui-rebuild/00-audit.md` through `04-implementation-roadmap.md`.
- Semantic tokens, Button, Panel, Badge, Icon, EmptyState and development-only Gallery: `ui/src/lib/design/`. Native fields and disclosures remain in use; unused dialog, drawer, toast, split-pane and other primitives are deferred.
- Contextual case card: `ui/src/lib/layout/CaseContext.svelte`.
- Entity dossier, source citation and recent activity: `ui/src/lib/features/entities/`; `ui/src/lib/Entity.svelte` preserves its existing presentation interface.
- Shell, explicit search, case information, library visibility and navigation: `ui/src/lib/Workbench.svelte`; open-case integration in `ui/src/App.svelte`; tokens and development-only gallery entry in `ui/src/main.ts`.
- Existing import picker exposed through `ui/src/lib/Actions.svelte`; icon provenance and MIT dependency entry recorded in `docs/dependencies.csv`.
- Frontend regressions and native workflow capture: `Workbench.test.ts`, `SourceCitation.test.ts` and `ui/tests/desktop/workflow.spec.mjs`.
All fifteen concept images remain in the outer workspace's Design concepts directory. Earlier core/backend working-tree changes were preserved; this phase did not change Rust implementations or IPC bindings.

## Figma references
DesignCode file NKlo830IkWSROn9bVbPtNQ: dark Pro composition preview 23201:64056; SideMenu 23204:130790 / 23204:130773; Search 23204:129892; Primary 104:6313; Ghost 184:9849; ListCard 23204:134507; Notification 23204:134600; Inspector light instance 370:732420 / set 249:28723. These inform composition and tokens. Pro preview source is raster-only; no editable Pro component or prototype timing is claimed. See the inventory and mapping for verified access limits.

## Motion and interactions
Shared 120/160/200 ms motion tokens and reduced-motion safeguards are present. Short control hover feedback is implemented. Sidebar collapse, case disclosure and confidence disclosures operate immediately; animated shell transitions, inspector resizing and the full motion engine remain future work. Command/Ctrl+K focuses search; submission is explicit, Escape closes case information and returns focus.

## Functionality preserved
Native case creation/open/close/recovery, import, explicit extraction, collection/network controls, analytical writes, all seven confidence dimensions, disagreements and actor attribution, evidence roles, search coverage/paging, full activity history, bounded graph, export/verify/restore remain accessible. Entity → identifier/relationship → observation → source bytes uses existing reads and back history. Citation preview quotes the backend excerpt; an unavailable citation does not turn an assertion into a source quote. Late reads and errors are guarded after close or navigation. No new network mechanism, telemetry, runtime asset fetching, markup rendering, schema change or npm dependency.

## Tests executed
- Svelte check: zero errors/warnings.
- Vitest: **34 tests passed across 6 files**, 1.22 s (baseline 29).
- Native Apple Silicon desktop acceptance with KOKIN_NETWORK=deny: eleven-step workflow repeated **twice**, one Mocha test, 26.7 s. Added identifier-to-own-evidence navigation; close/reopen and export/restore preserve recorded identities, counts and lineage.
- Production frontend build succeeds; gallery and desktop-driver markers excluded from built JavaScript.
- Research, documentation cross-reference and SQL-ordering lints pass.
The desktop run used the existing test-only debug build. It is not a release binary. The driver logs an existing addTauriApi hook warning despite successful command/assertion execution. Full Rust security suite was not rerun for this frontend-only phase.

## Measurements
Apple M4 Pro (Mac16,8), arm64, 24 GiB RAM. Baseline production frontend build 478 ms; current 537 ms. Baseline CSS 16.63 KB / gzip 3.56 KB; current 27.73 KB / gzip 5.92 KB. Baseline JS 117.18 KB / gzip 39.69 KB; current 141.12 KB / gzip 48.28 KB. Decorative local image 1481.36 KB. Build and test durations are single observations, not startup or rendering benchmarks. FPS, cold startup, memory and repeated-navigation profiles remain unmeasured.

## Visual verification and limitations
Selected reference and actual native captures were inspected together. Screenshots under `docs/ui-rebuild/screenshots/` include baseline, entity top, entity details and gallery; test evidence is synthetic and confined to tests. Native viewport achieved 1487 × 854 CSS pixels; the Mac display clamped height below the 1487 × 1058 reference. Top and scrolled captures cover the composition; full-height pixel equivalence is not claimed. Real metadata, all seven scales, actual first-party branding and native font intentionally replace illustration content.
Inter was not bundled because OFL is outside the current dependency license allowlist. Icons are pinned MIT Heroicons; original decorative art has exact generation prompt and hashes in the asset provenance manifest.

## Remaining work
Migrate evidence/search/activity and analytical forms into feature modules; finish case/recovery/restore styling; settings, light theme, palette and inspector sizing; cohesive motion; graph interaction polish; accessibility and large-case performance profiling; Windows native verification and personal-use release gate. Timeline/maps/AI and broader integrations require operational backend contracts. See the phased roadmap for file-level follow-up.
