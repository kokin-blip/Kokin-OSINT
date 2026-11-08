<script lang="ts">
 import type {ResearchResultFilter} from '../../bindings';
 let {value=$bindable<Partial<ResearchResultFilter>>({})}=$props();
</script>
<details class="filters"><summary>Filter captured fields and profile metadata</summary>
 <p>Filters apply across the run before paging. A profile must match all conditions itself. Unknown metadata is separate from false. Age describes a reported person age; dates describe reported creation or activity.</p>
 <div class="controls">
 <label>Module<input bind:value={value.module} maxlength="128" placeholder="Exact module name"/></label>
 <label>Field key<input bind:value={value.field} maxlength="128" placeholder="Any scalar field"/></label>
 <label>Field pattern<input bind:value={value.pattern} maxlength="256" placeholder="*example*"/></label>
 <label>Verified<select bind:value={value.verified}><option value="">Any</option><option value="true">True</option><option value="false">False</option><option value="unknown">Unknown</option></select></label>
 <label>Private<select bind:value={value.private}><option value="">Any</option><option value="true">True</option><option value="false">False</option><option value="unknown">Unknown</option></select></label>
 <label>Minimum reported age<input type="number" min="0" max="150" bind:value={value.min_age}/></label>
 <label>Maximum reported age<input type="number" min="0" max="150" bind:value={value.max_age}/></label>
 <label>Date field<select bind:value={value.date_field}><option value="creation_date">Creation date</option><option value="last_seen">Last seen</option></select></label>
 <label>From date<input type="date" bind:value={value.date_from}/></label><label>Through date<input type="date" bind:value={value.date_to}/></label>
 </div><p>Patterns are case sensitive: * matches any text, ? one character, and [] a character set. Empty patterns do not restrict fields. Dates use the source's calendar date; they are not observation or indexing dates.</p><button onclick={()=>value={}}>Clear metadata filters</button>
</details>
<style>
 .filters{margin-block:16px;padding-block:12px;border-block:1px solid var(--border)}p{max-width:75ch;color:var(--foreground-secondary)}.controls{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px}label{display:grid;gap:6px;min-width:0}input,select{width:100%;min-width:0}
</style>
