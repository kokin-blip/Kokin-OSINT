# Increment 5 — the egress broker's policy engine

Date: 2026-08-12 · Branch: `feat/phase0-foundations`

## Scope, and what is deliberately absent

`kokin-net` is the only crate permitted to open a socket (ADR-0010). This
increment builds its **policy engine, resolver abstraction, offline enforcement,
and rate limiter** — and **no live transport at all**. No HTTP client is linked.

That ordering is deliberate. PoC P4's kill criterion is that *any* bypass of this
guard halts connector work, so the guard should be built and attacked before
anything can depend on it. Shipping a working fetcher alongside an untested
guard would invert the incentive: the fetcher would be the thing that worked, and
the guard the thing that got adjusted until the fetcher stopped failing.

Live transport, record/replay `HttpCapability`, and secret scrubbing are
increment 5b.

## Files changed

| File | What it does |
|---|---|
| `crates/kokin-net/src/guard.rs` | Scheme allowlist, address classification, host allowlist, redirect policy. Pure logic, no I/O. |
| `crates/kokin-net/src/resolver.rs` | `Resolver` trait, `StaticResolver` for tests, `OfflineResolver` as the fail-closed default. |
| `crates/kokin-net/src/lib.rs` | `NetworkMode`, `EgressPolicy`, `vet_request`, `reconfirm_before_connect`, `RateLimiter`, `CappedReader`. |
| `docs/threat-model/attack-catalog.csv` | A-001..A-004 and A-011 moved `planned` → `partial`. |
| `docs/threat-model/README.md` | Status ratio corrected, with what `partial` means here. |

## The decision that makes this testable

**Resolution is injected.** Without that, the most important test in the crate
could not exist: proving DNS rebinding is caught requires a name that resolves to
a public address when vetted and a private one moments later at connect. That is
one line with `StaticResolver` and impossible to arrange reliably against live
DNS. It is also what lets the entire suite pass with `KOKIN_NETWORK=deny`.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace --all-targets
                                        72 passed, 0 failed
                                        (26 net + 23 store + 14 blob + 9 keys)
cargo clippy --workspace --all-targets  0 warnings (-D warnings)
cargo fmt --all -- --check              clean
privacy lint (local)                    no network usage outside kokin-net
```

Twenty-six broker tests. The ones carrying weight:

- **the PoC P4 address list is refused in full** — `127.0.0.1`, `::1`,
  `169.254.169.254`, `100.64.0.1`, RFC1918, `fc00::1`, `fe80::1`, `0.0.0.0`
- **`::ffff:127.0.0.1` is refused as its embedded v4 address** — the bypass that
  defeats a v6 check written without thinking about mapped addresses
- **DNS rebinding between vetting and connecting is caught** — the full sequence,
  public then private, against a resolver that flips mid-test
- **every resolved address must pass, not just one** — a name answering with both
  a public and a private address is a rebinding attack, and taking the first
  acceptable address would walk into it
- **a disallowed host or scheme is refused before any resolution happens** — the
  test resolver panics if called, because a DNS query is itself an observable
  event that leaks what was attempted
- **an exact host allowlist does not match a prefix** — `example.com` must not
  admit `notexample.com` or `example.com.evil.test`, the classic bypass
- **the byte cap stops at the limit rather than after it** — asserts exactly 100
  bytes were buffered from a 10,000-byte source, so a cap checked after the read
  would fail this test
- **`NetworkMode` defaults to `Deny`** for unset, misspelled, `true`, `1`, and
  even `ALLOW` — a typo must never enable the network

## Security and privacy implications

- The address check is an **allowlist by exclusion of every non-global range**,
  not a blocklist of remembered prefixes. "Not globally routable" ages better
  than a hand-maintained list as IANA adds special-purpose ranges.
- `169.254.0.0/16` is refused as link-local, which covers `169.254.169.254` —
  the cloud metadata endpoint that turns an SSRF into credential theft.
- Carrier-grade NAT (`100.64.0.0/10`) is refused; `Ipv4Addr::is_private` does not
  cover it and it is routinely used for internal infrastructure.
- Cross-host redirects are never followed automatically. The chain is still
  recorded so the analyst sees where the trail led.
- `EgressPolicy::deny_all()` is the starting point, so a forgotten field fails
  closed rather than open.
- Refusals name the specific reason. "We refused because that resolves to your
  router" is actionable in a user-visible network log; "blocked" is not.

## Known limitations

1. **Nothing live routes through this yet**, which is why A-001..A-004 and A-011
   are `partial`, not `active`. The policy is tested; it is not yet protecting
   anything.
2. **No record/replay `HttpCapability`.** A-018 stays `partial`.
3. **No secret scrubbing on log paths.** Nothing logs yet.
4. **No live `Resolver` implementation.** `OfflineResolver` refuses everything,
   so this fails closed rather than silently reaching DNS.
5. **No happy-eyeballs or address-family preference.** `vet_request` returns the
   first address after checking them all; a real client needs a connect strategy.
6. `RateLimiter` is per-instance, not shared across threads. Concurrency belongs
   with the job runner.
7. The `NetworkMode::from_env` test mutates a process-global environment
   variable. Safe today because no other test in the crate reads it, but it is a
   latent source of flakiness if that changes.

## Manual verification

```
KOKIN_NETWORK=deny cargo test -p kokin-net    # 26 tests, no network
```

## Next recommended increment

**Increment 5b — live transport and record/replay.** `HttpCapability` with
`LiveHttp`, `RecordingHttp`, and `ReplayHttp`, where a replay cache miss is a
test failure and never a live call, plus the fixture sanitiser and secret
scrubbing. That is what moves A-001..A-004, A-011, and A-018 to `active`.
