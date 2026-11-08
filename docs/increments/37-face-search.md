# Increment 37 — local faces and combined discovery

Search → **Faces and related evidence** opens the dedicated photo/attachment workflow. It supports explicit single/batch face analysis,
ranked case-local comparisons, same-attachment face/text intersection, suggested
complete-link groups, analyst-reviewed person links, and cited source previews.
Person dossiers show reviewed faces. Face analysis keeps text/OCR coverage unchanged; source previews do not require reinstalling face models. Existing word/OCR search and opt-in
semantic/hybrid retrieval remain available for related names and topics.

## Setup

On Apple Silicon macOS, prepare the existing media and ONNX resources first, then:

```sh
python3 scripts/prepare_faces_mac.py
cargo build -p kokin-media --features engine --bins
python3 scripts/bundle_media_mac.py
node ui/node_modules/@tauri-apps/cli/tauri.js build --debug --bundles app --config src-tauri/media-packaging.conf.json
```

The face/video preparer verifies a pinned LGPL FFmpeg source archive and the
fixed ONNX face runtime, builds the contained video helper, and keeps all native
notices/source material beside it. It does not install models or Docker.
`python3 scripts/prepare_faces_mac.py --offline` verifies prepared native outputs.

In Search, choose **Faces and related evidence**, then open **Local analysis and resources** and install/import both reviewed
face resources. Explicit resource downloads disclose destinations and hashes.
Choose an imported attachment, page or video range, then **Analyze faces**.
Select a face and optionally enter names/topics before **Search case**. An optional
encrypted case setting queues analysis after new supported imports; it defaults
to off and stops when the case closes. PDF automation covers page one only.

For optional automatic web discovery, follow `deploy/searxng/README.md`. Queries
are text-only and require online mode and query-specific disclosure. Raw JSON
responses enter encrypted Evidence before extraction. Destination pages are
collected separately with the existing Collect URL controls. Browser handoff
opens FaceCheck/PimEyes without uploading photos; providers may charge or limit
searches. There is no hosted face API in this increment.

## Bounds and interpretation

- Existing image/PDF input bound: 64 MiB and 16 million decoded pixels.
- Video: 512 MiB, selected range within 24 hours, 2-second sampling, at most 1,800
  sampled frames. No audio, live cameras, or persistent plaintext media files.
  Frame rotation is applied. Worker/decoder process groups have a monitored 1 GiB
  RSS ceiling and 30-minute deadline; cancellation terminates the group. The private worker protocol streams compact image payloads with a 512 MiB output cap.
- At most 100 artifacts and 10,000 detections per atomic analysis batch. A failed
  batch preserves its prior index; a partially sampled video records its coverage.
- Result cards show at most 50 appearances; all source timestamps remain available through the paginated face browser. Face vectors have 128 dimensions and never enter the 384-dimensional text
  index. Exact face search scans at most 100,000 current faces; grouping is bounded
  to 10,000 and supports cancellation. Stable complete-link assignment requires
  every pair to meet 0.5 cosine similarity. Groups do not propagate person labels.
- Candidate threshold defaults to the OpenCV SFace LFW example's 0.363 cosine.
  It is an initial retrieval setting, not calibrated identity confidence. The
  two public-domain test portraits and their image variants are functional
  verification, not population-level accuracy/bias validation.
- Reviewed identities require a rationale. Scores and nearby names never create
  identities automatically. New person creation plus review is atomic.
- Reruns mark replaced detections stale without erasing review history. Video reruns supersede only the frames actually sampled, preserving sightings from overlapping jobs with different offsets. Source
  shredding clears live derived crops/vectors. Existing encrypted package export
  includes the face index and reviews; external copies retain normal export caveats.
- Native inference is available only on the verified macOS bundle. Other platforms
  report that containment is unavailable. Existing indexes remain encrypted case data.

## Verification commands

```sh
cargo test --workspace
npm --prefix ui run check
npm --prefix ui test
KOKIN_FACE_TEST_MODELS=/path/to/models KOKIN_FACE_TEST_RUNTIME="$PWD/src-tauri/native/face-onnx/libonnxruntime.1.31.0.dylib" cargo test -p kokin-faces --features inference --test native -- --ignored --nocapture
KOKIN_FACE_TEST_MODELS=/path/to/models cargo test -p kokin-media --features engine --test faces contained_image_pdf_video_orientation_and_cancellation -- --ignored --nocapture
cargo test -p kokin-net --test discovery -- --ignored --nocapture
```

Explicit native tests need permission for macOS to apply its own worker sandbox.
The connector fixture test owns 127.0.0.1:8088, so stop local SearXNG first.
Desktop face acceptance uses `ui/wdio.faces.conf.mjs` (a scoped WDIO 10 compatibility shim for the installed Tauri service) and `ui/tests/desktop/faces.spec.mjs` with staged models
specified by `KOKIN_FACE_TEST_MODELS`; it imports exact reviewed files through the
resource installer instead of downloading implicitly.


For that acceptance build set `VITE_DESKTOP_TEST=1` and use `--features desktop-test --config
src-tauri/media-desktop-test.conf.json`, then run from `ui`:

```sh
KOKIN_FACE_TEST_MODELS=/path/to/models npx wdio run ./wdio.faces.conf.mjs
```

The regular bundle uses `media-packaging.conf.json` without test commands.
For a local debug bundle, apply `codesign --force --deep --sign -` to the finished
`.app`, then verify it with `codesign --verify --deep --strict`. This seals the
bundled native resources; it does not provide distribution signing or notarization.


Additional video acceptance uses `scripts/face_fixture_video.swift` to encode the
public-domain astronaut fixture into six-frame H.264/MOV clips, once at rotation
0 and once at rotation 90. Give it new output paths named `h264-upright-v2.mov`
and `h264-rotation-v2.mov`, then set `KOKIN_FACE_TEST_VIDEO_DIR` to their directory
and run all ignored media face tests. The fixture encoder needs macOS encoder
access; it is a developer utility, not an application dependency. The additional
tests verify H.264 decoding, matrix rotation, and the 1,800-frame decoder cap.

`verify_face_reference.py --models-dir /path/to/models` compares real worker
outputs with OpenCV 4.13's reference detection/alignment/recognition pipeline;
install OpenCV in a separate validation environment, never as an app dependency.

## Verified on Apple Silicon, 2026-10-19

- Workspace Rust tests and separate documentation tests passed. Face persistence,
  bindings, encrypted restore, shredding, overlapping video sampling, and version
  isolation contracts passed; four native media acceptance tests passed, including
  H.264 rotation, frame limits, and cancellation during processing.
- Frontend: 70 tests passed; Svelte check reported no errors or warnings, and the
  production frontend build passed.
- Both real desktop workflows passed: face model installation, analysis, review,
  groups and reopen; and the existing collection/analysis/export/restore workflow
  twice. Live loopback SearXNG discovery returned cited evidence. Upstream engine
  failures are surfaced as partial results.
- OpenCV reference embedding cosine agreement exceeded 0.9992 for both licensed
  portraits. A synthetic 10,000-face encrypted case completed exact retrieval,
  ranking, deduplication and bounded hydration in 170 ms in one debug run.
  Measurements and their limits are recorded in
  [the benchmark receipt](../benchmarks/face-search-mac.json).
- The Search face workspace was visually checked in the native app. The normal
  bundle excludes the desktop test controls.

These are functional acceptance checks. Broader real-world threshold and accuracy
validation remains necessary before treating this pre-alpha as a release.
