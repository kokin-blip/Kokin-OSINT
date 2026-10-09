<script lang="ts">
  import SourceCitation from "./SourceCitation.svelte";
  import type { Snippet } from "svelte";
  import type { DimensionView, EntityView, ObservationView, PageView, SubjectView } from "../../bindings";

  interface Props {
    context?: Snippet;
    activity?: Snippet;
    source?: ObservationView | null;
    sourceError?: string;
    relationships?: PageView<SubjectView> | null;
    onopen?: (kind: string, id: string) => void;
    onpage?: (delta: number) => void;
    entity: EntityView;
    /** Open an observation in the evidence panel. */
    onevidence?: (kind: string, id: string) => void;
  }

  let { context, activity, entity, onevidence, source = null, sourceError = "", relationships = null, onopen, onpage }: Props = $props();

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
<div class="dossier-main">
  <header class="dossier-heading"><h3 id="entity-title">
    <span class="type">{entity.type_key}</span>
    {entity.display_name}
  </h3><span class="identity">Canonical entity</span></header>

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


  <SourceCitation {source} {sourceError} {onopen}/>

<div class="lower-grid">
  <section class="identifier-panel" aria-labelledby="identifiers-title">
    <h4 id="identifiers-title">Identifiers</h4>
    {#if entity.identifiers.length === 0}
      <p class="note">None recorded.</p>
    {:else}
      <ul class="identifiers">
        {#each entity.identifiers as row (row.identifier_id)}
          <li>
            <span class="namespace">{row.namespace}</span>
            {#if onopen}<button class="link" type="button" onclick={() => onopen("identifier", row.identifier_id)}>{row.value}</button>{:else}<span class="value">{row.value}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="evidence-panel" aria-labelledby="evidence-title">
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
            {#if onevidence}
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


  {#if relationships}
    <section class="relationships-panel" aria-labelledby="relationships-title">
      <h4 id="relationships-title">Relationships ({relationships.total})</h4>
      {#each relationships.rows as edge (edge.subject_id)}
        <button class="relationship-row" type="button" onclick={() => onopen?.('relationship', edge.subject_id)}>{edge.title}<span class="by">Open relationship and its own evidence</span></button>
      {:else}<p class="note">None recorded.</p>{/each}
      {#if relationships.total > relationships.limit}<div class="pages"><button type="button" disabled={relationships.offset === 0} onclick={() => onpage?.(-50)}>Previous relationships</button><span>{relationships.offset+1}–{Math.min(relationships.offset+relationships.limit,relationships.total)} of {relationships.total}</span><button type="button" disabled={relationships.offset+relationships.limit>=relationships.total} onclick={() => onpage?.(50)}>Next relationships</button></div>{/if}
    </section>
  {/if}

  {#if activity}<div class="activity-panel">{@render activity()}</div>{/if}
  {#if entity.history.length > 0}
    <section class="history-panel" aria-labelledby="history-title">
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
</div>
</div>
<aside class="dossier-inspector" aria-label="Entity assessment">{#if context}{@render context()}{/if}
  <section class="confidence-panel" aria-labelledby="confidence-title">
    <h4 id="confidence-title">Confidence</h4>
    <!-- All seven, always, assessed or not (D-031). A sparse list renders as
         absence and absence reads as "no concern", which is the opposite of
         what an unassessed dimension means. -->
    <ul class="dimensions">
      {#each entity.confidence as dimension (dimension.dimension)}
        {@const state = agreementOf(dimension)}
        <li class="dimension {state}">
          <details open={state === "disagreed"}><summary class="head">
            <span class="name">{dimension.title}</span>
            {#if AGREEMENT[state].label}
              <span class="badge">{AGREEMENT[state].label}</span>
            {/if}
            {#if !AGREEMENT[state].label && dimension.values.length === 1}<span class="badge">{dimension.values[0].label}</span>{/if}
          </summary>
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
          </details>
        </li>
      {/each}
    </ul>
  </section>

</aside>
</article>

<style>
  .entity {
    border: 1px solid var(--border);
    border: none;
    padding: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(260px, 1fr);
    gap: var(--space-5);
  }

  h3 {
    margin: 0 0 0.5rem;
    font-size: var(--text-title);
  }

  h4 {
    margin: 0 0 var(--space-4);
    font-size: 16px;
    color: var(--foreground);
    font-weight: 600;
    letter-spacing: 0;
  }

  .type,
  .namespace {
    display: inline-block;
    font-size: 0.7rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    background: var(--surface-raised);
    color: var(--foreground-secondary);
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
    background: var(--border);
    color: var(--foreground-secondary);
    margin-right: 0.4rem;
    vertical-align: 0.1em;
  }

  .redirected {
    border-left: 3px solid var(--border-strong);
    background: var(--surface-raised);
    padding: 0.5rem 0.75rem;
    margin: 0 0 0.75rem;
    font-size: 0.875rem;
  }

  .notes {
    margin: 0 0 0.5rem;
    color: var(--foreground-secondary);
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
    border: 1px solid var(--border);
    border-left-width: 3px;
    border-radius: 4px;
    padding: 0.5rem 0.75rem;
  }

  /* Not assessed is a state, not a warning, and not a clean bill of health.
     Muted rather than coloured either way. */
  .unassessed {
    border-left-color: var(--border-strong);
    background: var(--surface-panel);
  }

  .assessed {
    border-left-color: var(--success);
  }

  /* A disagreement is the one state here that wants attention: two readings of
     the same question coexist and nobody has settled it. */
  .disagreed {
    border-left-color: var(--warning);
    background: var(--warning-surface);
  }

  summary { cursor: pointer; list-style: none; }
  summary::after { content: ""; width: 6px; height: 6px; border-right: 1px solid var(--foreground-muted); border-bottom: 1px solid var(--foreground-muted); transform: rotate(45deg); margin-left: auto; }
  details[open] summary::after { transform: rotate(225deg); }
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
    color: var(--foreground-muted);
    font-size: 0.8rem;
  }

  .note,
  .hint {
    margin: 0.35rem 0 0;
    color: var(--foreground-muted);
    font-size: 0.8rem;
  }

  .values {
    margin-top: 0.5rem;
    display: grid;
    gap: 0.5rem;
  }

  .values > li {
    padding-left: 0.75rem;
    border-left: 2px solid var(--border);
  }

  .value {
    font-weight: 600;
    margin-right: 0.4rem;
  }

  /* "Somebody looked and could not tell" is a recorded judgement, so it reads
     as one rather than as a blank. */
  .insufficient {
    border-left-color: var(--accent);
  }

  .by {
    display: block;
    color: var(--foreground-muted);
    font-size: 0.75rem;
  }

  .factors {
    margin: 0.25rem 0 0;
    padding-left: 1rem;
    list-style: disc;
    color: var(--foreground-secondary);
    font-size: 0.8rem;
  }

  .identifiers > li,
  .evidence > li,
  .merged > li,
  .history > li {
    padding: 0.25rem 0;
    border-bottom: 1px solid var(--border);
  }

  .role-contradicts .badge {
    background: var(--warning-surface);
    color: var(--warning);
  }

  .rationale {
    margin: 0.2rem 0 0;
    color: var(--foreground-secondary);
    font-size: 0.875rem;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }

  code {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8rem;
  }

  .dossier-main > section, .lower-grid > section, .confidence-panel { background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-panel); padding: var(--space-5); min-width: 0; }
  .dossier-heading { grid-column: 1 / -1; display: flex; justify-content: space-between; gap: 16px; align-items: center; }
  .type { background: var(--accent-surface); color: var(--accent); font-size: 12px; padding: 4px 8px; text-transform: capitalize; }
  .lower-grid > section { align-self: start; }
  .identity { background: var(--accent-surface); color: var(--accent); border: 1px solid var(--border); padding: 4px 10px; border-radius: var(--radius-control); font-size: 12px; }
  .dossier-main, .dossier-inspector { display: flex; flex-direction: column; gap: var(--space-5); min-width: 0; }
  .lower-grid { display: grid; grid-template-columns: minmax(0,1.15fr) minmax(0,1fr); grid-template-areas: "ident activity" "relationships activity" "evidence evidence" "decisions decisions"; gap: 20px; }
  .identifier-panel { grid-area: ident; } .relationships-panel { grid-area: relationships; } .evidence-panel { grid-area: evidence; } .history-panel { grid-area: decisions; } .activity-panel { grid-area: activity; min-width: 0; }
  .relationship-row { display: block; width: 100%; text-align: left; margin-bottom: 8px; padding: 12px; }
  .pages { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 16px; }
  .link { overflow-wrap: anywhere; text-align: left; min-height: 32px; }
  @media (max-width: 1180px) { .entity { grid-template-columns: 1fr; } .confidence-panel { grid-column: 1; grid-row: auto; } }
  @media (max-width: 1020px) { .lower-grid { grid-template-columns: 1fr; grid-template-areas: "ident" "relationships" "activity" "evidence" "decisions"; } }
</style>
