<script lang="ts">
 import * as api from '../../bindings';
 import FaceTile from './FaceTile.svelte';
 let {generation,entityId,onface,onopen}:{generation:number;entityId:string;onface:(face:api.FaceView)=>void;onopen:(kind:string,id:string,page?:number)=>void}=$props();
 let page=$state<api.FacePageView|null>(null),offset=$state(0),error=$state('');
 $effect(()=>{entityId;offset=0;});
 $effect(()=>{let live=true;void api.personFaces(generation,entityId,offset).then(v=>{if(live)page=v;}).catch(()=>{if(live)error='Reviewed face links could not be read.';});return()=>{live=false;};});
</script>
<section aria-label="Reviewed face links"><h3>Reviewed face links</h3>{#if error}<p role="alert">{error}</p>{/if}{#if page?.faces.length}<div class="faces">{#each page.faces as face}<FaceTile {face} onselect={onface} onopen={f=>onopen('document',f.artifact_id,f.page)}/>{/each}</div><div class="toolbar"><button type="button" disabled={offset===0} onclick={()=>offset=Math.max(0,offset-50)}>Previous face links</button><span>{offset+1}–{Math.min(offset+50,page.total)} of {page.total}</span><button type="button" disabled={offset+50>=page.total} onclick={()=>offset+=50}>Next face links</button></div>{:else}<p>No current face detections have an analyst-reviewed link to this person.</p>{/if}</section>
<style>section{margin:24px 0;padding-top:20px;border-top:1px solid var(--border)}h3{font-size:16px}.faces,.toolbar{display:flex;flex-wrap:wrap;gap:16px}.toolbar{align-items:center}p,span{font-size:13px;color:var(--foreground-secondary);line-height:1.6}</style>
