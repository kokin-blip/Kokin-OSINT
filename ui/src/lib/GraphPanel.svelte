<script lang="ts">
 import {onMount} from 'svelte';
 import {mergeViews,type BoardState} from './features/graph/board';
 import {zoomAt,fitCamera,centerCamera,type Camera} from './features/graph/viewport';
 import * as api from './bindings';
 import ErrorNotice from './ErrorNotice.svelte';
 let {seed,generation,onopen,seeds=[],initial,expanded=false,onselection=()=>{}}:{seed:string;generation:number;onopen:(kind:string,id:string)=>void;seeds?:string[];initial?:BoardState;expanded?:boolean;onselection?:(ids:string[])=>void}=$props();
 let direction=$state('both');let relationshipKind=$state('');let selected=$state<string[]>([]);let roots=$state<string[]>([]);let positions=$state<Record<string,{x:number;y:number;pinned:boolean}>>({});let visibleQuery=$state('');let suppressClick=false;
 $effect(()=>{if(initial){view=initial.view;camera=initial.camera;positions=initial.positions??{};hops=initial.hops;limit=initial.limit;perNode=initial.perNode;direction=initial.direction??'both';relationshipKind=initial.relationshipKind??'';visibleQuery=initial.visibleQuery??'';roots=initial.roots??[];fitting=false;}});
 export function snapshot():BoardState{return {version:1,view:$state.snapshot(view),camera:$state.snapshot(camera),positions:Object.fromEntries(positioned.map(node=>[node.id,{x:node.x,y:node.y,pinned:positions[node.id]?.pinned??false}])),hops,limit,perNode,direction,relationshipKind,visibleQuery,roots:[...new Set([...seeds,seed,...roots].filter(Boolean))]};}
 function chooseNode(event:MouseEvent,id:string){event.preventDefault();if(suppressClick){suppressClick=false;return;}selected=event.shiftKey?(selected.includes(id)?selected.filter(n=>n!==id):[...selected,id]):[id];onselection([...selected]);if(!event.shiftKey)onopen('entity',id);}
 function place(id:string,axis:'x'|'y',raw:string){const value=Number(raw),node=nodes.get(id);if(!node)return;if(!raw.trim()||!Number.isFinite(value)||Math.abs(value)>1e6){error=api.asCommandError({code:'invalid_value',message:'Enter a finite coordinate between −1000000 and 1000000.'});return;}error=null;positions[id]={x:node.x,y:node.y,pinned:positions[id]?.pinned??false,[axis]:value};}
 function pin(id:string){const node=nodes.get(id);if(node)positions[id]={x:node.x,y:node.y,pinned:!positions[id]?.pinned};}

 let hops=$state(2);let limit=$state(40);let perNode=$state(16);let view=$state<api.GraphView|null>(null);let busy=$state(false);let error=$state<api.CommandError|null>(null);let job=$state('');let live=true;
 onMount(()=>()=>{live=false;if(job)void api.cancelOperation(job).catch(()=>{});});
 async function expand(from?:string[]){if(busy)return;busy=true;error=null;try{const chosen=from??(seeds.length?seeds:[seed]);roots=[...new Set([...roots,...chosen])];const id=await api.startOperation(seeds.length||from?'graph_explore':'graph',{seed,seeds:chosen,hops,node_limit:limit,per_node:perNode,direction,kinds:relationshipKind.trim()?[relationshipKind.trim()]:[]},generation);if(!live){await api.cancelOperation(id);return;}job=id;let status=await api.operationStatus(job);while(live&&status.status==='running'){await new Promise(r=>setTimeout(r,100));if(!live)return;status=await api.operationStatus(job);}if(!live)return;if(status.status==='completed'){view=mergeViews(from?view:null,JSON.parse(status.result_json??'{}'));fitting=true;focusId='';record='';}else error=status.error;}catch(raw){if(live)error=api.asCommandError(raw);}finally{job='';if(live)busy=false;}}
 async function cancel(){if(job)try{await api.cancelOperation(job);}catch(raw){if(live)error=api.asCommandError(raw);}}
 let disclosure:HTMLDetailsElement;let canvas=$state<HTMLDivElement>();let viewportWidth=$state(900);const viewportHeight=420;
 let camera=$state<Camera>({x:0,y:0,scale:1});let fitting=$state(true);let focusId=$state('');let record=$state('');
 const markerId=$props.id();
 export function reveal(){disclosure.open=true;disclosure.scrollIntoView({block:'nearest'});}
 let positioned=$derived.by(()=>{const depths=new Map<number,number>();return (view?.nodes??[]).map(node=>{const index=depths.get(node.depth)??0;depths.set(node.depth,index+1);return {...node,x:positions[node.id]?.x??100+node.depth*260,y:positions[node.id]?.y??50+index*70};});});
 let nodes=$derived(new Map(positioned.map(node=>[node.id,node])));
 let visible=$derived(positioned.filter(n=>!visibleQuery.trim()||n.name.toLowerCase().includes(visibleQuery.trim().toLowerCase())));
 let visibleIds=$derived(new Set(visible.map(n=>n.id)));
 let bounds=$derived.by(()=>{const left=Math.min(0,...positioned.map(n=>n.x-100)),top=Math.min(-20,...positioned.map(n=>n.y-70));return {left,top,width:Math.max(300,...positioned.map(n=>n.x+130))-left,height:Math.max(160,...positioned.map(n=>n.y+60))-top};});
 $effect(()=>{if(fitting)camera=fitCamera(bounds,viewportWidth,viewportHeight);});
 $effect(()=>{if(!canvas||typeof ResizeObserver==='undefined')return;const element=canvas;const observer=new ResizeObserver(()=>{viewportWidth=Math.max(1,element.clientWidth);});observer.observe(element);return()=>observer.disconnect();});
 function fit(){fitting=true;camera=fitCamera(bounds,viewportWidth,viewportHeight);}
 function zoom(factor:number,x=viewportWidth/2,y=viewportHeight/2){fitting=false;camera=zoomAt(camera,factor,x,y);}
 function focus(id:string){const node=nodes.get(id);if(!node)return;fitting=false;focusId=id;camera=centerCamera(camera,node.x+15,node.y,viewportWidth,viewportHeight);}
 function keyboard(e:KeyboardEvent){if(e.target!==canvas||e.altKey||e.ctrlKey||e.metaKey)return;const shifts:Record<string,[number,number]>={ArrowLeft:[60,0],ArrowRight:[-60,0],ArrowUp:[0,60],ArrowDown:[0,-60]};if(shifts[e.key]){e.preventDefault();fitting=false;camera={...camera,x:camera.x+shifts[e.key][0],y:camera.y+shifts[e.key][1]};}else if(e.key==='+'||e.key==='='){e.preventDefault();zoom(1.25);}else if(e.key==='-'){e.preventDefault();zoom(.8);}else if(e.key==='0'){e.preventDefault();fit();}}
 $effect(()=>{if(!canvas)return;const element=canvas;let drag:{x:number;y:number;camera:Camera;node?:string;position?:{x:number;y:number}}|null=null;
  const down=(e:MouseEvent)=>{if(e.button!==0)return;const anchor=e.target instanceof Element?e.target.closest('a'):null;const id=anchor?.getAttribute('data-node');if(anchor&&!id)return;if(id&&positions[id]?.pinned)return;e.preventDefault();element.focus();if(id&&!selected.includes(id)){selected=[id];onselection([...selected]);}suppressClick=false;fitting=false;const node=id?nodes.get(id):null;drag={x:e.clientX,y:e.clientY,camera:{...camera},node:id??undefined,position:node?{x:node.x,y:node.y}:undefined};};
  const move=(e:MouseEvent)=>{if(!drag)return;const rect=element.getBoundingClientRect(),dx=(e.clientX-drag.x)*viewportWidth/Math.max(1,rect.width),dy=(e.clientY-drag.y)*viewportHeight/Math.max(1,rect.height);if(drag.node&&drag.position){if(Math.abs(dx)+Math.abs(dy)>4){suppressClick=true;positions[drag.node]={x:drag.position.x+dx/drag.camera.scale,y:drag.position.y+dy/drag.camera.scale,pinned:false};}}else camera={...drag.camera,x:drag.camera.x+dx,y:drag.camera.y+dy};};
  const end=()=>{drag=null;};
  const wheel=(e:WheelEvent)=>{if(!e.ctrlKey&&!e.metaKey)return;e.preventDefault();const rect=element.getBoundingClientRect();zoom(Math.exp(-Math.max(-100,Math.min(100,e.deltaY))*.005),(e.clientX-rect.left)*viewportWidth/Math.max(1,rect.width),(e.clientY-rect.top)*viewportHeight/Math.max(1,rect.height));};
  element.addEventListener('mousedown',down);element.addEventListener('wheel',wheel,{passive:false});window.addEventListener('mousemove',move);window.addEventListener('mouseup',end);window.addEventListener('blur',end);
  return()=>{element.removeEventListener('mousedown',down);element.removeEventListener('wheel',wheel);window.removeEventListener('mousemove',move);window.removeEventListener('mouseup',end);window.removeEventListener('blur',end);};
 });
