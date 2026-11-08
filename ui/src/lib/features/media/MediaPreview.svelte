<script lang="ts">
 import ImageResearch from './ImageResearch.svelte';
 import FaceOverlay from '../faces/FaceOverlay.svelte';
 import * as api from '../../bindings';
 import ErrorNotice from '../../ErrorNotice.svelte';
 import type {CaseOperations} from '../operations/controller.svelte';
 let {artifact,generation,operations,onopen,onface,initialPage=1}:{artifact:string;generation:number;operations:CaseOperations;onopen?:(id:string,page?:number)=>void;onface?:(face:api.FaceView)=>void;initialPage?:number}=$props();
 let resource=$state<api.OcrResourceView|null>(null),installing=$state(false),accepted=$state(false);
 $effect(()=>{let active=true;void api.ocrResourceInfo().then(value=>{if(active)resource=value;}).catch(raw=>{if(active)error=api.asCommandError(raw);});return()=>{active=false;};});
 async function install(){if(!accepted)return;installing=true;error=null;try{await api.installOcrResource();resource=await api.ocrResourceInfo();}catch(raw){error=api.asCommandError(raw);}finally{installing=false;}}
 let page=$state(1),view=$state<api.MediaView|null>(null),loading=$state(false),error=$state<api.CommandError|null>(null);
 $effect(()=>{page=initialPage;});
 $effect(()=>{const id=artifact,current=page,refresh=operations.result;void refresh;let active=true;loading=true;view=null;error=null;if(!Number.isInteger(current)||current<1||current>65536){loading=false;return()=>{active=false;};}
 void api.mediaRead(generation,id,current-1).then(result=>{if(active)view=result;}).catch(raw=>{if(active)error=api.asCommandError(raw);}).finally(()=>{if(active)loading=false;});return()=>{active=false;};});
 function process(ocr=false){void operations.run('media_extract',{version:1,artifact_id:artifact,page:page-1,ocr});}
</script>
<section aria-label="Media preview" class="media-preview">
 <h3>Media evidence</h3>
 <div class="toolbar"><label>Page<input type="number" min="1" max={view?.pages??65535} bind:value={page}/></label><button type="button" disabled={operations.busy||!Number.isInteger(page)||page<1} onclick={()=>void operations.run("media_preview",{version:1,artifact_id:artifact,page:page-1,ocr:false})}>Render page preview</button><button type="button" disabled={operations.busy||!Number.isInteger(page)||page<1} onclick={()=>process()}>Process and preview</button><button type="button" disabled={operations.busy||!resource?.installed||!Number.isInteger(page)||page<1} onclick={()=>process(true)}>OCR selected page</button></div>
 <p>Processing runs locally. OCR requires the reviewed English resource. Original bytes are preserved; previews and OCR are derived readings.</p>
 {#if resource&&!resource.installed}<details><summary>Install optional English OCR data</summary><p>Copy the reviewed local eng.traineddata file ({(resource.size_bytes/1024/1024).toFixed(2)} MiB) to {resource.destination}. {resource.license}. No network request is made.</p><label><input type="checkbox" bind:checked={accepted}/>I want to install this resource at the displayed location.</label><button type="button" disabled={!accepted||installing} onclick={install}>{installing?'Installing…':'Choose local OCR data'}</button></details>{/if}
 {#if error}<ErrorNotice {error}/>{/if}
 {#if loading}<p role="status">Reading encrypted preview…</p>{:else if view}
 <div class="preview-source"><img src={`data:image/png;base64,${view.preview_png}`} alt={`Derived source preview, page ${view.page+1} of ${view.pages}`}/><FaceOverlay {artifact} {generation} page={view.page} {onface}/></div><p>{view.engine} · Page {view.page+1} of {view.pages}</p>
 {#each view.limitations as limitation}<p class="limitation">{limitation}</p>{/each}
 {#if view.duplicate_candidates.length}<p>Preview similarity candidates: {#each view.duplicate_candidates as candidate}<button type="button" onclick={()=>onopen?.(candidate)}>{candidate}</button>{/each}. Similarity does not establish identity or authenticity.</p>{/if}
 {#if view.candidate_scan_truncated}<p>Similarity scan was bounded; additional candidates may exist.</p>{/if}
 {:else}<p>No processed preview for this page. Choose Process and preview to extract supported content.</p>{/if}
<ImageResearch {artifact} {generation} page={page-1} {operations} {onopen} {onface}/>
</section>
<style>.preview-source{position:relative;width:fit-content;max-width:100%}.media-preview{margin:24px 0;padding:20px;border:1px solid var(--border);border-radius:var(--radius-panel)}.toolbar{display:flex;flex-wrap:wrap;gap:12px;align-items:end}label{display:grid;gap:6px}input{width:6rem}img{display:block;max-width:100%;max-height:650px;object-fit:contain;background:var(--surface-raised)}p{color:var(--foreground-secondary);max-width:75ch;overflow-wrap:anywhere}.limitation{color:var(--warning)}</style>
