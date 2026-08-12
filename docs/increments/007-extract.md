# Increment 7 — extraction

Date: 2026-08-12 · Branch: `feat/phase0-foundations`

Reference-workflow step 4: an artifact becomes observations, each pointing at
exactly where in the bytes it came from. This is the first crate that reads
evidence back out rather than writing it in, and the first one hostile content
reaches directly.

Landed in two parts. **7a** is migration 3, which adds `observation` and
`observation_supersession`; **7b** is `kokin-extract` and the hostile corpus.

## Files changed

| File | What it does |
|---|---|
| `crates/kokin-store/src/migrations.rs` | Migration 3: `observation` with `locator_json`, `observation_supersession`, and their append-only triggers. |
| `crates/kokin-store/src/golden_schema.txt` | Re-blessed. |
| `crates/kokin-extract/src/html.rs` | The parsing core: bytes in, observations out. No database, no filesystem. |
| `crates/kokin-extract/src/lib.rs` | The row-writing half: run lifecycle, lineage, supersession, audit, panic boundary. |
| `crates/kokin-extract/tests/hostile_corpus.rs` | 17 tests against documents that are trying something. |
| `tests/fixtures/hostile/` | Four documents plus a README mapping each to its attack-catalog row. |
| `docs/dependencies.csv` | `lol_html`, `html-escape`, and the two transitive crates they bring. |
| `docs/threat-model/attack-catalog.csv` | A-012 `planned` → `partial`. |

## The five ideas

**Streaming, not a DOM.** `lol_html` is a tokeniser with CSS-selector handlers,
so memory is bounded by the largest single token rather than by the document. A
DOM parser would turn the required 500 MB document into several gigabytes of
nodes. This is the difference between "we refuse documents over some size" and
"we can read them", and evidence refused for being large is evidence an
investigation does not get.

**A byte range is the locator.** ADR-0006 wants every observation to point at
where it came from; a range into an immutable, content-addressed artifact is the
strongest available form, because verifying it needs no interpretation. A CSS
path would depend on the reader rebuilding the same tree we did. The test checks
one the way a human would — by slicing the document with it.

**The parser returns syntax; the extractor stores meaning.** See the bug below.

**Re-extraction supersedes, never replaces.** An artifact is immutable, so
running a newer extractor over it is the normal case: the bytes did not change,
our reading did. Old observations are never edited or deleted — they gain a
supersession row pointing at whatever replaced them, or at *nothing* when the
newer reading withdraws the claim. "This is no longer observed" and "this was
replaced by that" are different facts and the schema can say both.

**Refusing beats truncating.** Over the observation cap, extraction fails and
writes nothing. A silently truncated reading is indistinguishable from a
complete one, and an analyst would have no way to learn the page held more than
they were shown.

## Tests executed and results

```
KOKIN_NETWORK=deny cargo test --workspace --all-targets
                                        136 passed, 0 failed
                                        (48 net + 26 store + 17 extract-hostile
                                         + 14 blob + 12 ingest + 10 extract
                                         + 9 keys)
cargo clippy --workspace --all-targets -- -D warnings
                                        0 warnings
cargo fmt --all --check                 clean
cargo deny check                        advisories, bans, licenses, sources ok
```

Twenty-seven new tests. The ten in `src/lib.rs` cover the database half —
lineage, supersession, withdrawal, media-type refusal, missing artifact,
shredded blob, append-only triggers, the audit chain, rollback on refusal, and
the panic boundary. The seventeen in `tests/hostile_corpus.rs` are the corpus
the plan asks for:

- **markup inside script and style is not extracted** (A-009) — a `<script>`
  containing `<a href="…attacker…">` and a `<meta>` yields nothing. Leaking it
  would hand an investigator links and metadata the page never showed anyone.
  Covers `<style>`, `<noscript>`, comments and `<textarea>` too.
- **injection strings are stored verbatim and inert** (A-005) — "ignore previous
  instructions…" is recorded as a value, not obeyed and not rewritten. It is
  evidence that the page said this.
- **csv formula payloads survive extraction unescaped** (A-008) — `=`, `+`, `-`,
  `@`, leading TAB and leading CR all reach storage intact, which is what makes
  the export escape the single place that has to get it right.
- **malformed markup does not panic and still reads what it can** (A-012) —
  unquoted attributes, bad nesting, stray brackets, markup after `</html>`.
- **an unterminated title swallows the rest of the document** — see limitations.
- **a 500 mb document is read in bounded memory** (A-011) — generated, not
  committed. Completes in about a second.
- **an unterminated tag hits the memory ceiling rather than growing**.
- **a document over the observation cap is refused not truncated**.
- **streaming in small chunks gives identical results** — 7-byte chunks produce
  byte-identical output to one write, including every offset.
- **a locator points at the bytes it claims to**.
- **invalid utf8 bytes do not panic**.
- plus mailto parsing, relative links, oversized values, and empty meta tags.

## A bug the corpus caught

`lol_html` is a *rewriter*, so `get_attribute` and text chunks return the
document's source bytes rather than the value they denote. `content="&#13;=1+1"`
arrives as the literal characters `&#13;=1+1`.

Storing that would have been storing syntax instead of content, and the
consequence is not cosmetic. The CSV-formula escape at export (A-008) decides
what to neutralise by looking at what a value *starts with*. It would have seen
`&`, concluded the value was harmless, and passed a formula straight into a
spreadsheet — a control defeated not by being wrong but by being handed
pre-mangled input. The same applies to any future check that reasons about a
leading character.

