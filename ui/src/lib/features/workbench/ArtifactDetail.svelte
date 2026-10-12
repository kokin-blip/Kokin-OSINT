<script lang="ts">import type {EvidenceListView,PageView,ObservationRowView} from '../../bindings';let {table,observations,detailBusy,observationOffset=$bindable(0),showWithdrawn=$bindable(false),onpage,onopen}:{table:EvidenceListView;observations:PageView<ObservationRowView>;detailBusy:boolean;observationOffset?:number;showWithdrawn?:boolean;onpage:(delta:number)=>void;onopen:(kind:string,id:string)=>void}=$props();</script>

<section aria-label="Document observations">        <h3>Document observations</h3>
<p>{table.artifact.media_type} · {table.artifact.extraction.replaceAll("_"," ")}</p>
        {#if table.gaps.length}<aside class="coverage">
<strong>Incomplete reading</strong>{#each table.gaps as gap}<p>{gap.detail} {gap.magnitude??""} {gap.unit??""}</p>{/each}</aside>{/if}
        <label class="check">
<input type="checkbox" bind:checked={showWithdrawn} onchange={()=>{observationOffset=0;onpage(0);}}/>Show withdrawn ({table.superseded})</label>
        {#each observations.rows as row (row.observation_id)}<button class="row" type="button" onclick={()=>onopen('observation',row.observation_id)}>
<strong>{row.kind}{row.superseded?" · withdrawn":""}</strong>
<span>{row.value}</span>
</button>{/each}
        <div class="pagination">
<button type="button" disabled={detailBusy||observationOffset===0} onclick={()=>onpage(-50)}>Previous</button>
<span>{observations.total===0?0:observationOffset+1}–{Math.min(observationOffset+50,observations.total)} of {observations.total}</span>
<button type="button" disabled={detailBusy||observationOffset+50>=observations.total} onclick={()=>onpage(50)}>Next</button>
</div>
        <button type="button" onclick={()=>onopen('document',table.artifact.artifact_id)}>Open source document</button>
</section>

<style>

section{padding:20px;border:1px solid var(--border);border-radius:var(--radius-panel);background:var(--surface-panel)}
  h3{margin-top:0;font-size:24px}
  .coverage{padding:16px;background:var(--warning-surface);border:1px solid var(--border);border-radius:var(--radius-control)}
  .check{display:flex;gap:8px;align-items:center;margin:16px 0}
  .row{display:block;text-align:left;width:100%;padding:14px;margin-bottom:8px;background:transparent;overflow-wrap:anywhere}
  .row span{display:block;color:var(--foreground-secondary);margin-top:6px}
  .pagination{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin:20px 0;font-size:12px;color:var(--foreground-secondary)}


</style>
