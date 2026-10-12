<script lang="ts">
  import CollectionForm from './CollectionForm.svelte';
  import PackageForm from './PackageForm.svelte';
  import { onMount } from "svelte";
  import * as api from "../../bindings";
  import ErrorNotice from "../../ErrorNotice.svelte";
  let {route, entity, generation, onchange, evidenceId=$bindable(""), online=$bindable(false)}:{route:{kind:string;id:string}|null;entity:api.EntityView|null;generation:number;onchange:()=>void;evidenceId?:string;online?:boolean}=$props();
  let action=$state("create_entity");
  let error=$state<api.CommandError|null>(null);
  let busy=$state(false);
  let batchRunning=$state(false);
  let stopBatch=$state(false);
  let operation=$state<api.OperationView|null>(null);
  let result=$state<Record<string,unknown>|null>(null);
  let url=$state("");
  let confirmed=$state(false);
  let name=$state("");
  let notes=$state("");
  let type=$state("person");
  let types=$state<Array<[string,string]>>([]);
  let entityChoices=$state<Array<{id:string;name:string}>>([]);
  let entityQuery=$state("");
  let left=$state("");
  let right=$state("");
  let namespace=$state("email_address");
  let value=$state("");
  let relation=$state("associated_with");
  let reason=$state("");
  let dimension=$state("");
  let assessedValue=$state("");
  let scales=$state<api.ScaleOptionView[]>([]);
  let citations=$state<api.GroundingInput[]>([]);
  let role=$state("supports");
  let evidenceKind=$state("observation");
  let passphrase=$state("");
  let exclusions=$state("");
  let mergeConfirmed=$state(false);
  let preview=$state<[api.EntityView,api.EntityView]|null>(null);
  $effect(()=>{left;right;action;mergeConfirmed=false;preview=null;});
  async function previewMerge(){try{const ids=[left,right];const result=await Promise.all([api.entity(left),api.entity(right)]);if(live && left===ids[0] && right===ids[1])preview=result;}catch(raw){error=api.asCommandError(raw);}}

  let live=true;
  let choiceVersion=0;
  let timer:ReturnType<typeof setTimeout>|undefined;
  onMount(()=>{void loadOptions();return()=>{live=false;clearTimeout(timer);};});
  async function loadOptions(){try{const [network,registry,options]=await Promise.all([api.networkMode(),api.entityTypes(),api.scaleOptions()]);if(!live)return;online=network;types=registry;scales=options;dimension=options[0]?.dimension??"";await findEntities();}catch(raw){if(live)error=api.asCommandError(raw);}}
  async function findEntities(){const version=++choiceVersion;try{const names=entityQuery.trim()? (await api.searchCase({query:entityQuery,kinds:["entity"],limit:50})).hits.map(h=>({id:h.canonical_id??h.subject_id,name:h.title})): (await api.entityPage()).rows.map(e=>({id:e.entity_id,name:e.display_name}));if(live && version===choiceVersion)entityChoices=names;}catch(raw){if(live)error=api.asCommandError(raw);}}
  async function toggleNetwork(){try{online=await api.setNetworkMode(!online);}catch(raw){error=api.asCommandError(raw);}}
  function addCitation(){if(!evidenceId.trim())return;citations=[...citations,{kind:evidenceKind,id:evidenceId.trim(),role}];evidenceId="";}
  function grounding(){return citations.length?citations:evidenceId.trim()?[{kind:evidenceKind,id:evidenceId.trim(),role}]:[];}
  async function run(kind:string,args:unknown){busy=true;error=null;result=null;try{const id=await api.startOperation(kind,args,generation);if(!live)return;await poll(id);}catch(raw){if(live){error=api.asCommandError(raw);busy=false;}}}
  async function poll(id:string){try{const view=await api.operationStatus(id);if(!live)return;operation=view;if(view.status==="running"){timer=setTimeout(()=>void poll(id),300);return;}busy=false;if(view.status==="completed"){result=JSON.parse(view.result_json??"{}");passphrase="";mergeConfirmed=false;citations=[];onchange();void findEntities();}else error=view.error;}catch(raw){if(live){error=api.asCommandError(raw);busy=false;}}}
  async function cancel(){if(!operation)return;try{await api.cancelOperation(operation.operation_id);}catch(raw){error=api.asCommandError(raw);}}
  export async function importFile(){if(busy)return;try{const selection=await api.selectPath("import",generation);if(selection)await run("import",{selection});}catch(raw){error=api.asCommandError(raw);}}
  async function collect(event:SubmitEvent){event.preventDefault();if(confirmed && online)await run("collect",{url});}
  async function extract(){if(route?.kind==="artifact"||route?.kind==="document")await run("extract",{artifact_id:route.id});}
  async function batchExtract(){busy=true;batchRunning=true;stopBatch=false;error=null;try{let offset=0;let count=0;let unsupported=0;let failures:string[]=[];while(live && !stopBatch){const page=await api.artifactPage(offset);for(const row of page.rows){if(!live)return;if(stopBatch)break;if(!["text/html","application/xhtml+xml"].includes(row.media_type)){unsupported++;continue;}if(row.extraction==="complete")continue;const id=await api.startOperation("extract",{artifact_id:row.artifact_id},generation);let view=await api.operationStatus(id);while(live && view.status==="running"){operation=view;await new Promise(r=>setTimeout(r,300));view=await api.operationStatus(id);}if(!live)return;operation=view;if(view.status!=="completed")failures.push(row.artifact_id);else count++;}offset+=page.limit;if(offset>=page.total)break;}result={extracted:count,failed_artifacts:failures,unsupported_documents:unsupported,stopped_before_remaining:stopBatch};onchange();}catch(raw){error=api.asCommandError(raw);}finally{if(live){busy=false;batchRunning=false;}}}
  async function write(event:SubmitEvent){event.preventDefault();const evidence=grounding();let args:unknown;
    if(action==="create_entity")args={type_key:type,display_name:name,notes,evidence};
    else if(action==="add_identifier")args={entity_id:left,namespace,value,evidence};
    else if(action==="relate")args={from_entity:left,to_entity:right,kind:relation,evidence};
    else if(action==="assess")args={subject_kind:route?.kind,subject_id:route?.id,dimension,value_key:assessedValue,contributing_factors:reason.split("\n").filter(s=>s.trim())};
    else if(action==="split")args={entity:left,rationale:reason};
    else {if(action==="merge"&&(!mergeConfirmed||!preview))return;args={left,right,rationale:reason};}
    await run(action,args);
  }
  async function exportCase(event:SubmitEvent){event.preventDefault();try{const destination=await api.selectPath("export",generation);if(destination)await run("export",{destination,passphrase,exclude:exclusions.split(/\s+/).filter(Boolean)});}catch(raw){error=api.asCommandError(raw);}}
  async function verify(){try{const selection=await api.selectPath("package",generation);if(selection)await run("verify",{selection});}catch(raw){error=api.asCommandError(raw);}}
  async function restore(){try{const selection=await api.selectPath("package",generation);if(!selection)return;const destination=await api.selectPath("restore",generation);if(destination)await run("restore",{selection,destination,passphrase});}catch(raw){error=api.asCommandError(raw);}}
