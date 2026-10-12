<script lang="ts">
  import SettingsDialog from './features/settings/SettingsDialog.svelte';
  import ResizeHandle from './design/components/ResizeHandle.svelte';
  import {preferences} from './stores/preferences.svelte';
  import LibraryView from './features/workbench/LibraryView.svelte';
  import SubjectDetail from './features/workbench/SubjectDetail.svelte';
  import ArtifactDetail from './features/workbench/ArtifactDetail.svelte';
  import { onMount } from "svelte";
  import * as api from "./bindings";
  import EntityDossier from "./features/entities/EntityDossier.svelte";
  import ActivityExcerpt from './features/entities/ActivityExcerpt.svelte';
  import CaseContext from './layout/CaseContext.svelte';
  import Icon, { type IconName } from './design/components/Icon.svelte';
  import appIcon from './design/assets/kokin-icon.png';
  import Observation from "./Observation.svelte";
  import DocumentPanel from "./DocumentPanel.svelte";
  import GraphPanel from "./GraphPanel.svelte";
  import Actions from "./Actions.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  let {generation, caseView, onclose, caseBusy=false}:{generation:number;caseView?:api.CaseView;onclose?:()=>void;caseBusy?:boolean}=$props();
  let online=$state(false);
  let collapsed=$state(preferences.value.sidebarCollapsed);
  let showSettings=$state(false);
  let showInspector=$state(true);
  let inspectorWidth=$state(preferences.value.inspectorWidth);
  $effect(()=>{collapsed=preferences.value.sidebarCollapsed;inspectorWidth=preferences.value.inspectorWidth;});
  function hideInspector(){showInspector=false;queueMicrotask(()=>document.getElementById('toggle-inspector')?.focus());}
  function commitWidth(width:number){preferences.update({inspectorWidth:width});}
  async function setNetwork(allowed:boolean){online=await api.setNetworkMode(allowed);}
  let showActions=$state(true);
  let showLibrary=$state(true);
  let evidenceTotal=$state<number|null>(null);
  let actions=$state<{importFile:()=>Promise<void>}|undefined>();
  let source=$state<api.ObservationView|null>(null);
  let sourceError=$state('');
  const icons:Record<string,IconName>={Evidence:'document-text',Entities:'user',Search:'magnifying-glass',Activity:'clock'};
  type Tab = "Evidence" | "Entities" | "Search" | "Activity";
  type Route = {kind:string; id:string};
  let evidenceId = $state("");
  let tab = $state<Tab>("Evidence");
  let offset = $state(0);
  let total = $state(0);
  let artifacts = $state<api.ArtifactRowView[]>([]);
  let entities = $state<api.EntityRowView[]>([]);
  let search = $state<api.SearchView | null>(null);
  let activity = $state<api.ActivityView | null>(null);
  let query = $state("");
  let submittedQuery = "";
  let kindFilter = $state("");
  let submittedKind = "";
  let facetFilter=$state("");
  let submittedFacet="";
  let includeSuperseded = $state(false);
  let submittedSuperseded = false;
  let resolveMerged = $state(true);
  let submittedResolved = true;
  let error = $state<api.CommandError | null>(null);
  let busy = $state(false);
  let detailBusy = $state(false);
  let route = $state<Route | null>(null);
  let history = $state<Route[]>([]);
  let related = $state<api.PageView<api.SubjectView>|null>(null);
  let entity = $state<api.EntityView | null>(null);
  let observation = $state<api.ObservationView | null>(null);
  let subject = $state<api.SubjectView | null>(null);
  let doc = $state<api.DocumentView | null>(null);
  let table = $state<api.EvidenceListView | null>(null);
  let observations = $state<api.PageView<api.ObservationRowView> | null>(null);
  let observationOffset = $state(0);
  let showWithdrawn = $state(false);
  let live = true;
  let listVersion = 0;
  let pivotVersion = 0;
  let detailVersion = 0;
  let summaryVersion = 0;

  onMount(() => { void refresh(); return () => {live=false; listVersion++; detailVersion++; pivotVersion++;summaryVersion++;}; });
  async function refreshSummary(){const version=++summaryVersion;evidenceTotal=null;try{const page=await api.artifactPage();if(live && version===summaryVersion)evidenceTotal=page.total;}catch{if(live && version===summaryVersion)evidenceTotal=null;}}
  function changed(){void refresh();if(tab!=='Evidence')void refreshSummary();if(route)void open(route,false);}
  function importEvidence(){showActions=true;void actions?.importFile();}

  async function refresh() {
    const version = ++listVersion;
    busy=true; error=null;
    try {
      const result = tab === "Evidence" ? await api.artifactPage(offset)
        : tab === "Entities" ? await api.entityPage(offset)
        : tab === "Activity" ? await api.activityPage(offset)
        : await api.searchCase({query:submittedQuery, offset, limit:50, kinds:submittedKind?[submittedKind]:[], facets:submittedFacet?[submittedFacet]:[], include_superseded:submittedSuperseded, resolve_merged:submittedResolved});
      if (!live || version!==listVersion) return;
      if (tab==="Evidence") { const page=result as api.PageView<api.ArtifactRowView>; artifacts=page.rows; total=page.total; evidenceTotal=page.total; }
      else if (tab==="Entities") {const page=result as api.PageView<api.EntityRowView>; entities=page.rows; total=page.total;}
      else if (tab==="Activity") {activity=result as api.ActivityView; total=activity.page.total;}
      else {search=result as api.SearchView; total=search.total;}
    } catch (raw) {if(live && version===listVersion) error=api.asCommandError(raw);}
    finally {if(live && version===listVersion) busy=false;}
  }
  function navigate(next:Tab) {tab=next;offset=0;showLibrary=true;void refresh();}
  function submitSearch(event:SubmitEvent) {
    event.preventDefault(); submittedQuery=query;submittedKind=kindFilter;submittedFacet=facetFilter;submittedSuperseded=includeSuperseded;submittedResolved=resolveMerged;offset=0;void refresh();
  }
  function page(delta:number) {offset=Math.max(0,offset+delta);void refresh();}
  async function open(next:Route, push=true) {
    if(next.kind==='entity')showLibrary=false;
    if(push && route) history=[...history,route];
    pivotVersion++;route=next;related=null;entity=null;observation=null;subject=null;doc=null;table=null;observations=null;source=null;sourceError='';
    observationOffset=0;showWithdrawn=false;
    const version=++detailVersion;detailBusy=true;error=null;
    try {
      if(next.kind==="entity") {
        const [result,edges]=await Promise.all([api.entity(next.id),api.relationships(next.id)]);
        if(live && version===detailVersion) {
          entity=result;related=edges;
          const citation=result.evidence.find(link=>link.evidence_kind==='observation' && link.role==='supports') ?? result.evidence.find(link=>link.evidence_kind==='observation');
          if(citation) {
            try { const preview=await api.observation(citation.evidence_id);if(live && version===detailVersion)source=preview; }
            catch(raw){if(live && version===detailVersion)sourceError=`Source preview unavailable: ${api.asCommandError(raw).message}`;}
          }
        }
      }
      else if(next.kind==="observation") {const result=await api.observation(next.id);if(live && version===detailVersion) observation=result;}
      else if(next.kind==="artifact") {
        const [result,page]=await api.observationPage(next.id);
        if(live && version===detailVersion) {table=result;observations=page;}
      } else if(next.kind==="document") {const result=await api.artifactDocument(next.id);if(live && version===detailVersion) doc=result;}
      else {const result=await api.subject(next.kind,next.id);if(live && version===detailVersion) subject=result;}
    } catch(raw) {if(live && version===detailVersion) error=api.asCommandError(raw);}
    finally {if(live && version===detailVersion) detailBusy=false;}
  }
  function back() {const previous=history[history.length-1];history=history.slice(0,-1);if(previous) void open(previous,false);}
  function closeDetail() {pivotVersion++;route=null;history=[];detailVersion++;detailBusy=false;showLibrary=true;entity=null;related=null;source=null;sourceError='';observation=null;subject=null;doc=null;table=null;observations=null;}
  async function observationPage(delta:number) {
    if(route?.kind!=="artifact")return;
    observationOffset=Math.max(0,observationOffset+delta);
    const version=++detailVersion;const id=route.id;detailBusy=true;
    try {const [result,page]=await api.observationPage(id,showWithdrawn,observationOffset);if(live && version===detailVersion){table=result;observations=page;}}
    catch(raw){if(live && version===detailVersion)error=api.asCommandError(raw);}
    finally{if(live && version===detailVersion)detailBusy=false;}
  }
  async function evidence(kind:string,id:string) {const version=++pivotVersion;try {const [target,targetId]=await api.evidenceTarget(kind,id);if(live && version===pivotVersion)await open({kind:target,id:targetId});}catch(raw){if(live && version===pivotVersion)error=api.asCommandError(raw);}}
  async function relationshipPage(delta:number){if(!entity||!related)return;const version=detailVersion;try{const page=await api.relationships(entity.entity_id,Math.max(0,related.offset+delta));if(live && version===detailVersion)related=page;}catch(raw){if(live)error=api.asCommandError(raw);}}

