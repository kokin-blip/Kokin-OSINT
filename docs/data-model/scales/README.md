# Confidence scales

The seven dimensions of ADR-0007. There is **no composite score** anywhere —
not in these files, not in the schema, not in the UI. A single number formed
from a reliable source, a shaky identifier match and an untested temporal
assumption tells an analyst nothing about which one to go and check, and it
launders the weakest input behind an average.

| File | Dimension | Answers |
|---|---|---|
| `source-reliability.yaml` | Source reliability | What track record does the origin have? |
| `extraction-certainty.yaml` | Extraction certainty | Did we read the bytes correctly? |
| `identifier-match.yaml` | Identifier match | Is this the same actor? |
| `relationship-confidence.yaml` | Relationship confidence | What supports this edge? |
| `temporal-consistency.yaml` | Temporal consistency | Do the times fit, and how well are they known? |
| `geospatial-precision.yaml` | Geospatial precision | What area does this location actually describe? |
| `review-status.yaml` | Review status | Has a human decided, and what? |

## File format

Every file carries `id`, `version`, `title`, `adr`, `status`, `question`,
`values`, `threshold_rationale`, and `known_failure_modes`. ADR-0007 requires
definitions, calibration examples, threshold rationale and failure modes,
because *a scale without calibration examples is an opinion with a number
attached*.

Each entry in `values` has a `key`, a `label`, a `definition`, and
`calibration_examples`. `machine_assignable` marks the values an automated pass
is permitted to set; where it is absent, assume false.

## Two rules that are easy to break

**`insufficient_information` is in every scale and is never the bottom value.**
"We have not established this" and "we have established this is weak" are
different findings. Conflating them is how absence of evidence becomes evidence
of absence, so it is listed first, away from the ordered values.

**Thresholds are measured, then written down here — never invented.** Where a
number would be needed and does not exist, the file says so and the scale stays
`partial`:

- `identifier-match.yaml` is `partial`. Its similarity tiers wait on PoC P8,
  which measures the false-positive rate of the username sweep. Until then the
  machine may assign only the two ends, and every value asserting a real link
  requires a human.
- `geospatial-precision.yaml` deliberately carries **no** metre figures. Tiers
  name what resolved the location; any uncertainty radius shown must come from
  the geocoder or the device as data, because "locality means 5 km" is wrong
  for a village and wrong for Chongqing by two orders of magnitude.

## Versioning

`version` is per file. Assessments record the scale version they were made
under, so changing a scale never silently reinterprets existing rows
(ADR-0007). Editing a shipped scale in place is the same class of mistake as
editing a shipped migration: add a version, do not rewrite history.
