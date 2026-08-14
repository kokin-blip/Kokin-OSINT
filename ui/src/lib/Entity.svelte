<script lang="ts">
  import type { DimensionView, EntityView } from "./bindings";

  interface Props {
    entity: EntityView;
    /** Open an observation in the evidence panel. */
    onevidence?: (kind: string, id: string) => void;
  }

  let { entity, onevidence }: Props = $props();

  /**
   * **There is no total, and there is nowhere to put one.**
   *
   * ADR-0007 has seven dimensions and no composite, and every crate below
   * honours that. This is the layer where a total gets invented anyway, because
   * a total is what sorts a table and colours a badge — and once one exists in a
   * rendering it is indistinguishable from a measurement (A-033).
   *
   * The payload gives this component nothing to add: values are keys on named
   * ordinal scales, and the only number in the block is the scale version an
   * assessment was made under. This file must not reintroduce the arithmetic by
   * mapping keys to numbers, counting "good" values, or ordering entities by
   * how confident they look.
   */
  type Agreement = "unassessed" | "assessed" | "disagreed";

  /**
   * How each state of a dimension reads.
   *
   * The distinction that matters is the first two. `unassessed` means nobody
   * looked. `insufficient_information` means somebody looked and could not
   * tell — a judgement, recorded by a person, with a timestamp. Rendering them
   * alike destroys the whole reason ADR-0007 made the second one first-class.
   */
  const AGREEMENT: Record<Agreement, { label: string; note: string }> = {
    unassessed: {
      label: "Not assessed",
      note: "Nobody has judged this dimension. That is not the same as no concern.",
    },
    assessed: { label: "", note: "" },
    disagreed: {
      label: "Disagreement",
      note: "Rows merged into this entity were judged differently and nobody has reconciled them. Both readings are shown, attributed to the row that carries them.",
    },
  };

  function agreementOf(dimension: DimensionView): Agreement {
    const value = dimension.agreement;
    return value === "unassessed" || value === "disagreed" ? value : "assessed";
  }

  /** True when a person looked and recorded that they could not tell. */
  function isInsufficient(valueKey: string): boolean {
    return valueKey === "insufficient_information";
  }

  function actorKind(actor: string): "person" | "machine" {
    return actor.startsWith("rule:") || actor.startsWith("ai:") ? "machine" : "person";
  }

  /** Contributing factors are stored as a JSON array of strings (D-033). */
  function factors(json: string): string[] {
    try {
      const parsed: unknown = JSON.parse(json);
      return Array.isArray(parsed) ? parsed.filter((f): f is string => typeof f === "string") : [];
    } catch {
      return [];
    }
  }
</script>