</script>

<svelte:window onkeydown={(event)=>{
  if(showSettings)return;
  if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='k'){event.preventDefault();document.getElementById('case-global-query')?.focus();}
  if(event.altKey && event.key==='ArrowLeft' && history.length){event.preventDefault();back();}
  if(event.key==='Escape'){const disclosure=document.querySelector<HTMLDetailsElement>('.case');if(disclosure?.open){event.preventDefault();disclosure.open=false;disclosure.querySelector('summary')?.focus();}}
}}/>

<div class="ledger" class:collapsed style={`--inspector-width:${inspectorWidth}px`}>
<a class="skip-link" href="#case-workspace">Skip to investigation workspace</a>
<aside class="sidebar">
  <div class="sidebar-title"><img src={appIcon} alt="" width="24" height="24"/>{#if !collapsed}<strong>Kokin-OSINT</strong>{/if}</div>
  <button class="collapse" type="button" aria-label={collapsed?'Expand navigation':'Collapse navigation'} aria-expanded={!collapsed} onclick={()=>preferences.update({sidebarCollapsed:!collapsed})}><Icon name="bars-3"/>{#if !collapsed}<span>Collapse navigation</span>{/if}</button>
<nav aria-label="Case sections">
  {#each ["Evidence","Entities","Search","Activity"] as item}
    <button type="button" aria-label={item} title={collapsed?item:undefined} aria-current={tab===item?"page":undefined} onclick={()=>navigate(item as Tab)}><Icon name={icons[item]} size={22}/><span class:hidden={collapsed}>{item}</span></button>
  {/each}
</nav>
  <p class="sidebar-note" class:hidden={collapsed}>Local-first investigation<br/>Every conclusion has a source.</p>
</aside>
<div class="workspace" id="case-workspace" tabindex="-1">
<h2 class="sr-only">{tab}</h2>
<header class="global-toolbar">
  <form class="global-search" onsubmit={(event)=>{tab='Search';showLibrary=true;submitSearch(event);}}><Icon name="magnifying-glass"/><label for="case-global-query" class="sr-only">Global case search</label><input id="case-global-query" type="search" bind:value={query} placeholder="Search evidence, entities, or keywords…"/><button disabled={busy}>Search case</button><kbd title="Command or Control K">⌘ / Ctrl K</kbd></form>
  {#if caseView}<details class="case"><summary title={caseView.case_id}><Icon name="document-text"/><span class="name">{caseView.case_id}</span></summary><div class="case-information"><strong>Case open</strong><p class="path">{caseView.root}</p><p>Schema version {caseView.schema_version}</p><p>{caseView.has_recovery_key?'Recovery key available.':'No recovery key. If the passphrase is forgotten, this case cannot be opened again.'}</p></div></details>{/if}
  <button type="button" class="case-actions" aria-expanded={showActions} onclick={()=>showActions=!showActions}><Icon name="adjustments-horizontal"/>Case actions</button>
  <button type="button" id="toggle-inspector" aria-controls={entity?"entity-inspector":"case-inspector"} aria-expanded={showInspector} onclick={()=>showInspector=!showInspector}>{showInspector?"Hide inspector":"Show inspector"}</button>
  <button type="button" onclick={()=>showSettings=true}>Settings</button>
  {#if onclose}<button type="button" disabled={caseBusy} onclick={onclose}>Close case</button>{/if}
</header>
<div hidden={!showActions}><Actions bind:this={actions} {route} {entity} {generation} bind:online bind:evidenceId onchange={changed}/></div>
{#if error}<ErrorNotice {error}/>{/if}
<div class="content-grid" class:entity-layout={entity!==null} class:inspector-hidden={!showInspector}>
<div class:hasdetail={route!==null} class="workbench" class:entity-view={entity!==null} class:library-hidden={route!==null && !showLibrary}>
  <section class="list" aria-label={tab} hidden={route!==null && !showLibrary}>
    <LibraryView {tab} {busy} {total} {offset} {artifacts} {entities} {search} {activity} bind:query bind:kindFilter bind:facetFilter bind:includeSuperseded bind:resolveMerged onsubmit={submitSearch} onopen={(next)=>void open(next)} onpage={page}/>
  </section>
  {#if route}
    <section class="detail" aria-label="Evidence and analysis detail">
      <div class="toolbar"><button type="button" disabled={history.length===0} onclick={back}>Back</button><button type="button" onclick={closeDetail}>Close detail</button><button type="button" aria-expanded={showLibrary} onclick={()=>showLibrary=!showLibrary}>{showLibrary?'Hide library':'Show library'}</button></div>
      {#if detailBusy}<p role="status">Loading detail…</p>{/if}
      {#if entity}
        <EntityDossier oncollapse={hideInspector} {showInspector} bind:inspectorWidth onwidth={commitWidth} {entity} {source} {sourceError} relationships={related} onevidence={evidence} onopen={(kind,id)=>void open({kind,id})} onpage={relationshipPage}>
          {#snippet context()}<CaseContext {caseView} {online} {evidenceTotal} onimport={importEvidence}/>{/snippet}
          {#snippet activity()}<ActivityExcerpt onopen={(kind,id)=>void open({kind,id})} onhistory={()=>navigate('Activity')}/>{/snippet}
        </EntityDossier>
        {#key entity.entity_id}<GraphPanel seed={entity.entity_id} {generation} onopen={(kind,id)=>void open({kind,id})}/>{/key}

      {:else if observation}<button type="button" onclick={()=>evidenceId=observation!.observation_id}>Use as evidence</button><Observation {observation}/><button type="button" onclick={()=>open({kind:"document",id:observation!.lineage.artifact.artifact_id})}>Open source document</button>
      {:else if doc}<DocumentPanel {doc}/>
      {:else if subject}
        <SubjectDetail {subject} onopen={(kind,id)=>void open({kind,id})} onevidence={evidence}/>
      {:else if table && observations}
        <ArtifactDetail {table} {observations} {detailBusy} bind:observationOffset bind:showWithdrawn onpage={observationPage} onopen={(kind,id)=>void open({kind,id})}/>
      {/if}
    </section>
  {/if}
</div>
{#if !entity && showInspector}<div class="resize-slot"><ResizeHandle controls="case-inspector" bind:value={inspectorWidth} oncollapse={hideInspector} oncommit={commitWidth}/></div>{/if}
{#if !entity}<aside id="case-inspector" hidden={!showInspector} class="case-context" aria-label="Case context"><CaseContext {caseView} {online} {evidenceTotal} onimport={importEvidence}/></aside>{/if}
</div>
</div>
</div>

{#if showSettings}<SettingsDialog casePath={caseView?.root} {online} onnetwork={setNetwork} onclose={()=>showSettings=false}/>{/if}

<style>
  .ledger { display: grid; grid-template-columns: 178px minmax(0, 1fr); min-height: calc(100vh - 150px); transition:grid-template-columns var(--motion-layout) var(--motion-ease); }
  .ledger.collapsed { grid-template-columns: 68px minmax(0, 1fr); }
  .sidebar { border-right: 1px solid var(--border); padding: 20px 12px; }
  .sidebar-title { display: flex; align-items: center; gap: 10px; margin: 4px 0 28px; white-space: nowrap; }
  nav { display: grid; gap: 8px; margin: 16px 0; }
  nav button, .collapse { display: flex; align-items: center; gap: 14px; text-align: left; background: transparent; border-color: transparent; padding: 12px; }
  .collapse { width: 100%; font-size: 11px; color: var(--foreground-muted); }
  nav [aria-current="page"] { color: var(--accent); background: var(--accent-surface); border-color: #164573; }
  .hidden { display: none; }
  .sidebar-note { font-size: 11px; color: var(--foreground-muted); margin: 48px 12px; }
  .workspace { padding: 24px; min-width: 0; }
  .global-toolbar { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin-bottom: 24px; }
  .global-search { display: flex; flex: 1; min-width: 250px; align-items: center; gap: 12px; border: 1px solid var(--border); padding: 8px 12px; border-radius: var(--radius-panel); margin: 0; }
  .global-search input { flex: 1; border: 0; background: transparent; padding: 8px; width: 0; }
  kbd { color: var(--foreground-muted); font-family: inherit; font-size: 11px; white-space: nowrap; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .case-actions, .case summary { display: flex; align-items: center; gap: 8px; }
  .case { position: relative; min-width: 0; }
  .case summary { cursor: pointer; border: 1px solid var(--border); border-radius: var(--radius-control); padding: 10px 12px; }
  .case .name { max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .case[open] .case-information{animation:ledger-enter var(--motion-disclosure) var(--motion-ease);}
  .case-information { position: absolute; right: 0; top: calc(100% + 8px); width: min(360px, 75vw); border: 1px solid var(--border-strong); border-radius: var(--radius-panel); background: var(--surface-raised); padding: 20px; z-index: var(--layer-overlay); overflow-wrap: anywhere; box-shadow: 0 16px 32px #0008; }
  .path { color: var(--foreground-secondary); font-size: 12px; }
  .content-grid { display: grid; grid-template-columns: minmax(0, 1fr) 12px var(--inspector-width); gap: 12px; align-items: start; }
  .resize-slot{display:flex;align-self:stretch;}
  .case-context[hidden]{display:none;}
  .content-grid.inspector-hidden{grid-template-columns:1fr;}
  .skip-link{position:absolute;left:12px;top:12px;transform:translateY(-150%);z-index:30;padding:12px;background:var(--surface-raised);color:var(--foreground);border:1px solid var(--border-strong);}
  .skip-link:focus{transform:none;}
  .content-grid.entity-layout { grid-template-columns: 1fr; }
  .workbench { display: grid; gap: 24px; min-width: 0; }
  .hasdetail { grid-template-columns: minmax(180px, 1fr) minmax(0, 2fr); }
  .hasdetail.entity-view { grid-template-columns: 210px minmax(0, 1fr); }
  .hasdetail.library-hidden { grid-template-columns: 1fr; }
  .list[hidden] { display: none; }
  .list { border: 1px solid var(--border); border-radius: var(--radius-panel); padding: 20px; min-width: 0; background: var(--surface-panel); }
  .detail { min-width: 0; animation:ledger-enter var(--motion-disclosure) var(--motion-ease); }
  .toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .toolbar { margin-bottom: 20px; }
  form { display: grid; gap: 12px; margin-bottom: 16px; }
  @media (max-width: 1240px) { .content-grid { grid-template-columns: 1fr; } .case-context,.resize-slot { display: none; } .hasdetail.entity-view { grid-template-columns: 170px minmax(0, 1fr); } }
  @media (max-width: 1020px) { .ledger { grid-template-columns: 150px minmax(0, 1fr); } .workspace { padding: 16px; } .hasdetail, .hasdetail.entity-view { grid-template-columns: 1fr; } .list { max-height: 320px; overflow: auto; } }
</style>
