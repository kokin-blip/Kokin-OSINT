<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  let { children, variant = 'secondary', ...attributes }: HTMLButtonAttributes & { children?: Snippet; variant?: 'primary' | 'secondary' | 'ghost' } = $props();
</script>
<button type="button" {...attributes} class="{variant} {attributes.class ?? ''}">{#if children}{@render children()}{/if}</button>
<style>
  .ghost { background: transparent; border-color: transparent; }
</style>