</script>

<section class="actions" aria-label="Case actions">
  <div class="toolbar"><button type="button" disabled={busy} onclick={importFile}>Import file</button><button type="button" disabled={busy} onclick={batchExtract}>Extract unread HTML</button><button type="button" disabled={busy||!["artifact","document"].includes(route?.kind??"")} onclick={extract}>Extract selected document</button><button type="button" aria-pressed={online} onclick={toggleNetwork}>{online?"Online — switch offline":"Offline — enable collection"}</button></div>
  {#if error}<ErrorNotice {error}/>{/if}
  {#if batchRunning}<button type="button" disabled={stopBatch} onclick={()=>stopBatch=true}>{stopBatch?"Stopping after current document…":"Stop after current document"}</button>{/if}
  {#if busy}<p role="status">{operation?.stage??"Preparing operation…"} {#if operation?.cancellable}<button type="button" onclick={cancel}>Cancel operation</button>{/if}</p>{/if}
  {#if result}<p role="status">Operation complete.{#if typeof result.size_bytes==="number"} Imported {result.size_bytes} bytes{result.deduplicated?"; existing evidence bytes reused":""}. Extraction remains a separate action.{/if}{#if typeof result.extracted==="number"} Extracted {result.extracted} documents.{/if}{#if result.stopped_before_remaining} Stopped before the remaining documents.{/if}{#if Array.isArray(result.failed_artifacts) && result.failed_artifacts.length} {result.failed_artifacts.length} documents failed; review their details.{/if}{#if typeof result.unsupported_documents==="number" && result.unsupported_documents>0} {result.unsupported_documents} documents use unsupported formats.{/if}</p><details><summary>Operation details</summary><pre>{JSON.stringify(result,null,2)}</pre></details>{/if}
  <div class="flows">
    <details><summary>Collect a public page</summary>
      <CollectionForm bind:url bind:confirmed {busy} {online} onsubmit={collect}/>
    </details>
    <details><summary>Record analysis</summary>
      <form onsubmit={write}>
        <label>Action<select bind:value={action} onchange={()=>mergeConfirmed=false}><option value="create_entity">Create entity</option><option value="add_identifier">Add identifier</option><option value="relate">Create relationship</option><option value="assess">Assess selected subject</option><option value="merge">Merge entities</option><option value="reject">Reject a merge</option><option value="split">Split a merged record</option></select></label>
        {#if action==="create_entity"}<label>Name<input required bind:value={name}/></label><label>Type<select bind:value={type}>{#each types as [key,label]}<option value={key}>{label}</option>{/each}</select></label><label>Notes<textarea bind:value={notes}></textarea></label>{/if}
        {#if ["add_identifier","relate","merge","reject","split"].includes(action)}
          <label>Find entities<input bind:value={entityQuery} placeholder="Search names to narrow the choices"/></label><button type="button" onclick={findEntities}>Find entities</button><p>Showing up to 50 matches; refine the name to find another entity.</p>
          <label>{action==="split"?"Merged record to separate":"Entity"}<select required bind:value={left}><option value="">Choose an entity</option>{#each (action==="split" && entity ? entity.merged_from.map(e=>({id:e.entity_id,name:e.display_name})):entityChoices) as e}<option value={e.id}>{e.name}</option>{/each}</select></label>
          {#if action==="split" && route?.kind==="entity"}<p>Open the canonical entity first, then choose one of its merged records above.</p>{/if}
          {#if ["relate","merge","reject"].includes(action)}<label>Other entity<select required bind:value={right}><option value="">Choose another entity</option>{#each entityChoices as e}<option value={e.id}>{e.name}</option>{/each}</select></label>{/if}
        {/if}
        {#if action==="add_identifier"}<label>Identifier kind<input required bind:value={namespace}/></label><label>Value<input required bind:value={value}/></label>{/if}
        {#if action==="relate"}<label>Relationship<input required bind:value={relation}/></label>{/if}
        {#if ["create_entity","add_identifier","relate"].includes(action)}
          <fieldset><legend>Evidence citations</legend><p>Open an observation and choose “Use as evidence”, or enter a stored evidence reference.</p><label>Evidence kind<select bind:value={evidenceKind}><option value="observation">Observation</option><option value="artifact">Document</option><option value="capture">Capture</option></select></label><label>Evidence reference<input bind:value={evidenceId}/></label><label>Role<select bind:value={role}><option value="supports">Supports</option><option value="contradicts">Contradicts</option><option value="context">Context</option></select></label><button type="button" onclick={addCitation}>Add citation</button>{#each citations as citation,i}<p>{citation.role}: {citation.kind} {citation.id} <button type="button" onclick={()=>citations=citations.filter((_,j)=>i!==j)}>Remove</button></p>{/each}</fieldset>
        {/if}
        {#if action==="assess"}
          <p>Assessing {route?.kind??"nothing selected"}. Open an entity, identifier, or relationship first.</p><label>Dimension<select bind:value={dimension} onchange={()=>assessedValue=""}>{#each scales as scale}<option value={scale.dimension}>{scale.title}</option>{/each}</select></label><label>Judgment<select required bind:value={assessedValue}><option value="">Choose a judgment</option>{#each scales.find(s=>s.dimension===dimension)?.values??[] as [key,label]}<option value={key}>{label}</option>{/each}</select></label>
        {/if}
        {#if ["assess","merge","reject","split"].includes(action)}<label>Reason / contributing factors<textarea required bind:value={reason}></textarea></label>{/if}
        {#if action==="merge"}<button type="button" disabled={!left||!right||left===right||busy} onclick={previewMerge}>Preview merge</button><aside>
          {#if preview}{#each preview as candidate}<p><strong>{candidate.display_name}</strong>: {candidate.merged_from.length+1} records, {candidate.evidence.length} evidence links, {candidate.identifiers.length} identifiers. Review its evidence and individual confidence dimensions before merging.</p>{/each}{/if}<p>Merging combines the two clusters in lists and search. Evidence and conflicting assessments remain on their original records. The older cluster is canonical; a split can separate a record later.</p><label class="check"><input type="checkbox" bind:checked={mergeConfirmed}/>I reviewed the two entities and want to merge them</label></aside>{/if}
        <button disabled={busy||(action==="merge"&&(!mergeConfirmed||!preview))||(action==="assess"&&!["entity","identifier","relationship"].includes(route?.kind??""))}>Record decision</button>
      </form>
    </details>
    <details><summary>Export, verify, or restore</summary>
      <p>Packages have no recovery key. Losing the export passphrase loses access. Verification detects damage; it is not a digital signature. Exclusions remain recorded as missing evidence.</p>
      <PackageForm bind:passphrase bind:exclusions {busy} onsubmit={exportCase} onverify={verify} onrestore={restore}/>
    </details>
  </div>
</section>
<style>
  .actions{animation:ledger-enter var(--motion-disclosure) var(--motion-ease);padding:16px;margin-bottom:24px;border:1px solid var(--border);border-radius:var(--radius-panel);background:var(--surface-panel);border-bottom:1px solid var(--border);}
  .toolbar,.check{display:flex;align-items:center;gap:.75rem;flex-wrap:wrap;}
  .flows{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));align-items:start;gap:16px;margin-top:20px;}
  details{min-width:0;border:1px solid var(--border);border-radius:var(--radius-control);padding:16px;background:var(--surface-raised);}
  summary{cursor:pointer;color:var(--foreground);font-weight:600;padding:4px 0;}
  form{display:grid;gap:.75rem;margin:.75rem 0;}
  label{display:grid;gap:.3rem;}
  input,select,textarea{font:inherit;color:var(--foreground);background:var(--surface-canvas);border:1px solid var(--border-strong);padding:.5rem;min-width:0;}
  textarea{min-height:4rem;resize:vertical;}
  p{color:var(--foreground-secondary);max-width:65ch;}
  fieldset,aside{border:1px solid var(--border);border-radius:var(--radius-control);padding:16px;}legend{padding:0 8px;color:var(--foreground-secondary);}
  @media(max-width:1200px){.flows{grid-template-columns:1fr;}}
  pre{white-space:pre-wrap;overflow-wrap:anywhere;max-width:70ch;}
</style>
