# Reliability and expansion continuation

Accepted 2026-10-12. Preserve the current Night Ledger work. Visible production
timing, Windows and manual VoiceOver are deferred and do not block this sequence.

## Reliability changes

Blob publication now writes an exclusive encrypted staging file, synchronizes
its complete bytes, wraps the key before publication, and uses a no-overwrite
hard link. Interrupted reads, disk-full/short writes, publication and cleanup
failures are deterministic tests. Existing ciphertext and wrapped references are
never replaced during deduplication. Cleanup failure is reported as an error.

Package writing adds test-only thread-local I/O faults and explicit intermediate
staging cleanup before publication. Restore publication uses rustix's safe
atomic NOREPLACE operation on Mac/Linux, preserving even a racing empty directory.
The Windows path remains unverified; no first-party unsafe code was added here.

The real maximum-board expansion on the 100k-observation/20k-entity/300k-edge
corpus loads 300 stored-record nodes and 1,000 edges with aliases, parallel edges,
cycles, a self-loop, a disconnected seed, direction filters, cancellation and
preserved grounding. Release timing: n=30, p50 167.5 ms, p95 169.7 ms, p99 170.1 ms.
These warm reference-machine results are not universal or cold-disk guarantees.

The desktop-test build uses the same diagnostic corpus, renders all 1,300 SVG
links without document overflow, filters without deleting loaded facts, preserves
its mounted canvas across navigation, saves negative positions/pins, and reopens
the exact board revision. Starting-record controls are bounded visually; all
selected records remain accessible. Graph navigation refreshes its saved-board
list; aliases open their stored identity and let the backend retain the redirect.

## Expansion sequence

Media → DNS/RDAP, crt.sh, Wayback → optional local AI/semantic search → offline
maps → integrated acceptance. Native media/embedding engines are bundled with a
250 MiB app target; model/language/basemap data remains opt-in. The approved narrow
sqlite-vec initialization adapter receives its own safety ADR and review; other
application crates retain unsafe_code=forbid. No licensing policy or PR #1 adoption.

New destinations remain absent until their end-to-end implementations pass.
Translation and challenge-assistant workflows receive separate adoption reports.

Desktop-test measurements (Apple M4 Pro, 24 GiB): driver-inclusive mount 781 ms;
ten navigation cycles 82–98 ms. Host RSS 133.6 MiB before mounting, 143.8 MiB
after navigation, 207.7 MiB after close/save/reopen work. These stages perform
different work and do not establish a memory-leak result; WebKit memory cannot
be attributed through this harness. Raw measurements are recorded in
`docs/benchmarks/maximum-board-desktop-test-mac.json`.

The eleven-step offline desktop workflow passes twice (25.3 seconds in the
desktop-test harness), including saved boards/reports and exact package restore.
The inspector keyboard assertion dispatches a DOM key event; the maximum-board
spec separately exercises native driver keys. No visible production timing claim.

Fresh release merged-search measurements: n=30, p95 70.9 ms warm, 72.5 ms with
SQLite cache eviction, 71.9 ms with concurrent audit writes (182 writes). All are
within 200 ms. Cache eviction does not evict the OS cache or demonstrate cold disk.
