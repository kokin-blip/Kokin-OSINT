<script lang="ts">
  import {installPreferences,preferences} from '../stores/preferences.svelte';
  installPreferences();
  import Button from './components/Button.svelte';
  import Badge from './components/Badge.svelte';
  import Panel from './components/Panel.svelte';
  import Icon from './components/Icon.svelte';
  import EmptyState from './components/EmptyState.svelte';
  let loading = $state(false);
</script>
<main><h1>Night Ledger component gallery</h1><p>Development-only visual states. No case data or backend writes.</p>
<div class="states"><button onclick={()=>preferences.update({theme:"dark"})}>Dark theme</button><button onclick={()=>preferences.update({theme:"light"})}>Light theme</button><button onclick={()=>preferences.update({reduceMotion:!preferences.value.reduceMotion})}>Toggle reduced motion</button></div>
  <Panel title="Actions"><div class="states"><Button variant="primary" onclick={() => loading = !loading}>{loading ? 'Loading…' : 'Primary action'}</Button><Button>Secondary</Button><Button variant="ghost">Ghost</Button><Button disabled>Disabled</Button><Button aria-label="Add"><Icon name="plus"/></Button></div></Panel>
  <Panel title="Evidence states"><div class="states"><Badge>Not assessed</Badge><Badge tone="accent">Canonical entity</Badge><Badge tone="warning">Disagreement</Badge><Badge tone="success">Exact citation</Badge></div><p>States always include labels; no aggregate confidence score.</p></Panel>
  <Panel title="Fields"><label>Search<input type="search" placeholder="Search this case"/></label><label>Review state<select><option>Not assessed</option><option>Insufficient information</option></select></label><label class="check"><input type="checkbox"/>Include withdrawn observations</label></Panel>
  <EmptyState title="No evidence yet" description="Import a file, then extract supported content explicitly."><Button variant="primary">Example import action</Button></EmptyState>
</main>
<style>main { max-width: 900px; margin: 40px auto; padding: 24px; display: grid; gap: 24px; } h1 { margin: 0; } .states { display: flex; gap: 12px; flex-wrap: wrap; align-items: center; } label { display: grid; gap: 8px; margin: 12px 0; } .check { display: flex; align-items: center; }</style>
