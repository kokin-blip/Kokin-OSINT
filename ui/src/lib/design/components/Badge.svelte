<script lang="ts">
  import type { Snippet } from 'svelte';
  let { children, tone = 'neutral' }: { children: Snippet; tone?: 'neutral' | 'accent' | 'warning' | 'success' } = $props();
</script>
<span class="badge {tone}">{@render children()}</span>
<style>
  .badge { display: inline-flex; align-items: center; border: 1px solid var(--border); border-radius: 7px; padding: 2px 8px; font-size: var(--text-small); color: var(--foreground-secondary); background: var(--surface-raised); }
  .accent { color: var(--accent); background: var(--accent-surface); }
  .warning { color: var(--warning); background: var(--warning-surface); }
  .success { color: var(--success); }
</style>
