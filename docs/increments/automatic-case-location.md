# Automatic case location

Date: 2026-10-12.

Creating a case with a blank location now generates a unique `Case-<random>.kokincase`
folder under the OS application-data directory's `cases` folder. Rust resolves
the location; a case name is metadata and is never used as a path component.
Explicit paths retain their previous behavior. Existing locations are refused.
Opening an existing case still requires a path.

The actual folder is returned in the existing CaseView and shown on the recovery-key
screen and in case details. Recovery acknowledgement and encryption are unchanged.
The command name, frontend arguments and response schema are unchanged; empty
creation paths now have the documented default meaning.

Changed: src-tauri/src/lib.rs, src-tauri/src/session.rs, src-tauri/tests/session.rs,
ui/src/lib/CaseGate.svelte, ui/src/lib/CaseGate.test.ts,
ui/src/lib/RecoveryKey.svelte, ui/src/App.svelte and README.md.

Verification: 38 frontend tests pass; Svelte check has zero errors/warnings;
production frontend build passes; 11 Rust session tests and 6 binding/security
contract tests pass. Regression coverage includes blank/explicit paths, mandatory
open paths, unique default folders, path containment despite a traversal-like case
name, preserved existing files and reopening. Cross-reference and SQL-ordering
lints pass. Windows native verification and a new packaged release were not run.
