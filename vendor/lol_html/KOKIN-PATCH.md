# Rust 1.85 compatibility patch

Upstream: lol_html 3.0.1, BSD-3-Clause, crates.io checksum 5adbb62638edf7e6bc88835cd3ea388bdd53382af42045da0e414ea78aa0c91a.
Source: https://github.com/cloudflare/lol-html/releases/tag/v3.0.1

The crate declares Rust 1.85 but one let-chain requires Rust 1.88. The behavioral patch rewrites `flush_encoding_change` as equivalent nested ifs. A single unused pattern binding is prefixed with `_`
so the local path dependency remains warning-free.
Retain 3.0.1's security fixes and builder API. No parser behavior, memory limit,
markup rendering, licensing or application unsafe-code policy changes.
Remove this patch when upstream compiles on the supported compiler. This is a
third-party dependency, explicitly excluded from the application workspace.
