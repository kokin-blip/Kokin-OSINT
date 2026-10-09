<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, label, children, class: className = '' }: { title?: string; label?:string; children: Snippet; class?: string } = $props();
</script>
<section aria-label={label??title} class="panel {className}">
  {#if title}<h3>{title}</h3>{/if}
  {@render children()}
</section>
<style>
  .panel { background: var(--surface-panel); border: 1px solid var(--border); border-radius: var(--radius-panel); padding: var(--space-6); min-width: 0; }
  h3 { margin: 0 0 var(--space-4); font-size: var(--text-body); font-weight: 600; }
</style>
