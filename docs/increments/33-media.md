# Media delivery — work in progress

2026-10-13. This increment is not the completion of the four-workspace plan.
Connectors, AI and maps have not yet been implemented in this continuation.

Implemented: a first-party media worker using pdfium-render 0.9.4 and PDFium 7881
(V8/XFA disabled), image 0.25.6, EXIF, orientation, previews, DCT perceptual hashes,
and optional Tesseract OCR. Input: 64 MiB; decoded images: 16 million pixels;
worker watchdog: 90 seconds/512 MiB; output: 16 MiB; PDF text: first 128 pages,
4 MiB/10,000 observations. The first TIFF frame is explicitly partial. Image
metadata/native PDF text never implies visual interpretation or authenticity.

Mac sandbox denies networking and unrelated file contents. Its exact `/`
DIRECTORY-only lookup rule permits native initialization, not recursive reads.
The initial proposal was rejected by automatic review; ephemeral proofs showed
unrelated synthetic file reads remain denied, and the narrower directory-only
rule passed review. Rust host resolves canonical resource paths before launch.
No case key or database reaches the worker; commit remains generation-bound.

Migration 9 appends media derivatives. Preview and transformation snapshots are
encrypted blobs with lineage; reprocessing supersedes observations. Media citation
verification checks intact original bytes and an encrypted transformation snapshot.
This is labelled `derived_exact`, never original-byte exactness. OCR words are
not independently verified against source pixels. Inspectors and reports retain
that caveat. Package round-trip preserves derivatives and supersessions.

The optional English resource has an explicit local-import flow with size,
destination, license and SHA-256 disclosure. Existing installed data was verified
against tessdata_fast commit 87416418657359cb625c412a48b6e1d6d41c29bd, upstream
blob bbef4675053b5b468cdb477053e28b1c698ba08e, SHA-256
7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2.
No resource is copied or downloaded automatically. Brokered resource download
and a reusable global resource job controller remain to be completed.

Passed: real sandboxed PDF text/OCR, malformed PDF/page bounds, synthetic raster
formats, EXIF rotation, pre-cancellation; owned-loopback and unrelated-file denial;
encrypted snapshot/citation/supersession/package restore; Svelte check, 58 frontend
tests, application Clippy. Native tests are explicit and separate from the default
suite; production contains no test probe or fixture command.

Remaining media gates: desktop acceptance, scan-only PDFs, oversized/hostile image
coverage, during-processing cancellation/worker crashes, final packaged regression after the last changes, full explicit download/resource
job flow, and remaining integration acceptance.
Generated native engines are ignored and not committed. The installed-library
packaging script is a developer probe, not an approved release distribution step.
Visible production timing, Windows and manual VoiceOver remain deferred.

Owner approved the scoped native license exceptions (ADR-0017). A pinned engine
registry and opt-in developer preparation script now reject unknown OCR libraries
and notice identities. The actual media bundle built at 35.7 MiB, and its
packaged worker passed PDF/OCR acceptance and ad-hoc signature verification.
Rendering pages is now separate from extraction and never creates/supersedes
observations. Re-extraction links matching replacement observations. Imports
can cancel between chunks and at the atomic commit boundary; source files remain
intact and partial encrypted staging is removed.

Rust 1.85.1 workspace and native-wrapper checks pass after resolving existing
incompatible lockfile entries. lol_html 3.0.1 is retained with a documented
syntax-only compatibility patch (and unused-binding cleanup), preserving its
security fixes and API. The full deterministic workspace suite passed on 1.85.1;
follow-up changes receive affected checks before final acceptance.
