<script lang="ts">
 import SearchControls from '../search/SearchControls.svelte';
 import ActivityHistory from '../activity/ActivityHistory.svelte';
 import type * as api from '../../bindings';
 let {tab,busy,total,offset,artifacts,entities,search,activity,query=$bindable(''),kindFilter=$bindable(''),facetFilter=$bindable(''),includeSuperseded=$bindable(false),resolveMerged=$bindable(true),onsubmit,onopen,onpage}:{tab:string;busy:boolean;total:number;offset:number;artifacts:api.ArtifactRowView[];entities:api.EntityRowView[];search:api.SearchView|null;activity:api.ActivityView|null;query?:string;kindFilter?:string;facetFilter?:string;includeSuperseded?:boolean;resolveMerged?:boolean;onsubmit:(event:SubmitEvent)=>void;onopen:(route:{kind:string;id:string})=>void;onpage:(delta:number)=>void}=$props();
</script>


    <h3>{tab=== 'Search' ? 'Search and filters' : `${tab} library`}</h3>
    {#if tab==="Search"}
      <SearchControls {busy} {search} bind:query bind:kindFilter bind:facetFilter bind:includeSuperseded bind:resolveMerged {onsubmit}/>
    {/if}
    {#if busy}<p role="status">Reading the case…</p>{/if}
    {#if tab==="Evidence"}
      {#each artifacts as row (row.artifact_id)}
        <button class="row" type="button" onclick={()=>onopen({kind:"artifact",id:row.artifact_id})}>
          <strong>{row.canonical_locator??row.artifact_id}</strong>
<span>{row.media_type} · {row.observations} observations · {row.extraction.replaceAll("_"," ")}</span>
        </button>
      {/each}
    {:else if tab==="Entities"}
      {#each entities as row (row.entity_id)}
        <button class="row" type="button" onclick={()=>onopen({kind:"entity",id:row.entity_id})}>
<strong>{row.display_name}</strong>
<span>{row.type_key} · {row.identifier_count} identifiers · {row.evidence_count} evidence links · {row.merged_count} merged records</span>
</button>
      {/each}
    {:else if tab==="Search" && search}
      {#each search.hits as hit (hit.subject_kind+hit.subject_id)}
        <button class="row" type="button" onclick={()=>onopen({kind:hit.subject_kind,id:hit.canonical_id??hit.subject_id})}>
<strong>{hit.title}</strong>
<span>{hit.subject_kind} · {hit.facet}{hit.superseded?" · withdrawn":""}{hit.canonical_id?" · merged alias":""}</span>
<span>{hit.snippet}</span>
</button>
      {/each}
    {:else if tab==="Activity" && activity}
      <ActivityHistory {activity} {onopen}/>
    {/if}
    {#if !busy && total===0}<p class="empty">{tab==="Evidence"?"No documents yet. Import a file or collect a public page to begin.":tab==="Entities"?"No entities yet. Read evidence, then create an evidence-grounded entity.":tab==="Search"?"No matching results. Check the coverage above before drawing conclusions.":"No activity recorded."}</p>{/if}
    <div class="pagination">
<button type="button" disabled={busy||offset===0} onclick={()=>onpage(-50)}>Previous</button>
<span>{total===0?0:offset+1}–{Math.min(offset+50,total)} of {total}</span>
<button type="button" disabled={busy||offset+50>=total} onclick={()=>onpage(50)}>Next</button>
</div>

<style>
h3{font-size:16px;margin:0 0 20px}
  .row{display:block;width:100%;text-align:left;background:transparent;border:1px solid var(--border);padding:14px;overflow-wrap:anywhere;margin-bottom:8px}
  .row span{display:block;color:var(--foreground-secondary);margin-top:6px;font-size:12px}
  .pagination{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:20px;color:var(--foreground-secondary);font-size:12px;font-variant-numeric:tabular-nums}
  .empty{color:var(--foreground-secondary)}

</style>
