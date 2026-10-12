<script lang="ts">
 import {onMount} from 'svelte';
 import * as api from './bindings';
 import ErrorNotice from './ErrorNotice.svelte';
 let {seed,generation,onopen}:{seed:string;generation:number;onopen:(kind:string,id:string)=>void}=$props();
 let hops=$state(2);let limit=$state(40);let perNode=$state(16);let view=$state<api.GraphView|null>(null);let busy=$state(false);let error=$state<api.CommandError|null>(null);let job=$state('');let live=true;
 onMount(()=>()=>{live=false;if(job)void api.cancelOperation(job).catch(()=>{});});
 async function expand(){busy=true;error=null;try{job=await api.startOperation('graph',{seed,hops,node_limit:limit,per_node:perNode},generation);let status=await api.operationStatus(job);while(live&&status.status==='running'){await new Promise(r=>setTimeout(r,100));status=await api.operationStatus(job);}if(!live)return;if(status.status==='completed')view=JSON.parse(status.result_json??'{}');else error=status.error;}catch(raw){if(live)error=api.asCommandError(raw);}finally{job='';if(live)busy=false;}}
 async function cancel(){if(job)await api.cancelOperation(job);}
 let positioned=$derived((view?.nodes??[]).map((node,i,nodes)=>({...node,x:100+node.depth*260,y:50+nodes.slice(0,i).filter(n=>n.depth===node.depth).length*70})));
 let height=$derived(Math.max(260,...positioned.map(n=>n.y+60)));
 let width=$derived(Math.max(600,...positioned.map(n=>n.x+220)));
</script>
<details class="graph"><summary>Explore relationships</summary>
 <p>Expand stored entity records. Merged aliases open their canonical detail. Position and distance describe traversal only; they do not indicate confidence.</p>
 <form onsubmit={e=>{e.preventDefault();void expand();}}>
 <label>Hops<input type="number" min="1" max="4" bind:value={hops} required/></label>
 <label>Node limit<input type="number" min="1" max="300" bind:value={limit} required/></label>
 <label>Links per direction per node<input type="number" min="1" max="128" bind:value={perNode} required/></label>
 <button disabled={busy}>Expand graph</button>{#if busy}<button type="button" onclick={cancel}>Cancel expansion</button>{/if}
 </form>
 {#if error}<ErrorNotice {error}/>{/if}
 {#if busy}<p role="status">Reading bounded relationship hops…</p>{/if}
 {#if view}
 <p role="status">{view.nodes.length} records, {view.edges.length} {view.edges.length===1?'relationship':'relationships'}; {view.completed_hops} hops completed.{#if view.truncated} <strong>Truncated by traversal limits. Unshown relationships may exist.</strong>{/if}</p>
 <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard access is required to scroll this bounded viewport.) -->
 <div class="drawing" tabindex="0" role="region" aria-label="Relationship graph">
 <svg {width} {height} aria-label="Entity records and relationships">
 <defs><marker id="graph-arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--foreground-secondary)"/></marker></defs>
 {#each view.edges as edge}
 {@const from=positioned.find(n=>n.id===edge.from)}{@const to=positioned.find(n=>n.id===edge.to)}
 {#if from&&to}<a href="#relationship-detail" onclick={e=>{e.preventDefault();onopen('relationship',edge.id);}} aria-label={`Open ${edge.kind} relationship`}><title>{edge.kind}</title>{#if from.id===to.id}<path class="edge" d={`M ${from.x} ${from.y-20} C ${from.x-60} ${from.y-65}, ${from.x+60} ${from.y-65}, ${from.x+30} ${from.y-20}`} marker-end="url(#graph-arrow)"/>{:else}<line x1={from.x+(to.x>from.x?110:to.x<from.x?-80:15)} y1={from.y+(to.x===from.x?(to.y>from.y?20:-20):0)} x2={to.x+(to.x>from.x?-80:to.x<from.x?110:15)} y2={to.y+(to.x===from.x?(to.y>from.y?-20:20):0)} marker-end="url(#graph-arrow)"/>{/if}<text x={(from.x+to.x)/2} y={(from.y+to.y)/2-30} text-anchor="middle">{edge.kind}</text></a>{/if}
 {/each}
 {#each positioned as node}<a href="#entity-detail" onclick={e=>{e.preventDefault();onopen('entity',node.canonical_id);}} aria-label={`Open entity ${node.name}`}><title>{node.name}{node.id!==node.canonical_id?' (merged alias)':''}</title><rect x={node.x-80} y={node.y-20} width="190" height="40" rx="4"/><text x={node.x-70} y={node.y+5}>{node.name.length>23?node.name.slice(0,22)+'…':node.name}</text></a>{/each}
 </svg></div>
 <details><summary>Records and relationships as a list</summary>{#each view.nodes as n}<button type="button" onclick={()=>onopen('entity',n.canonical_id)}>{n.name}{n.id!==n.canonical_id?' (merged alias)':''}</button>{/each}{#each view.edges as e}<button type="button" onclick={()=>onopen('relationship',e.id)}>{e.kind}: {view.nodes.find(n=>n.id===e.from)?.name} → {view.nodes.find(n=>n.id===e.to)?.name}</button>{/each}</details>
 {/if}
</details>
<style>
 .graph{margin:1rem 0;padding:1rem;border:1px solid var(--border-strong);border-radius:4px}summary{cursor:pointer}p{max-width:70ch}form{display:flex;flex-wrap:wrap;gap:.75rem;align-items:end}label{display:grid;gap:.25rem;font-size:.875rem}input{width:6rem}.drawing{overflow:auto;max-height:32rem;margin:1rem 0;background:var(--surface-panel);border:1px solid var(--border-strong)}svg{display:block}line,.edge{fill:none;stroke:var(--foreground-secondary);stroke-width:3}rect{fill:var(--surface-raised);stroke:var(--accent);stroke-width:1}text{fill:var(--foreground);font-size:12px}a:hover line,a:focus line,.edge{fill:none;stroke:var(--foreground);stroke-width:5}a:hover rect,a:focus rect{fill:var(--accent-surface)}a:focus{outline:2px solid var(--accent)}details button{margin:.3rem;overflow-wrap:anywhere}strong{color:var(--warning)}
</style>
