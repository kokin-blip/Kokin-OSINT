import { describe, expect, it, afterEach, vi } from "vitest";
import { mount, unmount, flushSync } from "svelte";
import Entity from "./Entity.svelte";
import type {
  DimensionValueView,
  DimensionView,
  EntityView,
} from "./bindings";

/**
 * Seven dimensions, no total, and the difference between two kinds of nothing.
 *
 * ADR-0007 has seven dimensions and no composite. Every crate below this one
 * honours that; the view layer is where a total gets invented anyway, because a
 * total is what sorts a table and colours a badge — and once one exists in a
 * rendering it is indistinguishable from a measurement (A-033).
 *
 * The other rule here is D-031's: `unassessed` means nobody looked,
 * `insufficient_information` means somebody looked and could not tell. The
 * second is a judgement with an author and a timestamp. Rendering them alike
 * destroys the whole reason ADR-0007 made it first-class.
 *
 * Neither rule is assertable from Rust — the payload can carry seven dimensions
 * and the screen still show two — which is the argument D-039 records for
 * testing components at all.
 */

/** The seven the case ships with. */
const SCALES: [string, string][] = [
  ["source_reliability", "Source reliability"],
  ["extraction_certainty", "Extraction certainty"],
  ["identifier_match", "Identifier match"],
  ["relationship_confidence", "Relationship confidence"],
  ["temporal_consistency", "Temporal consistency"],
  ["geospatial_precision", "Geospatial precision"],
  ["review_status", "Review status"],
];

function value(over: Partial<DimensionValueView> = {}): DimensionValueView {
  return {
    entity_id: "ent-canonical",
    value_key: "usually_reliable",
    label: "Usually reliable",
    scale_version: 1,
    actor: "user:local",
    assessed_utc: "2026-08-14T09:00:00Z",
    contributing_factors_json: "[]",
    ...over,
  };
}

/** All seven, assessed only where `assessed` says so. */
function confidence(
  assessed: Record<string, DimensionValueView[]> = {},
): DimensionView[] {
  return SCALES.map(([dimension, title]) => {
    const values = assessed[dimension] ?? [];
    let agreement = "unassessed";
    if (values.length === 1) agreement = "assessed";
    if (values.length > 1) {
      agreement = values.every((v) => v.value_key === values[0].value_key)
        ? "assessed"
        : "disagreed";
    }
    return {
      dimension,
      title,
      question: `The question ${dimension} asks.`,
      agreement,
      values,
    };
  });
}

function entity(over: Partial<EntityView> = {}): EntityView {
  return {
    requested_id: "ent-canonical",
    entity_id: "ent-canonical",
    redirected: false,
    type_key: "person",
    display_name: "A. Mercer",
    notes: "",
    merged_from: [],
    identifiers: [],
    evidence: [],
    confidence: confidence(),
    history: [],
    ...over,
  };
}

let host: HTMLElement | null = null;
let component: Record<string, unknown> | null = null;

function render(props: {
  entity: EntityView;
  onevidence?: (kind: string, id: string) => void;
}): HTMLElement {
  host = document.createElement("div");
  document.body.appendChild(host);
  component = mount(Entity, { target: host, props });
  flushSync();
  return host;
}

afterEach(() => {
  if (component) void unmount(component);
  host?.remove();
  component = null;
  host = null;
});

/**
 * The rendered text as a person reads it.
 *
 * A Svelte template wraps prose across source lines, so `textContent` carries
 * the newlines and indentation of the *file* rather than of the page. Asserting
 * against raw `textContent` makes a phrase assertion fail when someone reflows a
 * paragraph, which is a test about formatting pretending to be a test about
 * meaning.
 */
function text(root: HTMLElement): string {
  return (root.textContent ?? "").replace(/\s+/g, " ").trim();
}

