# ADR-0019: Scoped public CA trust-root data notice

- Status: accepted by the owner, 2026-10-14
- Scope: `webpki-roots` only, CDLA-Permissive-2.0

The Rustls HTTPS broker uses public certificate-authority trust-root data. The
crate's data license is absent from Kokin's exact dependency allowlist. The
owner approved a crate-specific exception and bundling its notice. The general
allowlist remains unchanged; the native media exceptions remain separately scoped.

Keep TLS hostname verification and the existing vetted-address boundary. This
exception grants no connector, model-download, or collection consent. The
original notice is bundled under `native/trust-roots/LICENSE`.
