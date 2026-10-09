<script lang="ts">
  import type { ObservationView } from '../../bindings';
  export type SourcePreview = Pick<ObservationView, 'observation_id' | 'value' | 'kind' | 'quote' | 'superseded'> & {lineage: Pick<ObservationView['lineage'], 'artifact'>};
  let { source, sourceError = '', onopen }: { source: SourcePreview | null; sourceError?: string; onopen?: (kind: string, id: string) => void } = $props();
</script>
<section aria-label="Selected source citation">
  <h4>Source-backed observation</h4>
  {#if source}
    {#if source.quote.agreement === 'exact' || source.quote.agreement === 'contains' || source.quote.agreement === 'differs'}
      <blockquote>{source.quote.excerpt.quoted}</blockquote>
      {#if source.quote.agreement === 'differs'}<aside class="warning"><strong>The source differs from the recorded observation.</strong><p>Recorded value: {source.value}</p></aside>{/if}
      {#if source.quote.excerpt.lossy}<p class="warning">This excerpt includes decoding replacements. Inspect the source document before relying on its text.</p>{/if}
    {:else}
      <p class="warning">Source excerpt unavailable: {source.quote.agreement.replaceAll('_', ' ')}.</p><p>Recorded observation: {source.value}</p>
    {/if}
    <p class="by">Recorded observation · {source.kind}{source.superseded ? ' · withdrawn' : ''}</p>
    <div class="source-footer"><p>{source.lineage.artifact.canonical_locator ?? source.lineage.artifact.artifact_id}<span class="by">{source.lineage.artifact.media_type} · citation verdict: {source.quote.agreement.replaceAll('_', ' ')}</span></p>
      {#if onopen}<button class="primary" type="button" onclick={() => onopen('observation', source.observation_id)}>Open cited evidence</button>{/if}
    </div>
    <p class="hint">One linked observation is previewed. Read all supporting, opposing and contextual evidence below.</p>
  {:else}<p class="hint">{sourceError || 'No observation preview available. Open an evidence link to inspect its source.'}</p>{/if}
</section>
<style>
  section { background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-panel); padding: var(--space-5); min-width: 0; }
  h4 { margin: 0 0 16px; font-size: 14px; font-weight: 600; }
  blockquote { margin: 0 0 20px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-raised); padding: 20px; font: 15px/1.7 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 240px; overflow: auto; }
  .source-footer { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 12px; }
  .source-footer p { margin: 0; overflow-wrap: anywhere; min-width: 0; flex: 1; }
  .by, .hint { display: block; color: var(--foreground-secondary); font-size: 12px; }
  .hint { margin-bottom: 0; }
  .warning { border: 1px solid var(--border); border-radius: var(--radius-control); padding: 12px; background: var(--warning-surface); color: var(--warning); overflow-wrap: anywhere; }
</style>
