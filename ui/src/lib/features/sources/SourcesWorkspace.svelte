<script lang="ts">
 import * as api from '../../bindings';
 import type {CaseOperations} from '../operations/controller.svelte';
 import ErrorNotice from '../../ErrorNotice.svelte';
 import ResearchWorkspace from './ResearchWorkspace.svelte';
 let {operations,active,onopen,generation,onCandidate}:{operations:CaseOperations;active:boolean;onopen:(id:string)=>void;generation:number;onCandidate?:(proposal:api.ResearchCandidate,action:'create_entity'|'add_identifier'|'relate')=>void}=$props();
 let sourceView=$state('public');
 let connector=$state('dns');let input=$state('');let recordType=$state('A');let page=$state(0);let timestamp=$state('');let consent=$state(false);
 let disclosure=$state<api.SourceDisclosureView|null>(null);let error=$state<api.CommandError|null>(null);let preparing=$state(false);let version=0;let live=true;
 import {onMount,untrack} from 'svelte';
 onMount(()=>()=>{live=false;version++;});
 $effect(()=>{connector;input;recordType;page;timestamp;untrack(()=>{version++;});disclosure=null;consent=false;});
 function request():api.SourceRequest{return{version:1,connector,input,record_type:recordType,page,timestamp,consent};}
 async function prepare(event:SubmitEvent){event.preventDefault();const current=++version;preparing=true;error=null;disclosure=null;consent=false;try{const result=await api.sourceDisclosure(request());if(live&&current===version)disclosure=result;}catch(raw){if(live&&current===version)error=api.asCommandError(raw);}finally{if(live)preparing=false;}}
 async function collect(){if(!disclosure||!consent||!operations.online)return;await operations.run('source_collect',request());}
</script>
<section aria-label="Public sources" hidden={!active}>
 <nav aria-label="Source workspaces"><button aria-pressed={sourceView==='public'} onclick={()=>sourceView='public'}>Public records</button><button aria-pressed={sourceView==='research'} onclick={()=>sourceView='research'}>Research and tools</button></nav>
 {#if sourceView==='research'}<ResearchWorkspace {generation} online={operations.online} {onopen} {operations} {onCandidate}/>{:else}
 <header><h2>Public records</h2><p>Collect a disclosed public response, inspect its observations, then decide which records it supports. No browser sessions or automatic analytical decisions.</p></header>
 <form onsubmit={prepare}>
  <label>Source<select bind:value={connector}><option value="dns">DNS · Cloudflare public resolver</option><option value="rdap">RDAP · reviewed IANA service</option><option value="crt_sh">Certificate history · crt.sh</option><option value="wayback">Wayback · capture discovery</option><option value="wayback_capture">Wayback · retrieve archived page</option></select></label>
  <label>{connector.startsWith('wayback')?'Original public URL':'Public domain'}<input bind:value={input} required maxlength="4096" placeholder={connector.startsWith('wayback')?'https://example.org/page':'example.org'}/></label>
  {#if connector==='dns'}<label>Record type<select bind:value={recordType}>{#each ['A','AAAA','CNAME','MX','NS','TXT','SOA'] as kind}<option>{kind}</option>{/each}</select></label>{/if}
  {#if connector==='wayback'}<label>Discovery page<input type="number" min="0" max="100" step="1" bind:value={page}/></label>{/if}
  {#if connector==='wayback_capture'}<label>Selected capture timestamp<input bind:value={timestamp} required pattern={'[0-9]{14}'} placeholder="YYYYMMDDhhmmss"/></label>{/if}
  <button disabled={preparing||operations.busy}>{preparing?'Preparing disclosure…':'Review request'}</button>
 </form>
 {#if error}<ErrorNotice {error}/>{/if}
 {#if disclosure}<section class="disclosure" aria-label="Request disclosure"><h3>Before collection</h3><p>Provider health: {disclosure.health}. Maximum response: {disclosure.max_response_bytes/1024/1024} MiB. One request at a time, at most one per second.</p><p>Transmitted fields: {disclosure.transmitted_fields.join(', ')}. The provider also sees your network address.</p><ul>{#each disclosure.request_urls as url}<li><code>{url}</code></li>{/each}</ul><p>{disclosure.limitation}</p><p>Requests use the approved broker. Cross-origin redirects are refused. Fixtures are version {disclosure.fixtures_version}; live operation does not create recordings.</p><label class="consent"><input type="checkbox" bind:checked={consent}/>I authorize this disclosed request.</label><button type="button" disabled={!consent||!operations.online||operations.busy} onclick={collect}>Collect into Evidence</button>{#if !operations.online}<p>Collection is offline. Enable online mode above to proceed.</p>{/if}</section>{/if}
 {#if operations.result&&typeof operations.result.artifact_id==='string'}<button type="button" onclick={()=>onopen(operations.result!.artifact_id as string)}>Inspect collected evidence</button>{/if}
 <p class="hint">Archive discovery does not retrieve a page. Enter a selected timestamp for the separate capture action. Results never silently fall back to a live site; all limitations remain visible in Evidence.</p>
 {/if}
</section>
<style>
 section{min-width:0}nav{display:flex;gap:12px;margin-bottom:24px;flex-wrap:wrap}header{margin-bottom:24px}h2{margin:4px 0 12px}p{color:var(--foreground-secondary);max-width:78ch;line-height:1.6}form{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:16px;align-items:end}label{display:grid;gap:8px;font-size:13px}input,select{width:100%;box-sizing:border-box}.disclosure{margin:24px 0;padding:24px;border:1px solid var(--border);border-radius:var(--radius-panel);background:var(--surface-panel)}code{overflow-wrap:anywhere}.consent{display:flex;align-items:center;margin:16px 0}.consent input{width:auto}.hint{font-size:12px}@media(max-width:800px){form{grid-template-columns:1fr}}
</style>
