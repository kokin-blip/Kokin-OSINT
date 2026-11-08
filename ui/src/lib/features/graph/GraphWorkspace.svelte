<script lang="ts">
 import {onMount} from 'svelte';
 import * as api from '../../bindings';
 import GraphPanel from '../../GraphPanel.svelte';
 import EntityPicker from '../pickers/EntityPicker.svelte';
 import ErrorNotice from '../../ErrorNotice.svelte';
 import {decodeBoard,type BoardState} from './board';
 let {generation,onopen,onselection,caseBusy=false}:{generation:number;caseBusy?:boolean;onopen:(kind:string,id:string)=>void;onselection:(ids:string[])=>void}=$props();
 let seeds=$state<string[]>([]),seedNames=$state<Record<string,string>>({}),boards=$state<api.WorkspaceSummaryView[]>([]),offset=$state(0),total=$state(0),name=$state('Investigation board'),loaded=$state<api.WorkspaceView|null>(null),initial=$state<BoardState>(),epoch=$state(0),error=$state<api.CommandError|null>(null),busy=$state(false),message=$state('');let live=true,loadVersion=0;
 let canvas=$state<{snapshot:()=>BoardState}|undefined>();
 export function addSeed(id:string,name=id){if(!seeds.includes(id)&&seeds.length<300)seeds=[...seeds,id];seedNames[id]=name;}
 onMount(()=>{void refresh();return()=>{live=false;loadVersion++;};});
 export async function refresh(){try{const page=await api.workspacePage(generation,'board',offset);if(live){boards=page.rows;total=page.total;}}catch(raw){if(live)error=api.asCommandError(raw);}}
 async function openBoard(id:string){const version=++loadVersion;busy=true;error=null;try{const row=await api.workspaceRead(generation,id);if(!live||version!==loadVersion)return;loaded=row;name=row.name;initial=decodeBoard(row.payload);seeds=initial.roots??[];epoch++;message='Loaded current records. Saved layout restored.';}catch(raw){if(live&&version===loadVersion)error=api.asCommandError(raw);}finally{if(live&&version===loadVersion)busy=false;}}
 async function save(archived=false){if(busy||caseBusy||!canvas)return;busy=true;error=null;try{const row=await api.workspaceSave(generation,{id:loaded?.id??crypto.randomUUID(),kind:'board',name,expected_revision:loaded?.revision??0,payload:canvas.snapshot(),archived});if(!live)return;loaded=row;message=archived?'Board archived; its history is retained.':`Saved revision ${row.revision}.`;await refresh();}catch(raw){if(live)error=api.asCommandError(raw);}finally{if(live)busy=false;}}
 function fresh(){loadVersion++;loaded=null;initial=undefined;seeds=[];seedNames={};name='Investigation board';epoch++;message='';onselection([]);}
</script>
<section class="graph-workspace" aria-labelledby="graph-workspace-title"><header><h3 id="graph-workspace-title">Graph</h3><button type="button" onclick={fresh} disabled={busy}>New board</button></header>
 {#if caseBusy}<p role="status">A case operation is in progress. Workspace writes are paused until it finishes, including consistent export snapshots.</p>{/if}
 <p>Explore and record evidence-backed connections. Boards save your view inside this encrypted case; layout does not establish confidence.</p>
 {#if error}<ErrorNotice {error}/>{/if}{#if message}<p role="status">{message}</p>{/if}
 <details><summary>Saved boards ({total})</summary>{#each boards as board}<button type="button" disabled={busy} onclick={()=>void openBoard(board.id)}>{board.name} · revision {board.revision}</button>{/each}<button type="button" disabled={offset===0||busy} onclick={()=>{offset-=50;void refresh();}}>Previous boards</button><button type="button" disabled={offset+50>=total||busy} onclick={()=>{offset+=50;void refresh();}}>Next boards</button></details>
 <details open={seeds.length===0}><summary>Add graph starting records</summary><EntityPicker label="Add graph starting records" onchoose={row=>addSeed(row.id,row.name)}/></details>
 <details open={seeds.length<5}><summary>{seeds.length} starting records selected</summary><div class="seed-list">{#each seeds as id}<button type="button" onclick={()=>seeds=seeds.filter(seed=>seed!==id)}>Remove {seedNames[id]??initial?.view?.nodes.find(n=>n.id===id)?.name??id}</button>{/each}</div></details>
 {#if seeds.length}{#key epoch}<GraphPanel expanded={true} bind:this={canvas} seed={seeds[0]} {seeds} {initial} {generation} {onopen} {onselection}/>{/key}
 <form onsubmit={e=>{e.preventDefault();void save();}}><label>Board name<input required maxlength="120" bind:value={name}/></label><button disabled={busy||caseBusy}>Save board</button>{#if loaded}<button type="button" disabled={busy||caseBusy} onclick={()=>void save(true)}>Archive board</button>{/if}</form>
 {:else}<p>Add one or more starting records, then expand within limits. Import evidence and create grounded entities if this case is empty.</p>{/if}
 {#if Array.isArray(initial?.notices)}{#each initial.notices as notice}<p role="status">{String(notice)}</p>{/each}{/if}
</section>
<style>.seed-list{max-height:240px;overflow:auto}.seed-list button{margin:6px}header,form{display:flex;flex-wrap:wrap;gap:12px;align-items:end}h3{font-size:28px;margin:0 auto 0 0}label{display:grid;gap:8px}section{padding:24px;border:1px solid var(--border);border-radius:var(--radius-panel);background:var(--surface-panel)}p{max-width:75ch}details button{margin:8px}form{margin-top:16px}</style>
