# Face and video native resources

Scope authorized by the owner's explicit face-recognition implementation request
(2026-10-19): YuNet/SFace local inference and an LGPL-only FFmpeg decoder. This
adds no general license exception to the Rust dependency policy.

## Pinned artifacts

- `resources/face-registry.json` pins the exact OpenCV Zoo commit, model lengths,
  SHA-256 hashes, upstream URLs, and preprocessing identity. YuNet's accompanying
  model license is MIT; SFace's is Apache-2.0. Copies are in `resources/notices`.
  These models are optional resources and are not included in installers.
- `resources/native-face-onnx-mac-arm64.json` pins Microsoft's ONNX Runtime
  1.31.0 archive and extracted files. MIT and third-party notices ship beside the
  library. Face inference uses a separate process and runtime directory; the
  existing text-embedding runtime is unchanged. The earlier 1.22.0 face worker
  reproduced upstream macOS exit bug microsoft/onnxruntime#24579; 1.31.0 passed
  inference and clean-exit acceptance.
- `resources/native-video-mac-arm64.json` pins FFmpeg 7.1.5 source. Configure
  disables GPL, nonfree components, network protocols, external dependencies,
  hardware acceleration, and FFmpeg programs. Only local MOV/MP4, Matroska/WebM,
  AVI demuxing and selected video decoders are enabled. No audio is processed.

The LGPL helper is a separate executable, statically linked to the configured
FFmpeg libraries. Its source, build instructions/configuration, LGPL text, and
complete matching FFmpeg source archive are distributed beside it. The helper's
own source is MIT to permit modification and relinking. Do not remove these
materials when distributing the native bundle. The application does not prohibit
replacement/rebuilding of this helper. Decoder availability is format-specific;
unsupported codecs produce an explicit job failure rather than a fake empty result.

Preparation is explicit: `scripts/prepare_faces_mac.py` verifies archives, builds
and signs the decoder, stages runtime notices, and records the installed helper
hash. `--offline` verifies prepared outputs without downloading. Models are
verified again immediately before worker use.

Packaging still requires the existing product-wide native review and unsigned
build limitations. This review records exact artifact licenses; it does not
assert ownership of faces, accuracy across populations, or rights over material
found by an external service.
