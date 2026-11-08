# ADR-0020: Guarded security compatibility patches for Rust 1.85

- Status: implemented; verification is recorded in increment 35
- Scope: dependency source compatibility and security, no storage/API changes

The dependency gate found `quick-xml` pre-0.41 vulnerabilities, the `time`
RFC 2822 comment recursion advisory, and tokenizers' unmaintained `paste` macro.
Their straightforward upstream upgrades do not all fit the required Rust 1.85
baseline. Broadly suppressing their advisories or raising that baseline is avoided.

Three local sources retain their original licenses and recorded crate checksums:

- `plist` 1.8.0: manifest-only update to patched `quick-xml` 0.41.
- `tokenizers` 0.21.4: manifest-only alias from `paste` to maintained `pastey` 0.2.3;
  runtime/tokenizer code and the pinned BGE pipeline are unchanged.
- `time` 0.3.45: backport the upstream 0.3.47 nested-comment depth limit of 32,
  threading its depth through `cfws`/`comment`/`ccontent`. No later timezone or
  casting/let-chain changes are imported, and its version is not misrepresented.

RustSec checks versions rather than these patched bytes. The single time advisory
ID therefore has a documented exception **only with mandatory source and lock
guards** in `scripts/lint_dependency_patches.py`, also exercised by the default
suite. It refuses registry copies, additional versions, removed patches or changed
security code. A deep-comment regression verifies bounded rejection, and ordinary
RFC 2822 dates remain accepted. No unfixed vulnerability is sanctioned.

The yanked `chacha20` 0.10.1 transitive version is updated to 0.10.2 without changing
Kokin encryption algorithms, keys, persisted formats or command contracts. Full
case/blob/package acceptance remains required. Remove these source patches when
supported upstream dependencies provide the same fixes on the accepted baseline.
