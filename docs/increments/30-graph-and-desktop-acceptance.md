# Graph exploration and desktop acceptance

Date: 2026-10-08. Personal-use development; release gate remains open.

## Graph exploration

An entity detail now exposes **Explore relationships**. Indexed adjacency expands
stored records breadth-first, with 1–4 hops, 1–300 records, 1–128 links per direction
per record, a hard 1,000-edge cap and a 500 ms traversal budget. Limits apply during
traversal. Interrupted reads return cancellation; case close cancels the operation.
The interface shows completed hops and warns when traversal is truncated.

The scrollable SVG shows directed relationships. Nodes open canonical entity
details; edges open their own supporting/opposing evidence. An HTML list provides
an alternative for keyboard access and long names. Layout and distance describe
traversal, never confidence. Stored alias records remain distinct and are labeled;
this is not a canonicalized analytical graph. No schema migration or new persisted
facts were needed. Pan/zoom, layout optimization and graph editing remain later UX work.

Backend contracts check truncation, stable ordering, cancellation, included edge
endpoints and evidence attribution. A component test checks truncation disclosure
and evidence navigation. Svelte checks report zero errors/warnings; the UI detector
reported no findings for GraphPanel and Workbench.

## Desktop workflow

The full desktop test uses normal controls for case creation, replay collection,
explicit extraction, observation-to-source navigation, two different entity types,
evidence-grounded relationships, separate human review and relationship confidence,
search, activity verification, graph expansion, close/reopen, export/verify and
restore. It repeats into a fresh case with networking denied. Persisted rows are
compared exactly after reopen and restore, including identities, assessments,
lineage and evidence links; source text is read after restoration.

Debug-only fixture controls supply picker selections and replay captures and read
verification snapshots. They never create entities, relationships or assessments.
Picker selections still pass the ordinary case-bound token validation. Replay
misses fail without live fallback. The module and commands are feature-gated;
release builds refuse the desktop-test feature, and ordinary frontend builds
remove the driver import. Embedded WebKit option clicks do not emit change, so
this test dispatches normal select change and SVG click events through the test
driver. The runner also warns from its generic browser.execute compatibility shim;
these specs use direct selectors and browser.tauri.execute rather than that shim.
Bounded Mac inspection checked the graph controls, focus, labels and arrows;
a label overlap found during inspection was corrected.

The desktop exercise found and fixed empty analytical choices in newly created
cases: type/scale initialization now happens on creation and both opening paths,
before any form or write depends on them. A fresh-case/reopen regression checks
both entity types and all seven scale dimensions.

## Verified results

Both embedded-driver Mac desktop specs passed, including the full eleven-step
offline repeat. All 344 Rust tests and 29 frontend tests passed. Rust formatting,
Clippy, Svelte (zero errors/warnings), production frontend build, network boundary,
SQL ordering, research and cross-reference checks passed. Ordinary production
frontend assets and the normal Rust dependency tree contain no WDIO driver.
The existing dependency gate still requires cargo-deny in CI.

## Remaining gate

Windows execution and bounded manual platform/accessibility checks, integrated
fault injection, reference-machine search performance, dependency review and a
personal-use tagged build remain open. The explicit CI workflow is prepared for
Mac and Windows but has not been dispatched. Graph development follows the user's
request to begin expansion alongside desktop acceptance; this does not certify
release readiness or enable media, connectors, AI or maps.