Entity decoding is now applied to attribute values and to title text. Decoding
is *reading the document as specified*, not sanitising it: the artifact is
immutable and every observation carries a byte-range locator, so the encoded
original is always one lookup away.

The decode happens after a text node is fully accumulated, never per chunk,
because an entity can straddle a chunk boundary — `&am` then `p;`. Decoding
chunk by chunk would make the stored value depend on how the blob happened to be
split, which the 7-byte-chunk test now rules out.

A/B-verified: dropping the decode fails exactly
*csv_formula_payloads_survive_extraction_unescaped* and
*injection_strings_are_stored_verbatim_and_inert*, and nothing else.

## A default that was not safe

`MemorySettings::max_allowed_memory_usage` defaults to `usize::MAX`. That is a
reasonable default for a CDN rewriting its customers' own pages, and the wrong
one here: a single unterminated tag would buffer the entire document, which is
precisely the input a hostile page controls.

It is now set to 4 MiB. A/B-verified: leaving the default in place fails exactly
*an_unterminated_tag_hits_the_memory_ceiling_rather_than_growing* — a 200 KB
unterminated tag is buffered without complaint — and nothing else.

## Security and privacy implications

- **Nothing here opens a socket, and there is no handle that could.**
  `extract_artifact` takes a connection, a blob store and a key. Unlike ingest,
  it does not take an `HttpCapability` at all.
- **A panic becomes an error code, not a lost case** (A-012). The parse runs
  inside `catch_unwind`, and a panic is recorded as `extract.panicked` —
  deliberately *not* folded into `extract.malformed`, because a malformed
  document is a fact about the evidence while a panic is a bug in us. Filing the
  second as the first would quietly convert our defects into observations about
  someone's website. Tested by panicking on purpose.
- **Hostile values are stored, not sanitised.** Sanitising on write destroys
  evidence and protects nobody, since every consumer needs its own escaping
  anyway. Escaping is the renderer's and the exporter's job, at the point of
  use. The migration says so where the table is defined.
- **The media type is checked, not assumed.** Running an HTML parser over a PDF
  produces confident nonsense, and the artifact already records what it is.
- **Shredded is distinguished from missing.** A blob whose key has been
  destroyed reports `blob.shredded`; one that was never there reports
  `blob.missing`. "We destroyed it deliberately" and "we never had it" are
  different answers to an analyst's question.
- **A failed extraction writes nothing** — one transaction, so there is no
  partial reading that looks like a complete one.
- **`javascript:` and `data:` links are recorded but produce no domain**, so an
  unfetchable target never appears in front of an analyst looking like a host.

## Known limitations

1. **No free-text scanning.** Observations come only from structured positions:
   `<title>`, `<meta>`, and `a[href]` (with `mailto:` → `email.address` and the
   host → `domain.mention`). An email address written in body prose is not
   found. Every observation therefore has an exact locator, which is the
   trade being made; a text scanner is its own increment and needs its own
   hostile tests.
2. **Relative links produce no domain.** Resolving them needs the artifact's own
   URL, which is provenance the parser does not have. A guessed base would
   invent domains the page never named. The link itself is still recorded.
3. **An unterminated `<title>` swallows the document.** This is correct
   tokenising — everything after it is RCDATA — so a page can be crafted to
   yield almost nothing while parsing cleanly. The extractor does not "fix" it:
   a reader who sees one observation against a large artifact is being told the
   truth. Asserted by a test so that the day the reading changes, something
   says so.
4. **Only the first `<title>` is recorded**, and only UTF-8 is decoded. A
   `<meta charset>` partway through a document is deliberately ignored, because
   honouring it would mean locator offsets referred to a re-decoded document
   rather than the artifact's real bytes, making every locator unverifiable.
   Non-UTF-8 evidence is not yet supported.
5. **Supersession matches on (kind, locator).** Right for the normal case — the
   same claim about the same bytes of the same immutable artifact — but an
   extractor upgrade that shifts offsets will supersede everything with
   nothing, reading as a wholesale withdrawal rather than a revision.
6. **A meta key longer than 128 bytes is skipped**, counted only in the same
   bucket as oversized values, so the two are indistinguishable in the audit
   payload.
7. **`skipped_oversize` says how many, never which.** The count reaches the
   caller and the log; the values do not, by design, but there is no way to ask
   what was dropped short of re-running with a higher cap.
8. **One artifact per call, one transaction.** No batching, and no way to
   extract a whole case.
9. **`observation_supersession` has no reader yet.** The rows are written and
   the triggers protect them, but nothing surfaces "this observation was
   superseded" — and ADR-0006's `needs_review` flag on dependent claims cannot
   exist until L2 does.

## Manual verification

```
KOKIN_NETWORK=deny cargo test -p kokin-extract     # 27 tests, no network
KOKIN_BLESS_SCHEMA=1 cargo test -p kokin-store schema_matches
```

Both A/B controls, to confirm the tests fail for the reason claimed:

- drop the `decode(&c)` from meta `content` — the CSV-formula and
  injection-string tests fail, and nothing else does.
- replace `MemorySettings::new().with_max_allowed_memory_usage(…)` with
  `MemorySettings::new()` — the unterminated-tag test fails, and nothing else
  does.

## Next recommended increment

**Increment 8 — entities, identifiers, relationships, `evidence_link`,
`assessment`, and the seven scales from `docs/data-model/scales/*.yaml`.** That
is L2, the layer this increment's observations feed, and it is where ADR-0006's
"nothing in L2 exists without an `evidence_link`" constraint finally has both
sides to connect. It is reference-workflow step 5.

It also unblocks two things left open here: a reader for supersession, and the
`needs_review` flag on claims resting on superseded observations.
