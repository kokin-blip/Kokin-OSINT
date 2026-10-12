<script lang="ts">
 import type {SearchView} from '../../bindings';
 let {busy,search,query=$bindable(''),kindFilter=$bindable(''),facetFilter=$bindable(''),includeSuperseded=$bindable(false),resolveMerged=$bindable(true),onsubmit}:{busy:boolean;search:SearchView|null;query?:string;kindFilter?:string;facetFilter?:string;includeSuperseded?:boolean;resolveMerged?:boolean;onsubmit:(event:SubmitEvent)=>void}=$props();
</script>

      <form onsubmit={onsubmit}>
        <label>Search this case<input bind:value={query} type="search" placeholder="Name, identifier, or document text" />
</label>
        <label>Kind<select bind:value={kindFilter}>
<option value="">All kinds</option>{#each ["observation","entity","identifier","relationship"] as k}<option value={k}>{k}</option>{/each}</select>
</label>
        <label>Facet<input bind:value={facetFilter} placeholder="Exact facet, or leave empty"/>
</label>
        <label class="check">
<input type="checkbox" bind:checked={includeSuperseded}/>Include withdrawn observations</label>
        <label class="check">
<input type="checkbox" bind:checked={resolveMerged}/>Group merged entities</label>
        <button disabled={busy}>Search</button>
      </form>
      {#if search}
        <aside class="coverage" aria-label="Search coverage">
          <strong>{search.coverage.complete} of {search.coverage.artifacts} documents fully read</strong>
          <p>{search.coverage.never_attempted} unread; {search.coverage.in_progress} in progress; {search.coverage.partial} partial; {search.coverage.failed} failed.</p>
          <p>No match does not establish absence from unread or partly read evidence.</p>
        </aside>
      {/if}
<style>
form{display:grid;grid-template-columns:1fr;gap:12px;margin-bottom:20px}
  form label:first-child{grid-column:1/-1}
  label{display:grid;gap:6px}
  .check{display:flex;align-items:center;gap:8px}
  .coverage{border:1px solid var(--border);background:var(--warning-surface);border-radius:var(--radius-control);padding:16px;margin:16px 0}
  .coverage p{margin:8px 0 0}
  @media(max-width:1100px){form{grid-template-columns:1fr}
  }

.check input{flex:0 0 auto}
</style>
