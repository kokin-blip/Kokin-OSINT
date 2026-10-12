# Night Ledger settings, inspector and motion
Date: 2026-10-12. Scope: the requested next settings/layout/motion phase and bounded Mac accessibility/performance verification.

## Completed work and changed files
- `ui/src/lib/stores/preferences.svelte.ts`: validated, versioned display preferences; guarded storage reads/writes; OS appearance and reduced-motion listeners with teardown. Only theme, reduced motion, stronger focus, artwork visibility, sidebar collapse and inspector width persist. Case data, credentials and network mode do not.
- `features/settings/SettingsDialog.svelte`: native modal dialog with focus containment, Escape, focus return, dark/light/system appearance, accessibility controls, actual case location, actual case collection mode, reset controls and honest planned connector/AI status.
- `design/components/ResizeHandle.svelte`: pointer capture and desktop mouse drag, keyboard arrows/Shift/Home/End, Enter hide, bounded 260–480 px width, cancellation and listener cleanup.
- `Workbench.svelte`, `features/entities/EntityDossier.svelte`, `layout/CaseContext.svelte`, `App.svelte`: settings access, shared inspector width, temporary inspector visibility, skip link, modal shortcut isolation and optional decorative artwork.
- `design/tokens.css`, `design/Gallery.svelte`, `RecoveryKey.svelte`, `ErrorNotice.svelte`, `GraphPanel.svelte`, `Observation.svelte`, `features/operations/OperationsPanel.svelte`: semantic light palette, readable control contrasts, themed recovery/error/graph surfaces, consistent motion and gallery theme controls.
- Preference and divider unit tests, test-only PreferencesHarness, and `ui/tests/desktop/layout.spec.mjs`: functional/accessibility/performance probes. The new native probe is Mac-specific and skips Windows rather than claiming platform verification.

## Components and Figma references
Added SettingsDialog and ResizeHandle, reusing the current Night Ledger tokens and native controls. Continued the previously verified DesignCode dark/light/outline patterns; no additional Figma nodes or prototype timing were verified. Light semantic colors are a Kokin adaptation of those patterns, not a claim of exact Pro template reproduction. No new dependency, asset download or license-policy change.

## Motion implemented
200 ms sidebar grid transition; 160 ms initial detail, case-information, operation-panel and settings entry; 120 ms control border/press feedback. Theme backgrounds change immediately to avoid a transient dark-text/dark-button contrast failure. Case close and inspector hide remain immediate. There is no animation of confidence or evidence acceptance. OS reduced motion overrides the user preference. Source classification: timings are C (proposed Kokin motion); source visual states remain B, with no verified A prototype timings.

## Functionality preserved
All existing command interfaces and case/security/storage/network logic are retained. Settings changes only display state; network controls call the existing mode command and preserve per-collection disclosure/consent. Operation controllers stay mounted when hidden. Inspector hiding is temporary and does not remove assessments or change evidence; reopening restores its width. Existing search, pagination, actors, all seven dimensions, graph evidence navigation, case lifecycle and package lineage remain.

## Verification
- Svelte check: zero errors/warnings.
- **44 frontend tests across 9 files pass**, including preference validation/storage refusal, OS reduced-motion precedence/listener teardown, keyboard bounds/collapse and mouse drag/listener teardown.
- Native Mac layout/accessibility probe passes: 18 successive Tab presses stay inside the dialog, Escape restores focus, arrow/Enter controls and drag work, divider fills its grid row, reduced motion removes transitions, and a narrow window at 125% CSS scaling has no document-wide horizontal overflow.
- Native offline eleven-step investigation workflow repeats **twice**, passing in 24.7 s: collection/extraction, grounded entities/identifier/relationship, assessments, search/coverage, audit/graph, reopen and export/verify/restore preserve identities/counts/lineage.
- Production frontend build and driver/gallery marker exclusion pass; mechanical design detector has no findings after removing thick side borders. Documentation cross-references and diff whitespace checks pass.
- Focusable separator semantics follow the [W3C window splitter pattern](https://www.w3.org/WAI/ARIA/apg/patterns/windowsplitter/). Two narrow Svelte heuristic warnings are suppressed on this element with an explanatory comment; no file-wide or rule-wide detector ignore.
The embedded driver does not reliably deliver Home/End; these are verified in unit tests, with native arrows/Enter and mouse drag verified separately. Its existing addTauriApi hook warning persists even though the probes/assertions pass.

## Measurements and practical limits
Apple M4 Pro, arm64, 24 GiB. Latest single offline instrumented run, viewport 1487 × 856 CSS pixels:
- Sidebar event-to-two-animation-frames p95 **34 ms** over 12 toggles (observed samples 28–34 ms).
- Frame interval p95 **18 ms** over 157 callbacks. An earlier run measured 17 ms. This is near the 60 FPS goal but does not establish a universal 60 FPS guarantee.
- Host RSS after warm-up **119024 KiB**, then **119184 / 119280 / 119344 KiB** after three batches of 20 navigation changes: about 116 MiB, increasing 0.31 MiB across 60 navigations.
- Frontend document navigation: DOM interactive 7 ms, DOMContentLoaded 74 ms, load 75 ms. These are webview document timings, not cold native-process startup.
- No external HTTP(S) UI resource entries were recorded. JS heap measurements were unavailable.
- Production build: 535 ms; CSS 35.21 KB (gzip 6.89), JS 156.51 KB (gzip 52.70); local banner unchanged at 1481.36 KB.

The initial capture run showed a larger RSS rise; a bounded follow-up without PNG captures isolated navigation and produced the warm checkpoints above. RSS covers the instrumented native host, excludes separate WebKit processes and is not proof of production leak freedom. Production memory/cold-start profiling and large-case stress measurements remain release gates. The default suite stays deterministic and offline.

## Visual evidence and remaining work
Inspected light settings, dark workspace and narrow/scaled captures in two bounded visual rounds; fixed transient theme contrast and divider geometry. Screenshots and raw measurements: `docs/ui-rebuild/screenshots/settings-2026-10-12/`. Data is synthetic test data.
Measured text contrast: light body 16.43:1, light secondary 7.67:1, light secondary button 14.50:1, primary button 4.86:1. This is a focused check, not a full WCAG or VoiceOver certification.
Remaining: Windows native checks, manual screen-reader pass, production startup/total-memory and large-case profiling, command palette, graph pan/zoom/focus polish, broader visual regression coverage and personal-use release gate. No new release/push was performed.