describe("the confidence block", () => {
  it("renders all seven dimensions when only one is assessed", () => {
    const root = render({
      entity: entity({
        confidence: confidence({ source_reliability: [value()] }),
      }),
    });

    const rendered = root.querySelectorAll(".dimension");
    expect(rendered.length).toBe(7);
    // Named, not counted: a filter that dropped the unassessed ones would still
    // render seven of something if the fixture changed.
    for (const [, title] of SCALES) {
      expect(text(root)).toContain(title);
    }
    expect(root.querySelectorAll(".dimension.unassessed").length).toBe(6);
  });

  it("says nobody looked, rather than leaving a blank", () => {
    const root = render({ entity: entity() });
    const first = root.querySelector(".dimension");
    expect(first?.classList.contains("unassessed")).toBe(true);
    expect(text(root)).toContain("Not assessed");
    expect(text(root)).toContain("not the same as no concern");
  });

  it("distinguishes nobody looked from somebody looked and could not tell", () => {
    const root = render({
      entity: entity({
        confidence: confidence({
          source_reliability: [
            value({
              value_key: "insufficient_information",
              label: "Insufficient information",
            }),
          ],
        }),
      }),
    });

    // The assessed one is not in the unassessed state, and carries the author
    // and timestamp that make it a judgement rather than a gap.
    const assessed = root.querySelectorAll(".dimension.assessed");
    expect(assessed.length).toBe(1);
    expect(text(root)).toContain("Insufficient information");
    expect(text(root)).toContain("Somebody looked");
    expect(text(root)).toContain("user:local");
    expect(text(root)).toContain("2026-08-14T09:00:00Z");

    // And the six nobody touched still read as untouched.
    expect(root.querySelectorAll(".dimension.unassessed").length).toBe(6);
  });

  it("shows a disagreement attributed to the rows that carry it", () => {
    const root = render({
      entity: entity({
        merged_from: [
          { entity_id: "ent-absorbed", display_name: "Mercer, Alex", type_key: "person" },
        ],
        confidence: confidence({
          source_reliability: [
            value({ entity_id: "ent-canonical", value_key: "usually_reliable", label: "Usually reliable" }),
            value({ entity_id: "ent-absorbed", value_key: "mixed", label: "Mixed" }),
          ],
        }),
      }),
    });

    const disagreed = root.querySelectorAll(".dimension.disagreed");
    expect(disagreed.length).toBe(1);

    // Both readings survive. Picking a winner is a composite score wearing a
    // different hat.
    expect(text(root)).toContain("Usually reliable");
    expect(text(root)).toContain("Mixed");

    // And each is attributable to its row, which is what makes the
    // disagreement actionable rather than merely visible.
    expect(text(root)).toContain("ent-canonical");
    expect(text(root)).toContain("ent-absorbed");
  });

  it("invents no total, no score and no percentage", () => {
    const root = render({
      entity: entity({
        confidence: confidence({
          source_reliability: [value()],
          review_status: [value({ value_key: "needs_review", label: "Needs review" })],
        }),
      }),
    });

    const rendered = text(root);
    // No percentage, no "n/7", no "score", no star rating, no letter grade
    // presented as an overall verdict.
    expect(rendered).not.toMatch(/\d+\s*%/);
    expect(rendered).not.toMatch(/\b\d+\s*(of|\/)\s*7\b/);
    expect(rendered.toLowerCase()).not.toContain("overall");
    expect(rendered.toLowerCase()).not.toContain("score");
    expect(rendered.toLowerCase()).not.toContain("total confidence");

    // The only digits allowed in the block are the scale version and the
    // timestamps that say when a judgement was made — never a computed figure.
    const versions = [...rendered.matchAll(/scale v(\d+)/g)].map((m) => m[1]);
    expect(versions.length).toBeGreaterThan(0);
    expect(versions.every((v) => v === "1")).toBe(true);
  });

  it("marks an automated assessment as automated", () => {
    const root = render({
      entity: entity({
        confidence: confidence({
          identifier_match: [value({ actor: "rule:shared_identifier" })],
        }),
      }),
    });
    // An automated judgement shown as a person's is the provenance failure
    // ADR-0008 names.
    expect(text(root)).toContain("rule:shared_identifier");
    expect(text(root)).toContain("Automated");
  });

  it("shows the factors a judgement was based on", () => {
    const root = render({
      entity: entity({
        confidence: confidence({
          source_reliability: [
            value({
              contributing_factors_json: JSON.stringify([
                "the filing names the same registration number",
                "the press page has been stable for two years",
              ]),
            }),
          ],
        }),
      }),
    });
    expect(text(root)).toContain("same registration number");
    expect(text(root)).toContain("stable for two years");
  });

  it("survives a factors field that is not a JSON array of strings", () => {
    const root = render({
      entity: entity({
        confidence: confidence({
          source_reliability: [value({ contributing_factors_json: "not json at all" })],
        }),
      }),
    });
    // Degrades to "none recorded" rather than throwing and blanking the panel.
    expect(text(root)).toContain("No factors recorded.");
    expect(root.querySelectorAll(".dimension").length).toBe(7);
  });
});

describe("the entity panel", () => {
  it("says when it followed a merge rather than substituting silently", () => {
    const root = render({
      entity: entity({ requested_id: "ent-absorbed", redirected: true }),
    });
    expect(text(root)).toContain("ent-absorbed");
    expect(text(root)).toContain("merged into this entity");
  });

  it("distinguishes evidence that argues against the conclusion", () => {
    const root = render({
      entity: entity({
        evidence: [
          {
            link_id: "l1",
            entity_id: "ent-canonical",
            evidence_kind: "observation",
            evidence_id: "obs-1",
            role: "supports",
          },
          {
            link_id: "l2",
            entity_id: "ent-canonical",
            evidence_kind: "observation",
            evidence_id: "obs-2",
            role: "contradicts",
          },
        ],
      }),
    });
    expect(root.querySelectorAll(".role-supports").length).toBe(1);
    expect(root.querySelectorAll(".role-contradicts").length).toBe(1);
    expect(text(root)).toContain("contradicts");
  });

  it("opens an observation from its evidence link", () => {
    const onevidence = vi.fn();
    const root = render({
      entity: entity({
        evidence: [
          {
            link_id: "l1",
            entity_id: "ent-canonical",
            evidence_kind: "observation",
            evidence_id: "obs-1",
            role: "supports",
          },
        ],
      }),
      onevidence,
    });

    root.querySelector<HTMLButtonElement>(".evidence button")?.click();
    flushSync();
    expect(onevidence).toHaveBeenCalledWith("observation", "obs-1");
  });

  it("shows why a merge was made", () => {
    const root = render({
      entity: entity({
        history: [
          {
            action: "merge",
            actor: "user:local",
            rationale: "same email in the filings",
            decided_utc: "2026-08-14T09:05:00Z",
          },
        ],
      }),
    });
    // Without the reason the merge cannot be reconsidered, which is the only
    // thing that makes it reversible in practice (D-033).
    expect(text(root)).toContain("same email in the filings");
    expect(text(root)).toContain("user:local");
  });
});
