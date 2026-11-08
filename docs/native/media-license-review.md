# Native media engine review (approved scope)

2026-10-13. This is a scoped native packaging proposal, approved by the owner on 2026-10-13; not a general license-policy
change. No release has been published from this work. The Rust allowlist is intact.

The requested PDFium/Tesseract/Leptonica path has been implemented and tested in
an isolated developer build. PDFium is pinned to chromium/7881 mac-arm64, archive
SHA-256 52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40;
its args.gn disables V8 and XFA. All supplied notices were retained locally.

Several native notices are outside the exact SPDX allowlist in deny.toml:
FreeType's FTL, the IJG portions of libjpeg-turbo, libpng's license, and the 0BSD
liblzma component reached through the installed OCR build. The owner approved those specific native exceptions before packaging. No GPL alternative, third-party executable connector, or new general
network path is proposed. The Tesseract, Leptonica, PNG/JPEG/TIFF/WebP/OpenJPEG,
archive and compression library identities must be recorded with their notices.

Approved scope (owner, 2026-10-13): narrowly permit these native license texts for the approved
media worker; bundle all notices and required FreeType credit; retain the general
Rust dependency policy; pin every native artifact identity and audit the installed
engine bill. A reproducible minimal OCR build should replace the exploratory
Homebrew closure and eliminate unused curl/archive/codec support where possible.

Alternatives: keep packaging deferred while retaining the verified developer
worker, or change the approved media-engine choice in a separate decision.
The current generated engine bill is a developer diagnostic, not a complete
release license audit or a reproducible engine installation manifest.
