# ADR-0018: fixed sqlite-vec initialization adapter

Accepted in the owner's expansion implementation plan, 2026-10-13.

Application crates retain `unsafe_code = forbid`. A separate, small adapter has
`unsafe_code = deny` with a single private boundary exception. It declares the
SQLite extension callback ABI explicitly, registers only statically linked
sqlite3_vec_init once, and links against the same libsqlite3-sys SQLCipher build
as case storage. No transmute, arbitrary pointer, extension path, or runtime
extension loading is exposed. The callback remains valid for process lifetime;
SQLite owns arguments passed to it. Initialization happens before opening cases.
Vectors live in the encrypted case database and are derived, rebuildable data.
Missing model/runtime resources never prevent opening facts or workspaces.
