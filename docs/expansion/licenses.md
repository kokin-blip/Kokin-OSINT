# Expansion source and dependency register

- OSINT Framework catalog: official Git blob `ff4107ba8904f04588636ccee1f61ef1e7540dbc`, retrieved 2026-10-20. MIT, copyright Justin Nordine; exact notice in resources/notices/osint-framework-MIT.txt. Metadata is publisher-supplied, not verified health. Source: https://github.com/lockfale/OSINT-Framework/blob/master/LICENSE.
- Username checks are first-party Rust implementations of public JSON response contracts. No archived GPL InstantUsername code is included.
- The inspected WhatsMyName.io catalog is research evidence only; it is not bundled or used at runtime. The original WhatsMyName dataset is CC BY-SA 4.0 and remains excluded under the current product dependency/data policy: https://github.com/WebBreacher/WhatsMyName/blob/main/LICENSE.md.
- Existing YuNet/SFace/ONNX/video resource exceptions remain exactly those documented in docs/native/face-and-video-license-review.md. No new native/model exception is implied.
- Optional provider integrations use official documented contracts and require the user's own account rights; code presence does not establish entitlement.
- New direct `zeroize` use in `kokin-net` reuses the already resolved `1.9.0` dependency (Apache-2.0 OR MIT); no new license exception. No CSV, browser, rendering or model dependency was added for this research slice.
- CLIP ViT-B/32 ONNX, C2PA libraries, HEIC/AVIF decoders, DOCX/PDF/XLSX renderers, crawler and extension dependencies remain unselected. Their licenses, exact pins, resource hashes, reference evaluations and delivery gates must be reviewed before enabling them.

## Phase 2 validation dependencies

Added from the already cached crates.io sources, with offline resolution and exact direct pins: `bs58 = 0.5.1` (MIT OR Apache-2.0, `check` feature reuses existing SHA-256), `sha3 = 0.10.9` (MIT OR Apache-2.0, Keccak-256 for EIP-55), and `bech32 = 0.11.1` (MIT, witness-aware Bech32/Bech32m). The new transitive `keccak = 0.1.6` is Apache-2.0 OR MIT. SHA-256/digest/tinyvec were already resolved. Their registry manifests/notices were inspected locally and package checksums are pinned in Cargo.lock. No native library, model, license exception or resource download was introduced. `cargo deny` remains a separate unavailable local gate; this is not an advisory audit claim.

Wallet reference behavior: [ERC-55](https://eips.ethereum.org/EIPS/eip-55) official test vectors; [TRON address formats](https://developers.tron.network/docs/account); Bitcoin developer [address overview](https://developer.bitcoin.org/devguide/transactions.html) and reviewed library source for Base58Check/SegWit rules. These pages were viewed through the Jules Work Chrome extension. No wallet key, balance lookup, transaction or provider call was performed.

## Hybrid continuation provenance

The first-party local research API and result/import/filter code add no dependency or model. REA inspected inert, previously acquired client bundles as research evidence only; no proprietary bundle code, private service endpoint or dataset was incorporated. OI spec-format and credit parsing follows the official public contracts linked in [phase-2-hybrid.md](phase-2-hybrid.md). Native worker acceptance reused existing reviewed engines and installed hash-matching English data; it introduced no resource/license exception.

## Phase 3 local media continuation

The first-party image recipe, encrypted query lineage, clipboard capture and bounded exact/pHash search add no dependency, model or native exception. They reuse the pinned `image` 0.25.6, PDFium 7881, existing contained workers, SQLCipher/blob storage and reviewed optional face resources. Native test packaging reuses the existing installed hash-verified English OCR resource. CLIP, HEIC/AVIF, C2PA and detector resources remain unselected/disabled; no bundle code, hidden endpoint or provider corpus was recovered or incorporated. No download, provider purchase or key creation was performed.
