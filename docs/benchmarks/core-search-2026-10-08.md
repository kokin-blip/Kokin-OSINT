# Merge-aware search measurements — 2026-10-08

Local Apple Silicon macOS, Rust 1.97 debug build, SQLCipher; 100,000 observations,
20,000 entities, 300,000 relationships, 200 artifacts and one active merge.
Thirty samples per measurement, query `mercer`, page size 50. This is a local
engineering probe, **not** sign-off on the original release/reference setup.

| Final query shape | p50 | p95 | p99 |
|---|---:|---:|---:|
| Warm | 150.1 ms | 151.3 ms | 151.4 ms |
| SQLite cache evicted before each query | 150.5 ms | 152.2 ms | 152.4 ms |
| Concurrent audit appends (10 ms pause between appends) | 162.1 ms | 275.1 ms | 299.0 ms |

490 concurrent appends completed. Warm and evicted-cache results meet the
unchanged 200 ms target locally; concurrent latency exceeds it. Other Rust tests
and desktop verification were running during this sample, so scheduling/crypto
load can affect the tail. Record a quiet reference run before closing R-014.

The cold measurement calls `PRAGMA shrink_memory` on the SQLCipher connection.
It does not purge the operating-system filesystem cache, and therefore must not
be presented as a cold-disk result. A true cold-cache reference measurement
remains pending. The concurrent writer appends audit records; it does not model
all extract/index/update workloads or the UI mutex queue.

Intermediate forms failed: materializing all matching snippets reached p95
292 ms, while inlining the matched CTE still reached 420 ms under build load.
The final form limits the window to merge-affected entity rows, returns the
ranked rows directly instead of rejoining all matches, ranks compact keys and
builds snippets only after selecting the page. Search/count/facet tests retain
alias searchability, full pages, deterministic ordering and split behavior.

Run deliberately, with no other build load:

```sh
KOKIN_NETWORK=deny cargo test -p kokin-osint --test scale --release \
  merged_search_warm -- --ignored --nocapture
```

The probe asserts the unchanged warm target. Cold and concurrent measurements
are published diagnostics, not silently broadened guarantees.

## Subsequent run after compilation

The same final query and corpus subsequently produced:

| Measurement | p50 | p95 | p99 |
|---|---:|---:|---:|
| Warm | 156.7 ms | 179.1 ms | 204.7 ms |
| SQLite cache evicted | 154.4 ms | 157.4 ms | 158.0 ms |
| Concurrent audit appends | 154.9 ms | 156.7 ms | 156.7 ms |

393 appends completed; the probe passed its unchanged warm p95 target. The
variation between runs is retained rather than replacing the earlier concurrent
counterexample. Neither run supplies Windows/reference or operating-system
cold-cache sign-off. CPU scheduling and background desktop work remain confounders.
