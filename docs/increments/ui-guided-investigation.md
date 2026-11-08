# Guided investigation workspace

2026-10-12. Implementation continues the accepted Night Ledger direction and
preserves all pre-existing local work. This report describes core stages 1–6;
it does not claim the broader media/connectors/AI/maps roadmap is implemented.

## Delivered

- Six direct destinations with a shared evidence/back path. Graph and Export
  use the main workspace, with a compact contextual inspector.
- Case-scoped operation ownership. Navigating retains jobs and analytical drafts;
  unmount cancels outstanding jobs, clears credentials and rejects late selections.
- Contextual create/connect/identifier/assessment/evidence forms, paginated record
  and observation pickers, reviewed evidence roles, and all seven independent
  confidence dimensions. Merge/reject/split/manual references stay advanced.
  The legacy action switcher is retained only in development/test builds.
- Multi-root graph expansion with direction/kind filters, shared node/edge/time
  budgets, truncation reasons, stored/canonical identity separation, multi-select,
  drag positioning/pins, saved camera/layout and a list alternative.
- Encrypted named board/report/guide revisions, optimistic revision checks and
  append-only migration 8. Existing package snapshots carry these records.
- Skippable introduction and collapsible real-case guide based on successful
  backend state/reads. Recovery acknowledgement, offline default and collection
  consent remain required. No sample evidence is inserted by onboarding.
- Analyst report drafts with cited findings, evidence roles, seven-dimensional
  assessments, limitations and saved graph illustrations. Markdown and HTML
  exports are plaintext; HTML is escaped, self-contained and script-free. PDF
  remains deferred. Assembly checks revisions/cancellation/size and uses staging
  with atomic no-overwrite publication.
- Exact development dependency pins and a reconciled lockfile; no graph, router,
  tour or animation runtime dependency added.
- Explicit opt-in local five-second timing utility. It collects frame/input
  timing samples only, never keys, coordinates, case IDs or content; no egress,
  persistence or telemetry. This is a timing proxy, not a compositor FPS measure.

## Verification

Frontend: 57 tests across 14 files; Svelte check reports zero errors/warnings.
Affected Rust suites: 178 passed, three deliberately ignored scale probes; the added atomic publication/cancellation regression also passes.
Clippy workspace/all-targets with warnings denied and formatting pass. Contract
coverage includes failed migration rollback, disk-full workspace save refusal,
stale revisions, immutable workspace history, onboarding skip/resume during loading, aliases and evidence attribution,
malicious report markup, metadata package roundtrip, late job IDs and native
selections after originating case close.

The contextual native Mac path imports actual local HTML, extracts and reads its
source, creates two grounded records through named pickers, connects them and
expands Graph without copying IDs. The eleven-step native reference workflow
runs twice: it saves a board/report, exports escaped HTML with SVG, reopens and
restores the package, then compares full board/report payloads and exact evidence
identities/lineage. Picker paths and replay fixtures use the test-only build.
Screenshots are review artifacts with synthetic test content, not production data.

The locked Mac suspends WebKit animations. An obsolete retained test-driver app was terminated before fresh-instance layout verification. The aggregate layout run consequently
failed at an invisible animated Settings dialog and subsequent specs inherited
that open case; this is not recorded as a passing layout/motion gate. Static
functional checks can disable animations explicitly in the harness. Windows and
manual VoiceOver are deferred by the user. Actual native dialog interaction,
visible production timing, maximum-board stress and the full fault-injection
matrix remain release gates.

## Performance scope

Existing merged lexical search probe (release, documented synthetic case with
100k observations/20k entities/300k edges; 30 samples per scenario) passes:
warm p95 78.4 ms; evicted SQLite cache p95 109.1 ms; concurrent audit writes p95
160.7 ms, p99 208.5 ms, 234 writes. SQLite cache eviction is not an OS cold-disk
measurement. This does not prove all query distributions meet the target.

A no-driver production build succeeds and its ad-hoc signature verifies. Three
fresh-process, warm-filesystem, locked-screen launches reached first observed
new WebKit at 671.9/285.4/285.3 ms; final combined estimated host/new-WebKit RSS
was roughly 188 MiB. These are subprocess/startup proxies on Apple M4 Pro/24 GiB,
not visible readiness, input latency, leak freedom or representative large-case
memory. The corrected canvas layout passes a fresh-instance native width assertion; visible production timing remains unverified.

## Limitations and remaining work

- Schema 8 cases cannot be opened by older schema-7-only builds. No shipped
  migration or historical observation/assessment was edited.
- Named board/report switching is explicit; unsaved content should be saved
  before selecting another named document. Navigation itself retains drafts.
- Board illustrations are static saved layouts resolved against current facts;
  unavailable records/citations and changed aliases receive notices. Layout
  never implies a verified relationship or aggregate confidence.
- npm audit reports nine high advisories in development ZIP/browser tooling,
  rooted in unpatched extract-zip vulnerabilities GHSA-jmr9-qjv8-65gv and
  GHSA-7pqw-9j4j-h8q3. No clean audit is claimed. WDIO and those dependencies are
  excluded from production assets; the existing service also emits its known
  addTauriApi warning despite successful individual workflow execution.
- PDF/image/OCR, public connectors, optional Ollama/model installation/embeddings,
  offline maps and measured feasibility tracks remain planned after the core gate.
  Settings labels AI as unavailable; no model is downloaded or run.
- Personal experimental app only: ad-hoc signed, unnotarized. No new release,
  remote push, licensing change or PR #1 adoption was performed in this task.

## Bounded production manual pass

During an unlocked interval, the no-driver production app opened the saved real
synthetic verification case through normal controls. Its guide loaded 6/6 steps;
Graph loaded the named board/current records and restored the saved 150% zoom.
The relationship/evidence links and keyboard/list alternatives were exposed.
The dedicated canvas now opens immediately; entity-detail exploration keeps its
previous collapsed default. The Mac locked again before the local five-second
responsiveness capture, so no production input/frame timing is claimed. Windows
and manual VoiceOver remain deferred.

Final contextual desktop regression passes in 3.5 seconds, including the default
open dedicated graph. The full reference repeat and exact workspace restore
passed in a fresh driver instance (66 seconds). Production package assembly,
formatting, source-boundary lints and signature/driver-exclusion verification are
recorded separately; these do not substitute for the outstanding large-board
and visible timing/fault gates.

## Source and design record

Primary files: `ui/src/App.svelte`, `ui/src/lib/Workbench.svelte`,
`ui/src/lib/GraphPanel.svelte`, `ui/src/lib/features/operations/`,
`ui/src/lib/features/pickers/`, `ui/src/lib/features/graph/`,
`ui/src/lib/features/onboarding/`, `ui/src/lib/features/reports/`, preferences,
Settings and bindings. Backend additions are `crates/kokin-store/src/workspace.rs`,
migration 8/golden schema, `crates/kokin-graph/src/explore.rs`, evidence attachment,
`src-tauri/src/workspace.rs`, `src-tauri/src/reports.rs`, and additive views and
operation dispatch. Export, binding, graph/store and desktop tests cover those
contracts. Historical increment reports remain intact.

Visual source remains selected Night Ledger concept 8 and the previously audited
DesignCode raster reference. No newly accessible editable Figma node, Code
Connect mapping or prototype animation is claimed. Existing semantic dark/light
tokens, native CSS disclosure transitions and reduced-motion behavior are reused;
no motion library is introduced. New graph interaction is direct positioning,
pan/zoom/focus with teardown; camera/layout never changes analytical confidence.
