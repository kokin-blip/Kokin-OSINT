# Increment 5b — URL identity and the replay harness

Date: 2026-08-12 · Branch: `feat/phase0-foundations`

Still **no live transport**. This increment adds the two pieces that let the
ingest pipeline be built and tested entirely from fixtures, which is what
reference-workflow step 11 ("repeat offline using recorded fixtures") requires.

`LiveHttp` and `RecordingHttp` remain unimplemented — deliberately, since there
is nothing to record until there is a connector to record it for.

## Files changed

| File | What it does |
|---|---|
| `crates/kokin-net/src/url_norm.rs` | `raw_locator` and `canonical_locator`; tracking-parameter stripping (A-013). |
| `crates/kokin-net/src/http.rs` | `HttpRequest`/`HttpResponse`, `HttpCapability`, `Fixture`, `ReplayHttp`. |
| `crates/kokin-net/src/lib.rs` | Module wiring and re-exports. |
| `docs/dependencies.csv` | `url` recorded. |

## The two ideas

**A URL answers two different questions, so it gets two forms.** *What did we
fetch?* is provenance and must be verbatim — editing it would falsify the
record. *Is this the same page?* is identity, and must ignore `utm_source`,
`fbclid` and friends, or one article becomes five entities depending on how
someone arrived at it. `normalise` returns both.

**The replay key and the evidence identity come from the same function.** If
replay had its own normaliser, a fixture could match a request that the evidence
model considers a different page — the offline suite would still pass while
quietly testing something other than what it claims. Sharing the normaliser makes
that drift impossible rather than unlikely.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace --all-targets
                                        94 passed, 0 failed
                                        (48 net + 23 store + 14 blob + 9 keys)
cargo clippy --workspace --all-targets  0 warnings (-D warnings)
cargo deny check bans licenses sources advisories
                                        advisories ok, bans ok, licenses ok, sources ok
```

Twenty-two new tests. The ones carrying weight:

- **tracking parameters are stripped from identity but kept verbatim** — asserts
  `raw_locator` is byte-for-byte the input, which is the provenance requirement
- **the same page reached two ways has one identity**
- **a meaningful parameter is never stripped** — the failure in the other
  direction: `?q=utm_source` is a search for the text "utm_source" and must
  survive, or over-eager stripping silently changes which page evidence points at
- **a missing fixture is an error and not a live request** — `ReplayHttp` holds
  no client, so there is nothing to fall back *to*
- **a replayed response is marked as coming from a fixture** — set on read rather
  than trusted from the file, so a capture can never claim to be live when it
  was replayed
- **volatile headers do not change the key** — `User-Agent` and `Cookie` are
  excluded, because a key that varies per machine gets "fixed" by loosening it
  until it matches anything
- **tracking parameters do not change the fixture key** — the shared-normaliser
  property, asserted directly

## A bug the tests caught

Port normalisation was wrong. I stripped the port when
`parsed.port() == parsed.port_or_known_default()`, but that method returns the
*explicit* port when one is present, so the comparison was always true and
`https://example.com:8443/p` silently became `https://example.com/p` — two
different services collapsed into one identity.

The `url` crate already drops a default port per the WHATWG spec, so the
correct fix was to delete the code entirely. The test that caught it was the one
asserting a **non**-default port survives; the default-port cases all passed.

## Security and privacy implications

- URL parsing is delegated to `url` rather than hand-rolled. Allowlist bypasses
  via userinfo tricks, mixed encodings, and IDN homographs are a well-known
  class, and a hand-written parser here would be a defect generator.
- `file:///etc/passwd` is rejected at normalisation as having no host, before the
  scheme guard sees it. Two independent refusals, which is the intent.
- Tracking parameters are stripped from identity but preserved in `raw_locator`,
  so referral context is neither leaked into entity identity nor destroyed
  (A-013).
- A replayed response cannot masquerade as a live one.

## Known limitations

1. **No `LiveHttp` or `RecordingHttp`.** A-001..A-004 and A-011 stay `partial`.
2. **No fixture sanitiser yet.** The CI hygiene job greps `tests/fixtures/` for
   secret patterns, but nothing strips `Set-Cookie` at record time — and there is
   no recorder yet to strip it in. Both land together, or the sanitiser would be
   untestable.
3. **The tracking-parameter list is hand-maintained.** Prefix families cover the
   open-ended cases (`utm_`, `pk_`, `mtm_`, `hsa_`), but new one-off parameters
   will appear. This is a maintenance burden, not a design flaw, and the failure
   mode is benign: an unstripped parameter fragments identity, it does not
   corrupt evidence.
4. **Bodies are `String`, not bytes.** Fine for HTML and JSON fixtures; binary
   responses need the blob store, and wiring that in belongs with ingest.
5. `ReplayHttp` does not verify that the stored fixture's request matches the one
   being served — it trusts the key. A hash collision is not a real concern with
   BLAKE3, but a hand-edited fixture could lie about its own request.

## Manual verification

```
KOKIN_NETWORK=deny cargo test -p kokin-net    # 48 tests, no network
```

## Next recommended increment

**Increment 6 — the ingest pipeline.** `URL → capture → blob → artifact`, with
`transform_run` and `derivation` rows written, driven entirely by `ReplayHttp`.
That is reference-workflow steps 2 and 3, and it is now unblocked: the store, the
blob store, and the replay harness all exist.
