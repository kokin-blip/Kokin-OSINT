# Increment 36 — operational offline maps

2026-10-14. Maps uses bundled MapLibre GL JS 6.14.0 and PMTiles 4.5.0. The older
upstream-example MapLibre version was replaced after its sanitizer advisory was
found; the final production npm audit reports zero advisories. Runtime notices,
styles/code and workers are bundled. No remote styles, sprites, fonts or geocoding
are used; attribution is escaped Svelte text rather than third-party HTML.

A Rust-owned picker creates a short-lived selection. Import revalidates file
identity, enforces PMTiles v3/header/section/zoom/compression/directory/range budgets,
checks copying/cancellation, hashes the archive, publishes without overwrite and
cleans staging/credits on failure. Only public basemap data belongs in this global
resource folder; basemaps remain outside encrypted cases and packages. The active
renderer reads through a native archive-ID/range capability, never an HTTPSource.
Invalid/unsupported archives/read/render failures stay explicit with a list fallback.

The optional world resource is a z0–z6 extraction from the fixed reviewed
`https://build.protomaps.com/20261011.pmtiles` build. First consent covers bounded
public metadata requests; a second approval covers the calculated tile transfer
and destination. The live metadata-only check passed: **43.0 MiB**, not the stale
~80 MiB estimate. Requests are TLS/vetted-address constrained, redirect-free,
rate/timeout/byte/range bounded, cancellable and ETag/size consistent. The full planet
is never downloaded. Local assembled bytes receive a SHA-256 identity; this is not
a publisher signature. The public resource remains subject to provider availability.
World installation is opt-in, separate from ordinary offline map use.

Grounded point locations require a verified source observation, manual coordinates,
uncertainty and rationale; entity links are optional. There is no silent geocoding.
Locations, revision-checked camera/layer preferences and analyst-rendered PNG
illustrations stay encrypted in the case. Points outside Mercator bounds remain
available in the paginated HTML list. Filtering changes the view only. Source/entity/
Graph/report navigation shares the existing inspector. Captions/reports include
source verdicts, coordinates, uncertainty, attribution and unavailable evidence.
Illustrations imply no confidence and remain available after restore without the
basemap archive.

The native offline test passed real synthetic MVT rendering (pixel assertion),
location creation/focus/zoom/fit, saved state, human/source navigation, image
publication and reopen. Instrumented map fetch/XHR recorded no remote requests,
with CSP/broker network denial as additional boundaries. Synthetic test archives
are cleaned from the global resource folder. Persistence/citation/malicious-markup/
stale-revision/range/truncation tests and package restore pass. Coarse-world extraction
also refuses partial transfers and preserves its selected tile bytes in offline tests.

The final owned Mac debug bundle passed six native specs: AI/maps, real installed
embeddings, twice-repeated eleven-step workflow, contextual forms, media and Sources
fixtures. Screenshots use clearly synthetic cases. Rust 1.85.1 full suite, strict
Clippy, typed-boundary/SQLCipher/citation/markup/network and dependency gates pass;
UI checks have zero errors/warnings and 63 tests pass. Final build details are
recorded below after verification. No production frame-rate/input-timing claims.

Known scope: no place-label/glyph layer, automatic geocoding or political-boundary
certainty. Unsupported tile compression/types fail visibly. Readable reports are
plaintext; case packages exclude global archives/models. Ollama/Qwen execution and
recommendations await an optional runtime. Windows, VoiceOver and visible production
timing remain explicitly deferred. No release is published.

## Mac production bundle verification

The final Rust 1.85.1 ARM64 production build completed. The app is **72.92 MiB**,
below the 250 MiB target, including the native media/ONNX engines and notices.
The executable contains no test-control/fault-command/WDIO markers; source feature
gates exclude the embedded driver. Signing remains ad hoc with no team identity
or notarization. This is a local build verification, not a published release or
visible production timing result. The owned loopback transport test passes status
JSON, redirect denial and prompt cancellation without an external runtime/model.

## Approved world resource acceptance

The owner-approved **43.0 MiB** transfer completed through the native two-consent
flow. The assembled 45,073,332-byte archive has SHA-256
`95120e4a039dc64a5c23ca0f0a4425bf2676e658012e02789dcb0d5c910caec1`
and remains installed under the disclosed global `resources/local/maps` folder.
The full planet archive was not downloaded. A subsequent native acceptance run
reused those installed bytes, rendered real vector geography (95 sampled colors),
exercised zoom, saved the camera and exported an attributed illustration with no
remote fetch/XHR map requests. It passed in 7.9 seconds. Test-only frame scheduling
avoids background WebKit throttling; this is functional rendering evidence, not
production FPS/input timing. An earlier test incorrectly retried after a completed
export because its old wait message remained visible; the status is now cleared
when rendering is ready and the acceptance harness recognizes completed operations.
The final Rust repeat reports 387 passed, zero failed, 12 deliberately ignored.

A separate installed-world run with **KOKIN_NETWORK=deny** passed rendering,
zoom, saved camera and attributed illustration export in 3.8 seconds, again with
zero remote map requests. It skips metadata/download entirely. Both timings are
functional test durations, not production latency measurements.
