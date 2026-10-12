# Design QA — Night Ledger foundation

## Settings/layout continuation — 2026-10-12

Scoped verification passed: light settings, dark workspace and 125% scaled narrow desktop captures were inspected; readable theme changes, divider geometry, modal focus and reduced motion were verified. Screenshots and measurements are under docs/ui-rebuild/screenshots/settings-2026-10-12/. Focused contrast checks pass; this is not comprehensive screen-reader/WCAG certification. See docs/increments/ui-night-ledger-settings-layout.md for proof, performance limits and remaining platform/release gates.

## Core views continuation — 2026-10-12

Evidence/library, search/coverage, artifact/subject detail, activity and collection/analysis/package form groups now follow the same outlined visual system. Focused native captures are in docs/ui-rebuild/screenshots/views-2026-10-12/. Two bounded inspection rounds found a narrow search-filter layout issue; the final CSS stacks filters and prevents checkbox shrinking. Captures precede that final adjustment. The 35-test frontend suite and repeated offline desktop workflow pass. Full redesign, comprehensive accessibility, Windows and runtime performance gates remain open; see docs/increments/ui-night-ledger-core-views.md.
**Result: passed for the initial foundation/entity slice, with the documented intentional differences below. The full frontend rebuild is not complete.**

Reference: outer workspace `Design concepts/08-night-ledger.png` (1487 × 1058).
Actual: `docs/ui-rebuild/screenshots/first-entity.png` and `first-entity-details.png`, plus replay captures. Native CSS viewport 1487 × 854; monitor height prevented an exact-height capture. Comparison covers the common-width top and scrolled details rather than claiming a full-height pixel match.

## Evidence and findings
- Near-black shell, restrained outlines, blue actions, wide source lead, identifiers/relationships, activity and contextual case/review column match the chosen hierarchy.
- Local icons and original decorative banner load in the native webview; asset diagnostics are saved beside captures.
- Citation copy comes from backend source excerpts; unavailable excerpts are labeled. Seven dimensions and attributed disagreements are retained, with no synthetic aggregate confidence.
- Search, case metadata, library/back controls and evidence navigation work with actual case reads.
- Focus indicators and reduced-motion CSS are present; search shortcut and focus-return are covered. Comprehensive screen-reader, contrast, scaling and Windows verification remain subsequent gates.
- No unresolved P0–P2 visual defect was identified in the inspected slice. Forms and other legacy views are outside this slice and explicitly await migration.
- P3 follow-up: refine lower-card spacing as view modules migrate; evaluate banner size alongside measured asset/startup costs.

## Intentional differences
Synthetic test case labels/counts differ from mockup data. Existing application icon replaces mockup branding. Native font replaces Inter pending a separate license decision. Operational navigation contains the four supported destinations; specialized actions retain their real entrypoints. All seven confidence scales appear instead of the illustration's abbreviated set.

## Verification
34 frontend tests pass; native eleven-step offline acceptance repeats twice, including identifier evidence, close/reopen and export/restore. Production build passes. Baseline and after captures are preserved; the complete audit and phase report record measurement and platform limitations.