<article class="entity" aria-labelledby="entity-title">
  <h3 id="entity-title">
    <span class="type">{entity.type_key}</span>
    {entity.display_name}
  </h3>

  {#if entity.redirected}
    <!-- Identity is a read, not a property of a row (ADR-0008). Silently
         substituting is how an analyst loses track of which of two records a
         name came from. -->
    <p class="redirected" role="status">
      You asked for <code>{entity.requested_id}</code>, which was merged into this
      entity.
    </p>
  {/if}

  {#if entity.notes}
    <p class="notes">{entity.notes}</p>
  {/if}

  {#if entity.merged_from.length > 0}
    <section aria-labelledby="merged-title">
      <h4 id="merged-title">Merged from</h4>
      <ul class="merged">
        {#each entity.merged_from as row (row.entity_id)}
          <li>
            <span class="type">{row.type_key}</span>
            {row.display_name}
          </li>
        {/each}
      </ul>
      <p class="hint">
        A merge is a projection, not a rewrite. These rows still hold their own
        identifiers, evidence and assessments, and everything below spans all of
        them.
      </p>
    </section>
  {/if}

  <section aria-labelledby="confidence-title">
    <h4 id="confidence-title">Confidence</h4>
    <!-- All seven, always, assessed or not (D-031). A sparse list renders as
         absence and absence reads as "no concern", which is the opposite of
         what an unassessed dimension means. -->
    <ul class="dimensions">
      {#each entity.confidence as dimension (dimension.dimension)}
        {@const state = agreementOf(dimension)}
        <li class="dimension {state}">
          <div class="head">
            <span class="name">{dimension.title}</span>
            {#if AGREEMENT[state].label}
              <span class="badge">{AGREEMENT[state].label}</span>
            {/if}
          </div>
          <p class="question">{dimension.question}</p>

          {#if dimension.values.length === 0}
            <p class="note">{AGREEMENT[state].note}</p>
          {:else}
            {#if AGREEMENT[state].note}
              <p class="note">{AGREEMENT[state].note}</p>
            {/if}
            <ul class="values">
              {#each dimension.values as value (value.entity_id + value.value_key)}
                <li class:insufficient={isInsufficient(value.value_key)}>
                  <span class="value">{value.label}</span>
                  {#if isInsufficient(value.value_key)}
                    <span class="badge">Somebody looked</span>
                  {/if}
                  <span class="by">
                    {value.actor}
                    {#if actorKind(value.actor) === "machine"}
                      <span class="badge">Automated</span>
                    {/if}
                    · {value.assessed_utc} · scale v{value.scale_version}
                  </span>
                  {#if entity.merged_from.length > 0}
                    <!-- Which row carries this judgement. After a merge, this is
                         what makes a disagreement attributable rather than
                         merely visible. -->
                    <span class="by">recorded against <code>{value.entity_id}</code></span>
                  {/if}
                  {#if factors(value.contributing_factors_json).length > 0}
                    <ul class="factors">
                      {#each factors(value.contributing_factors_json) as factor (factor)}
                        <li>{factor}</li>
                      {/each}
                    </ul>
                  {:else}
                    <span class="by">No factors recorded.</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
  </section>

  <section aria-labelledby="identifiers-title">
    <h4 id="identifiers-title">Identifiers</h4>
    {#if entity.identifiers.length === 0}
      <p class="note">None recorded.</p>
    {:else}
      <ul class="identifiers">
        {#each entity.identifiers as row (row.identifier_id)}
          <li>
            <span class="namespace">{row.namespace}</span>
            <span class="value">{row.value}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section aria-labelledby="evidence-title">
    <h4 id="evidence-title">Evidence</h4>
    {#if entity.evidence.length === 0}
      <p class="note">None recorded.</p>
    {:else}
      <ul class="evidence">
        {#each entity.evidence as link (link.link_id)}
          <li class="role-{link.role}">
            <!-- Evidence arguing against a conclusion has somewhere to live,
                 and rendering all three roles alike throws that away. -->
            <span class="badge">{link.role}</span>
            {#if link.evidence_kind === "observation" && onevidence}
              <button
                type="button"
                class="link"
                onclick={() => onevidence(link.evidence_kind, link.evidence_id)}
              >
                {link.evidence_kind} {link.evidence_id}
              </button>
            {:else}
              <span>{link.evidence_kind} {link.evidence_id}</span>
            {/if}
            {#if entity.merged_from.length > 0}
              <!-- Two rows of a merged cluster commonly cite the same
                   observation, which reads as a duplicated line unless it says
                   which record each link hangs on. Same reasoning as the
                   confidence block: after a merge, attribution is what stops a
                   projection looking like a single record's own history. -->
              <span class="by">cited by <code>{link.entity_id}</code></span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#if entity.history.length > 0}
    <section aria-labelledby="history-title">
      <h4 id="history-title">Decisions</h4>
      <ol class="history">
        {#each entity.history as decision (decision.decided_utc + decision.action)}
          <li>
            <span class="badge">{decision.action}</span>
            <span class="by">{decision.actor} · {decision.decided_utc}</span>
            <!-- The reason is the product, not a debugging aid (D-033): a merge
                 cannot be reconsidered later without it. -->
            <p class="rationale">{decision.rationale}</p>
          </li>
        {/each}
      </ol>
    </section>
  {/if}
</article>

<style>
  .entity {
    border: 1px solid #262b32;
    border-radius: 6px;
    padding: 1rem 1.25rem;
    margin-top: 1rem;
  }

  h3 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }

  h4 {
    margin: 1.25rem 0 0.5rem;
    font-size: 0.85rem;
    color: #8b95a1;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .type,
  .namespace {
    display: inline-block;
    font-size: 0.7rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    background: #232a33;
    color: #9fb3cc;
    margin-right: 0.5rem;
    vertical-align: 0.15em;
  }

  .badge {
    display: inline-block;
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 0.05rem 0.35rem;
    border-radius: 3px;
    background: #2b323c;
    color: #cbd3dd;
    margin-right: 0.4rem;
    vertical-align: 0.1em;
  }

  .redirected {
    border-left: 3px solid #4a5568;
    background: #1b1f26;
    padding: 0.5rem 0.75rem;
    margin: 0 0 0.75rem;
    font-size: 0.875rem;
  }

  .notes {
    margin: 0 0 0.5rem;
    color: #c3ccd6;
  }

  ul,
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .dimensions {
    display: grid;
    gap: 0.5rem;
  }

  .dimension {
    border: 1px solid #262b32;
    border-left-width: 3px;
    border-radius: 4px;
    padding: 0.5rem 0.75rem;
  }

  /* Not assessed is a state, not a warning, and not a clean bill of health.
     Muted rather than coloured either way. */
  .unassessed {
    border-left-color: #39424f;
    background: #16191e;
  }

  .assessed {
    border-left-color: #3f6b4a;
  }

  /* A disagreement is the one state here that wants attention: two readings of
     the same question coexist and nobody has settled it. */
  .disagreed {
    border-left-color: #6b5a3a;
    background: #1e1a13;
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .name {
    font-weight: 600;
  }

  .question {
    margin: 0.2rem 0 0;
    color: #8b95a1;
    font-size: 0.8rem;
  }

  .note,
  .hint {
    margin: 0.35rem 0 0;
    color: #8b95a1;
    font-size: 0.8rem;
  }

  .values {
    margin-top: 0.5rem;
    display: grid;
    gap: 0.5rem;
  }

  .values > li {
    padding-left: 0.75rem;
    border-left: 2px solid #2b323c;
  }

  .value {
    font-weight: 600;
    margin-right: 0.4rem;
  }

  /* "Somebody looked and could not tell" is a recorded judgement, so it reads
     as one rather than as a blank. */
  .insufficient {
    border-left-color: #5a6b8a;
  }

  .by {
    display: block;
    color: #8b95a1;
    font-size: 0.75rem;
  }

  .factors {
    margin: 0.25rem 0 0;
    padding-left: 1rem;
    list-style: disc;
    color: #c3ccd6;
    font-size: 0.8rem;
  }

  .identifiers > li,
  .evidence > li,
  .merged > li,
  .history > li {
    padding: 0.25rem 0;
    border-bottom: 1px solid #1c2027;
  }

  .role-contradicts .badge {
    background: #4a3a2b;
    color: #f0d0b0;
  }

  .rationale {
    margin: 0.2rem 0 0;
    color: #c3ccd6;
    font-size: 0.875rem;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: #8fb8e8;
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }

  code {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8rem;
  }
</style>
