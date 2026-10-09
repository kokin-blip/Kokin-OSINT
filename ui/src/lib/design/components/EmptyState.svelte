<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, description, children }: { title: string; description: string; children?: Snippet } = $props();
</script>
<div class="empty-state"><h3>{title}</h3><p>{description}</p>{#if children}{@render children()}{/if}</div>
<style>
  .empty-state { border: 1px dashed var(--border-strong); border-radius: var(--radius-panel); padding: var(--space-8); }
  h3 { margin: 0 0 var(--space-2); font-size: var(--text-body); }
  p { margin: 0 0 var(--space-4); color: var(--foreground-secondary); max-width: 50ch; }
</style>
