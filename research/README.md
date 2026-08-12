# Gate 2 — research artifacts

Status: **draft**, to be revised at gate 5.

Three CSVs, all enforced by `scripts/lint_research.py` in CI: every row either
cites a `source_id` that resolves to a row in `source-registry.csv`, or sets
`inferred=true` and says what the inference rests on. There is no third option.

| File | What it holds |
|---|---|
| `source-registry.csv` | 24 sources, each with a URL, an access date, and how it was retrieved |
| `competitor-matrix.csv` | 6 tools against 13 capability columns |
| `bellingcat-coverage.csv` | all 12 Bellingcat toolkit categories mapped to phase1 / phase2 / out_of_scope |

The registry is 24 records rather than the 60–100 the plan projected. That
number was a guess made before the research; the actual constraint is that
every technology and scope decision taken so far has a source, and it does.
It will grow as later increments introduce decisions that need backing —
padding it now with sources nothing depends on would make the lint weaker,
not the research stronger.

## Method note

Where a search summary and a primary source disagreed, the primary source won
and the disagreement is recorded. Two cases:

- **Maltego CE limits.** Third-party blogs state that Community Edition lost its
  entity cap and became commercially usable at version 4.8.0. Maltego's own
  documentation (S-001), fetched directly, still states 10,000 entities per
  graph and 24 results per Transform, and says nothing about commercial use.
  The matrix records the vendor's numbers.
- **Licences generally.** Every open-source licence claim comes from the GitHub
  REST API's `license.spdx_id` field rather than from a README or an article,
  because that field is derived from the repository's actual licence file.

Two competitor rows — Hunchly and IBM i2 — are marked `inferred=true`. Their
pricing and encryption behaviour were not confirmed against vendor
documentation, and third-party comparison pages for both are commercially
motivated. They stay in the matrix because their evidence models are the
relevant prior art, but nothing in the plan should rest on those two rows until
they are sourced.

## The gap

Every tool surveyed collapses evidence into conclusions, in one of two ways.

**Graph-native tools** (Maltego, OSINTBuddy, i2) make the entity graph *the*
data model. An entity and the evidence supporting it are the same object, so
there is nowhere to record that two sources disagree, and no way to revisit a
merge once made. **Capture-native tools** (Hunchly) do the reverse: excellent
preservation of what was seen, and no analytical layer at all — you get a pile
of authenticated pages and a human holding the conclusions in their head.

Aleph is closest to the middle: entities do trace back to source documents. But
it is a server deployment aimed at newsroom-scale document collections, not a
single investigator's encrypted local case, and Aleph Pro is moving on-premise
deployment behind a paid Enterprise tier (S-022).

None of the six record confidence as anything other than a single analyst
grading, and none support merge, split, undo, or competing hypotheses over
entity identity. That gap — L0 provenance and L2 analysis as separate layers,
joined by explicit evidence links, with entity resolution as a reversible
projection — is the product thesis, and it is not currently occupied.

## What the research changed

Two findings altered the plan rather than confirming it.

1. **WhatsMyName is CC BY-SA 4.0** (S-009), not MIT, and not covered by
   sherlock's MIT grant. PoC P8 proposed porting `wmn-data.json` into Kokin's
   declarative connector format — an *adaptation* of a ShareAlike work inside a
   source-available product. P8 is blocked pending **D-012**. This is exactly
   the check the plan called for ("verify the WMN dataset's own license
   separately"), and it did not come back clean.
2. **theHarvester has no licence at all** (S-007) — actively developed, 17k
   stars, and legally all-rights-reserved. It is a good illustration of why
   "popular and on GitHub" is not a licence assessment.

Confirmed as expected: holehe is GPL-3.0 and roughly two years stale (S-008);
`sqlite-vec` is still v0.1.9 (S-015) and `ort` still on release candidate 13
(S-016), so both keep their trait wrappers and documented exit conditions.
