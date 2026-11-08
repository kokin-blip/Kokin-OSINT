<script lang="ts">
 import {onMount} from 'svelte';
 import * as api from '../../bindings';
 import ErrorNotice from '../../ErrorNotice.svelte';
 let {onchoose,label='Find records'}:{onchoose:(record:{id:string;name:string})=>void;label?:string}=$props();
 let query=$state(''),submitted='',offset=$state(0),total=$state(0),rows=$state<Array<{id:string;name:string}>>([]),busy=$state(false),error=$state<api.CommandError|null>(null);let live=true,version=0;
 onMount(()=>{void load();return()=>{live=false;version++;};});
 async function load(){const current=++version;busy=true;error=null;try{const result=submitted?await api.searchCase({query:submitted,kinds:['entity'],offset,limit:50,resolve_merged:true}):await api.entityPage(offset);if(!live||current!==version)return;rows='hits'in result?result.hits.map(r=>({id:r.canonical_id??r.subject_id,name:r.title})):result.rows.map(r=>({id:r.entity_id,name:r.display_name}));total=result.total;}catch(raw){if(live&&current===version)error=api.asCommandError(raw);}finally{if(live&&current===version)busy=false;}}
 function search(){submitted=query.trim();offset=0;void load();}
</script>
<section aria-label={label}><div><label>{label}<input type="search" bind:value={query} onkeydown={e=>{if(e.key==='Enter'){e.preventDefault();search();}}}/></label><button type="button" disabled={busy} onclick={search}>Find records</button></div>
 {#if error}<ErrorNotice {error}/>{/if}{#if busy}<p role="status">Finding stored records…</p>{/if}
 {#each rows as row}<button type="button" onclick={()=>onchoose(row)}>{row.name}</button>{/each}
 {#if !busy&&!rows.length}<p>No matching records. Create an entity from its evidence first.</p>{/if}
 <div><button type="button" disabled={busy||offset===0} onclick={()=>{offset=Math.max(0,offset-50);void load();}}>Previous records</button><span>{rows.length?offset+1:0}–{Math.min(offset+rows.length,total)} of {total}</span><button type="button" disabled={busy||offset+50>=total} onclick={()=>{offset+=50;void load();}}>Next records</button></div></section>
<style>section>div{display:flex;flex-wrap:wrap;align-items:end;gap:8px}label{display:grid;gap:8px}section>button{margin:8px 8px 8px 0}section{margin:16px 0}span{padding:8px}</style>
