# Night Ledger: commands and graph navigation

Date: 2026-10-12. Scope: continue selected concept 8; preserve existing investigation controls. Windows work is deferred at the user's request.

## Work completed and files

- `ui/src/lib/features/commands/CommandPalette.svelte`: searchable native modal, arrow-key selection, Enter activation, Escape dismissal, focus return, unavailable-action explanations and command error handling.
- `ui/src/lib/Workbench.svelte`: Commands button and Command/Control K; Command/Control F remains explicit case search. Commands navigate the four implemented libraries, import through the existing picker, reveal analytical/package controls, open the selected entity's graph, follow/close detail history, toggle the inspector, open settings and close the case. No unimplemented feature is advertised as a command. VoiceOver's Control/Option chords do not trigger graph pan or detail-back shortcuts.
- `ui/src/lib/GraphPanel.svelte`: responsive SVG camera, empty-canvas mouse pan, keyboard arrows/+/-/0, cursor-anchored Control/Command wheel zoom, fit and seed/loaded-record focus. Canonical entity and relationship evidence links and the alternative HTML list remain. Node placement and endpoint lookup are linear rather than repeated scans. Camera updates transform one group. Finite 300-record columns can fit; zooming in restores readable labels. Ordinary wheel scrolling remains available. Unmount cancels an operation whose ID arrives late, and re-expansion clears stale focus selection.
- `ui/src/lib/features/graph/viewport.ts`: independently tested camera geometry; no graph dependency or physics layout.
- `ui/src/lib/features/settings/SettingsDialog.svelte`: explicit Local AI — planned section. `kokin-ai` remains a stub; this build has no model installation, Ollama inference or semantic search. No download or inference is invoked.
- Unit tests, `ui/tests/desktop/workflow.spec.mjs`, native captures and `docs/ui-rebuild/profiling/`: verification evidence and reproducible process profiling. Screenshot-only instrumentation finishes finite decorative animations to avoid waiting on animation promises while the screen is locked; it is not compiled into production.

## Design, motion and functionality

Uses existing Night Ledger outline surfaces, semantic colors, native controls and modal tokens. No new Figma node, Code Connect mapping or prototype motion was verified. Palette entry reuses the existing proposed 160 ms disclosure token (motion class C); no new timing system. Pan/zoom/focus are immediate so camera movement cannot lag user input; reduced-motion preferences continue to govern decorative entry. Existing settings/light theme/inspector behavior remains.

No Rust backend, typed command interface, migration, dependency, license, network boundary, encryption, provenance or audit-chain change. Collection/import/extraction, all seven confidence dimensions, recorded human/automated actors, merge attribution, opposing evidence, paging, explicit search/coverage, case close/reopen and package verification/restore are retained. Graph limits and truncation warnings remain; position never assigns confidence.

## Verification

- Svelte check: zero errors/warnings. **50 tests across 11 files pass**. New checks cover cursor anchoring, zoom limits, fit at maximum node count, focus centering, command filtering/disabled actions/keyboard selection, graph keyboard/mouse pan, VoiceOver chord isolation and ordinary-wheel behavior, evidence links and late operation cancellation after unmount.
- Native Mac offline eleven-step workflow passes twice in one run (25.8 s before capture instrumentation). Final native capture/acceptance run also passes in 25.7 s. New native assertions cover zoom/fit, keyboard pan, seed focus, palette Enter navigation/Escape, and subsequent relationship evidence navigation; reopen and export/verify/restore preserve original identities/counts/lineage.
- Production Mac application built with no desktop-test feature; ad-hoc signature verifies. Frontend and executable scans exclude driver/test-control/harness markers. No release or push requested/performed in this phase.
- Mechanical detector: no findings on the changed interfaces. Two existing narrow Svelte graph-region heuristic exceptions explain its documented keyboard interactions; no detector/file/rule ignores added. Graph and palette screenshots are synthetic acceptance data, not product fixtures.

## Production profiling and limitations

Apple M4 Pro, arm64, 24 GiB. Release executable launched in fresh processes with warm filesystem caches, sampled every 100 ms for 10 seconds per run. Method/source/raw results: `../ui-rebuild/profiling/`. Final production samples: host RSS **101696 / 101664 / 101664 KiB** (about 99.3 MiB); newly spawned WebKit RSS **89712 / 89952 / 89328 KiB**. Estimated combined RSS **186.9 / 187.1 / 186.5 MiB**. First new WebKit process observed at **565.1 / 166.0 / 167.7 ms**; the first sample also follows the rebuild/signing. All three host processes remained alive after 10 seconds. These startup proxies include process-poll sampling overhead and do not indicate that the interface was ready. Initial exploratory data overlapped native tests in one sample; it was replaced with these independent final measurements.

Precise visible-ready startup, frame/input responsiveness and populated-case production navigation were not measured. After unlocking, a computer-assisted production smoke check confirmed the visible case gate, Settings opening, its honest Local AI — planned status, and keyboard Escape returning to the case gate. The running executable path was verified against the current production bundle; this check does not use the embedded test driver. Process RSS is not physical footprint and shared pages can be double-counted. WebKit ownership inferred by new PIDs needs to be treated as an estimate, not an authoritative total. These empty-screen samples are not evidence of leak freedom or large-case performance.

Production frontend build: 563 ms, CSS 36.54 KB (gzip 7.07), JS 166.05 KB (gzip 56.00); local banner remains 1481.36 KB.

Manual VoiceOver checks are **deferred at the user's request**. After the Mac was unlocked, VoiceOver was temporarily enabled during setup; the user deferred the checks before a screen-reader pass was completed. Its original off state was verified afterward. No VoiceOver pass is claimed. Automated native keyboard checks and the production smoke check remain distinct from screen-reader observations. Windows build and checks are also deferred by the user.

Remaining release gates: deferred VoiceOver and Windows/Narrator checks; visible production startup/input/frame timings; representative large-case and repeated-navigation memory measurements; broader visual regression coverage. Local AI implementation remains a separate accepted roadmap milestone after core reliability.
