<script lang="ts">
 import AnalysisForm from './AnalysisForm.svelte';
 import CollectionForm from './CollectionForm.svelte';
 import PackageForm from './PackageForm.svelte';
 import type {EntityView} from '../../bindings';
 import type {CaseOperations} from './controller.svelte';
 let {operations,route,entity,showPackages=true}:{operations:CaseOperations;route:{kind:string;id:string}|null;entity:EntityView|null;showPackages?:boolean}=$props();
 export async function importFile(){await operations.importFile();}
</script>

<section class="actions" aria-label="Case actions">
  <div class="toolbar"><button type="button" disabled={operations.busy} onclick={operations.importFile}>Import file</button><button type="button" disabled={operations.busy} onclick={operations.batchExtract}>Extract supported documents</button><button type="button" disabled={operations.busy||!["artifact","document"].includes(route?.kind??"")} onclick={operations.extract}>Extract selected document</button><button type="button" aria-pressed={operations.online} onclick={operations.toggleNetwork}>{operations.online?"Online — switch offline":"Offline — enable collection"}</button></div>
  {#if operations.batchRunning}<button type="button" disabled={operations.stopBatch} onclick={()=>operations.stopBatch=true}>{operations.stopBatch?"Stopping after current document…":"Stop after current document"}</button>{/if}
  {#if operations.busy}<p role="status">{operations.operation?.stage??"Preparing operation…"} {#if operations.operation?.cancellable}<button type="button" onclick={operations.cancel}>Cancel operation</button>{/if}</p>{/if}
  {#if operations.result}<p role="status">Operation finished.{#if operations.result.complete===false} Processing is partial; review evidence and recorded coverage gaps.{/if}{#if typeof operations.result.processing_error==="string"} {operations.result.processing_error}{/if}{#if typeof operations.result.size_bytes==="number"} Imported {operations.result.size_bytes} bytes{operations.result.deduplicated?"; existing evidence bytes reused":""}. Extraction remains a separate action.{/if}{#if typeof operations.result.extracted==="number"} Extracted {operations.result.extracted} documents.{/if}{#if operations.result.stopped_before_remaining} Stopped before the remaining documents.{/if}{#if Array.isArray(operations.result.failed_artifacts) && operations.result.failed_artifacts.length} {operations.result.failed_artifacts.length} documents failed; review their details.{/if}{#if typeof operations.result.unsupported_documents==="number" && operations.result.unsupported_documents>0} {operations.result.unsupported_documents} documents use unsupported formats.{/if}</p><details><summary>Operation details</summary><pre>{JSON.stringify(operations.result,null,2)}</pre></details>{/if}
  <div class="flows">
    <details><summary>Collect a public page</summary>
      <CollectionForm bind:url={operations.url} bind:confirmed={operations.confirmed} busy={operations.busy} online={operations.online} onsubmit={operations.collect}/>
    </details>
    {#if import.meta.env.DEV||import.meta.env.VITE_DESKTOP_TEST==='1'}<details><summary>Record analysis</summary>
      <AnalysisForm {operations} {route} {entity} legacy={true} oninspect={()=>{}}/>
    </details>
    {/if}{#if showPackages}<details><summary>Export, verify, or restore</summary>
      <p>Packages have no recovery key. Losing the export passphrase loses access. Verification detects damage; it is not a digital signature. Exclusions remain recorded as missing evidence.</p>
      <PackageForm bind:passphrase={operations.passphrase} bind:exclusions={operations.exclusions} busy={operations.busy} onsubmit={operations.exportCase} onverify={operations.verify} onrestore={operations.restore}/>
    </details>{/if}
  </div>
</section>
<style>
 .actions{padding:16px;margin-bottom:24px;border:1px solid var(--border);border-radius:var(--radius-panel);background:var(--surface-panel)}.toolbar{display:flex;align-items:center;gap:12px;flex-wrap:wrap}.flows{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));align-items:start;gap:16px;margin-top:20px}details{min-width:0;border:1px solid var(--border);border-radius:var(--radius-control);padding:16px;background:var(--surface-raised)}summary{cursor:pointer;font-weight:600}pre{white-space:pre-wrap;overflow-wrap:anywhere}p{max-width:75ch;color:var(--foreground-secondary)}@media(max-width:1200px){.flows{grid-template-columns:1fr}}
</style>