</script>
<details class="graph" bind:this={disclosure} open={expanded}><summary>Explore relationships</summary>
 <p>Expand stored entity records. Merged aliases open their canonical detail. Position and distance describe traversal only; they do not indicate confidence.</p>
 <form onsubmit={e=>{e.preventDefault();void expand();}}>
 <label>Hops<input type="number" min="1" max="4" bind:value={hops} required/></label>
 <label>Node limit<input type="number" min="1" max="300" bind:value={limit} required/></label>
 <label>Links per direction per node<input type="number" min="1" max="128" bind:value={perNode} required/></label>
 <label>Direction<select bind:value={direction}><option value="both">Both directions</option><option value="incoming">Incoming</option><option value="outgoing">Outgoing</option></select></label><label>Relationship kind<input bind:value={relationshipKind} placeholder="All kinds"/></label>
 <button disabled={busy}>Expand graph</button>{#if busy}<button type="button" onclick={cancel}>Cancel expansion</button>{/if}
 </form>
 {#if error}<ErrorNotice {error}/>{/if}
 {#if busy}<p role="status">Reading bounded relationship hops…</p>{/if}
 {#if view}
 <p role="status">{view.nodes.length} records, {view.edges.length} {view.edges.length===1?'relationship':'relationships'}; {view.completed_hops} hops completed.{#if view.truncated} <strong>Truncated by traversal limits. Unshown relationships may exist.</strong> {view.truncation_reasons?.join(", ")??""}{/if}</p>
 <label>Filter loaded records<input type="search" bind:value={visibleQuery}/></label><p>{visible.length} visible of {view.nodes.length} loaded records. This filters this canvas, not the entire case.</p>
 <button type="button" disabled={!selected.length||busy} onclick={()=>void expand(selected)}>Expand selected records</button>
 <div class="graph-tools"><button type="button" onclick={()=>zoom(1.25)}>Zoom in</button><button type="button" onclick={()=>zoom(.8)}>Zoom out</button><output aria-label="Graph zoom">{Math.round(camera.scale*100)}%</output><button type="button" onclick={fit}>Fit graph</button><button type="button" onclick={()=>focus(seed)}>Focus seed</button>
 <label>Focus record<select bind:value={record}><option value="">Choose a loaded record</option>{#each positioned as node}<option value={node.id}>{node.name}{node.id!==node.canonical_id?' (merged alias)':''}</option>{/each}</select></label><button type="button" disabled={!record} onclick={()=>focus(record)}>Focus record</button></div>
 <p id={markerId+'-help'} class="hint">Drag empty canvas to pan. With canvas focused: arrows pan, +/− zoom, 0 fits. Control/Command + wheel zooms at the pointer. Links open evidence; the list below provides an alternative.</p>
 {#if selected.length}<p role="status">Selected: {selected.map(id=>nodes.get(id)?.name??id).join(", ")}. Shift-click adds or removes records.</p>{/if}
 {#if focusId}<p role="status">Focused: {nodes.get(focusId)?.name}</p>{/if}
 <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (The graph region supports documented keyboard pan and zoom; evidence links remain native anchors.) -->
 <div class="drawing" bind:this={canvas} tabindex="0" role="region" aria-label="Relationship graph" aria-describedby={markerId+'-help'} onkeydown={keyboard}>
 <svg viewBox={`0 0 ${viewportWidth} ${viewportHeight}`} aria-label="Entity records and relationships">
 <defs><marker id={markerId} viewBox="0 0 10 10" refX="10" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="var(--foreground-secondary)"/></marker></defs>
 <g transform={`translate(${camera.x} ${camera.y}) scale(${camera.scale})`}>
 {#each view.edges as edge}
 {@const from=nodes.get(edge.from)}{@const to=nodes.get(edge.to)}
 {#if from&&to&&visibleIds.has(from.id)&&visibleIds.has(to.id)}<a href="#relationship-detail" onclick={e=>{e.preventDefault();onopen('relationship',edge.id);}} aria-label={`Open ${edge.kind} relationship`}><title>{edge.kind}</title>{#if from.id===to.id}<path class="edge" d={`M ${from.x} ${from.y-20} C ${from.x-60} ${from.y-65}, ${from.x+60} ${from.y-65}, ${from.x+30} ${from.y-20}`} marker-end={`url(#${markerId})`}/>{:else}<line x1={from.x+(to.x>from.x?110:to.x<from.x?-80:15)} y1={from.y+(to.x===from.x?(to.y>from.y?20:-20):0)} x2={to.x+(to.x>from.x?-80:to.x<from.x?110:15)} y2={to.y+(to.x===from.x?(to.y>from.y?-20:20):0)} marker-end={`url(#${markerId})`}/>{/if}<text x={(from.x+to.x)/2} y={(from.y+to.y)/2-30} text-anchor="middle">{edge.kind}</text></a>{/if}
 {/each}
 {#each visible as node}<a data-node={node.id} href="#entity-detail" onclick={e=>chooseNode(e,node.id)} aria-label={`Open entity ${node.name}`}><title>{node.name}{node.id!==node.canonical_id?' (merged alias)':''}</title><rect class:focused={focusId===node.id} class:selected={selected.includes(node.id)} x={node.x-80} y={node.y-20} width="190" height="40" rx="4"/><text x={node.x-70} y={node.y+5}>{node.name.length>23?node.name.slice(0,22)+'…':node.name}</text></a>{/each}
 </g></svg></div>
 {#each selected as id}{@const n=nodes.get(id)}{#if n}<fieldset><legend>Position {n.name}</legend><label>X<input type="number" value={n.x} min="-1000000" max="1000000" onchange={e=>place(id,'x',e.currentTarget.value)}/></label><label>Y<input type="number" value={n.y} min="-1000000" max="1000000" onchange={e=>place(id,'y',e.currentTarget.value)}/></label><button type="button" aria-pressed={positions[id]?.pinned??false} onclick={()=>pin(id)}>{positions[id]?.pinned?'Unpin':'Pin'} record</button></fieldset>{/if}{/each}
 <details><summary>Records and relationships as a list</summary>{#each view.nodes as n}<button type="button" onclick={()=>onopen('entity',n.id)}>{n.name}{n.id!==n.canonical_id?' (merged alias)':''}</button>{/each}{#each view.edges as e}<button type="button" onclick={()=>onopen('relationship',e.id)}>{e.kind}: {view.nodes.find(n=>n.id===e.from)?.name} → {view.nodes.find(n=>n.id===e.to)?.name}</button>{/each}</details>
 {/if}
</details>
<style>
 .graph{margin:1rem 0;padding:1rem;border:1px solid var(--border-strong);border-radius:4px}summary{cursor:pointer}p{max-width:70ch}form{display:flex;flex-wrap:wrap;gap:.75rem;align-items:end}label{display:grid;gap:.25rem;font-size:.875rem}input{width:6rem}.graph-tools{display:flex;flex-wrap:wrap;gap:8px;align-items:end;margin-top:16px}.graph-tools button{margin:0}output{padding:8px}.hint{font-size:12px;color:var(--foreground-secondary)}.drawing{overflow:hidden;height:420px;margin:1rem 0;background:var(--surface-panel);border:1px solid var(--border-strong)}svg{display:block;width:100%;height:100%}.selected{stroke-dasharray:4 2;stroke-width:3}.focused{stroke:var(--foreground);stroke-width:3}line,.edge{fill:none;stroke:var(--foreground-secondary);stroke-width:3}rect{fill:var(--surface-raised);stroke:var(--accent);stroke-width:1}text{fill:var(--foreground);font-size:12px}a:hover line,a:focus line,.edge{fill:none;stroke:var(--foreground);stroke-width:5}a:hover rect,a:focus rect{fill:var(--accent-surface)}a:focus{outline:2px solid var(--accent)}details button{margin:.3rem;overflow-wrap:anywhere}strong{color:var(--warning)}
</style>
