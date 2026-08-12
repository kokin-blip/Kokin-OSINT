# ADR-0007: Confidence is multi-dimensional; there is no composite score

- **Status:** accepted
- **Date:** 2026-08-12
- **Reversibility:** hard once assessments exist in user cases.

## Context

The spec forbids "an unexplained universal confidence percentage". That
prohibition is easy to agree with and easy to violate by accident, because a
single number is what every UI wants and what every stakeholder asks for.

A composite score is not merely uninformative, it is actively misleading. "73%"
formed from a reliable source, a shaky identifier match, and an untested
temporal assumption tells an analyst nothing about which of those to go and
check — and it launders the weakest input behind an average.

## Decision

An `assessment` table holds **one row per dimension** for a subject. There is no
composite column anywhere in the schema, because a column that exists will
eventually be displayed.

Seven dimensions: source reliability, extraction certainty, identifier match,
relationship confidence, temporal consistency, geospatial precision, review
status.

Each references a **versioned scale** loaded from
`docs/data-model/scales/*.yaml`, and each scale file must carry definitions,
calibration examples, threshold rationale, and known failure modes. A scale
without calibration examples is an opinion with a number attached.

`'insufficient_information'` is a first-class value in every scale, **distinct
from a low value**. "We have not established this" and "we have established this
is weak" are different findings, and conflating them is how absence of evidence
becomes evidence of absence.

`contributing_factors_json` drives the UI's "why" panel. A factor that is not
recorded cannot be displayed — which forces the scoring rules to be honest,
because anything that influenced the assessment has to be written down to have
any effect on it.

## Where the thresholds come from

Thresholds are derived empirically and then written into the scale calibration
examples. PoC P7 measures Hamming and cosine separation on 200 images with known
transforms, and *those* numbers become the near-duplicate scale. Inventing a
threshold and calling it "high confidence" is the failure mode this rule exists
to prevent.

## Consequences

- Sorting and filtering are harder without a single number. The UI filters by
  dimension, which is more honest and more useful — "show me claims whose
  identifier match is weak" is the question analysts actually have.
- Users may still want an overall impression. They get the dimensions and a
  review status, not a synthetic average.
- Scales are versioned, so changing one does not silently reinterpret existing
  assessments; old rows keep pointing at the scale version they were made under.
