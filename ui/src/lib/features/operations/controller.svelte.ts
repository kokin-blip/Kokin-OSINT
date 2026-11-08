import {onMount} from 'svelte';
import * as api from '../../bindings';
export interface OperationContext {generation:number;route:()=>{kind:string;id:string}|null;onchange:()=>void;oncomplete?:(kind:string)=>void;}
/** One controller per open-case Workbench; views borrow it without owning jobs. */
export function createCaseOperations(context:OperationContext){
  let evidenceId=$state('');let online=$state(false);let activeJob='';let activeKind='';
  let target=$state<{kind:string;id:string}|null>(null);let targetEntity=$state<api.EntityView|null>(null);let targetLabel=$state('');
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
  let autoTimer:ReturnType<typeof setTimeout>|undefined;
  let pendingFaces:string[]=[];
  async function drainFaces(){if(!live)return;if(busy){autoTimer=setTimeout(()=>void drainFaces(),300);return;}const artifact=pendingFaces.shift();if(artifact)await run('face_analyze',{version:1,artifact_ids:[artifact],page:0,start_ms:0,end_ms:null});if(pendingFaces.length)autoTimer=setTimeout(()=>void drainFaces(),300);}
  let timer:ReturnType<typeof setTimeout>|undefined;
  onMount(()=>{void loadOptions();return()=>{live=false;clearTimeout(timer);clearTimeout(autoTimer);pendingFaces=[];if(activeJob)void api.cancelOperation(activeJob).catch(()=>{});passphrase="";citations=[];evidenceId="";};});
  async function loadOptions(){try{const [network,registry,options]=await Promise.all([api.networkMode(),api.entityTypes(),api.scaleOptions()]);if(!live)return;online=network;types=registry;scales=options;dimension=options[0]?.dimension??"";await findEntities();}catch(raw){if(live)error=api.asCommandError(raw);}}
  async function findEntities(){const version=++choiceVersion;try{const names=entityQuery.trim()? (await api.searchCase({query:entityQuery,kinds:["entity"],limit:50})).hits.map(h=>({id:h.canonical_id??h.subject_id,name:h.title})): (await api.entityPage()).rows.map(e=>({id:e.entity_id,name:e.display_name}));if(live && version===choiceVersion)entityChoices=names;}catch(raw){if(live)error=api.asCommandError(raw);}}
  async function toggleNetwork(){try{online=await api.setNetworkMode(!online);}catch(raw){error=api.asCommandError(raw);}}
  function addCitation(){if(!evidenceId.trim())return;citations=[...citations,{kind:evidenceKind,id:evidenceId.trim(),role}];evidenceId="";}
  function grounding(){return citations.length?citations:evidenceId.trim()?[{kind:evidenceKind,id:evidenceId.trim(),role}]:[];}
  async function run(kind:string,args:unknown){if(!live||busy)return;busy=true;operation=null;error=null;result=null;try{const id=await api.startOperation(kind,args,context.generation);if(!live){await api.cancelOperation(id);return;}activeJob=id;activeKind=kind;await poll(id);}catch(raw){if(live){error=api.asCommandError(raw);busy=false;}}}
  async function poll(id:string){try{const view=await api.operationStatus(id);if(!live)return;operation=view;if(view.status==="running"){timer=setTimeout(()=>void poll(id),300);return;}busy=false;activeJob='';if(view.status==="completed"||view.status==="partial"){result=JSON.parse(view.result_json??"{}");passphrase="";mergeConfirmed=false;citations=[];context.onchange();if(view.status==="completed")context.oncomplete?.(activeKind);void findEntities();
        if(activeKind==='import'&&typeof result?.artifact_id==='string'){
          const imported=result.artifact_id;
          void (async()=>{try{const enabled=await api.faceStatus(context.generation);if(!live||!enabled.auto_analyze)return;const doc=await api.artifactDocument(imported);if(!live)return;if(['application/pdf','image/jpeg','image/png','image/webp','image/tiff','video/mp4','video/quicktime','video/webm','video/x-matroska','video/x-msvideo'].includes(doc.media_type)){pendingFaces.push(imported);void drainFaces();}}catch(raw){if(live)error=api.asCommandError(raw);}})();
        }}else error=view.error;}catch(raw){if(live){error=api.asCommandError(raw);busy=false;}}}
  async function cancel(){if(!operation)return;try{await api.cancelOperation(operation.operation_id);}catch(raw){error=api.asCommandError(raw);}}
  async function importFile(){if(busy)return;try{const selection=await api.selectPath("import",context.generation);if(selection)await run("import",{selection});}catch(raw){error=api.asCommandError(raw);}}
  async function collect(event:SubmitEvent){event.preventDefault();if(confirmed && online)await run("collect",{url});}
  async function extract(){if(context.route()?.kind==="artifact"||context.route()?.kind==="document")await run("extract",{artifact_id:context.route()!.id});}
  async function batchExtract(){busy=true;batchRunning=true;stopBatch=false;error=null;try{let offset=0;let count=0;let unsupported=0;let failures:string[]=[];while(live && !stopBatch){const page=await api.artifactPage(offset);for(const row of page.rows){if(!live)return;if(stopBatch)break;if(!["text/html","application/xhtml+xml","application/json","application/dns-json","application/rdap+json","application/pdf","image/jpeg","image/png","image/webp","image/tiff"].includes(row.media_type)){unsupported++;continue;}if(row.extraction==="complete")continue;const id=await api.startOperation("extract",{artifact_id:row.artifact_id},context.generation);if(!live){await api.cancelOperation(id);return;}activeJob=id;let view=await api.operationStatus(id);while(live && view.status==="running"){operation=view;await new Promise(r=>setTimeout(r,300));if(!live)return;view=await api.operationStatus(id);}if(!live)return;operation=view;activeJob='';if(view.status!=="completed")failures.push(row.artifact_id);else count++;}offset+=page.limit;if(offset>=page.total)break;}result={extracted:count,failed_artifacts:failures,unsupported_documents:unsupported,stopped_before_remaining:stopBatch};context.onchange();}catch(raw){error=api.asCommandError(raw);}finally{if(live){busy=false;batchRunning=false;}}}
  async function write(event:SubmitEvent){event.preventDefault();const evidence=grounding();let args:unknown;
    if(action==="add_evidence")args={subject_kind:(target??context.route())?.kind,subject_id:(target??context.route())?.id,evidence};
    else if(action==="create_entity")args={type_key:type,display_name:name,notes,evidence};
    else if(action==="add_identifier")args={entity_id:left,namespace,value,evidence};
    else if(action==="relate")args={from_entity:left,to_entity:right,kind:relation,evidence};
    else if(action==="assess")args={subject_kind:(target??context.route())?.kind,subject_id:(target??context.route())?.id,dimension,value_key:assessedValue,contributing_factors:reason.split("\n").filter(s=>s.trim())};
    else if(action==="split")args={entity:left,rationale:reason};
    else {if(action==="merge"&&(!mergeConfirmed||!preview))return;args={left,right,rationale:reason};}
    await run(action,args);
  }
  async function exportCase(event:SubmitEvent){event.preventDefault();try{const destination=await api.selectPath("export",context.generation);if(destination)await run("export",{destination,passphrase,exclude:exclusions.split(/\s+/).filter(Boolean)});}catch(raw){error=api.asCommandError(raw);}}
  async function verify(){try{const selection=await api.selectPath("package",context.generation);if(selection)await run("verify",{selection});}catch(raw){error=api.asCommandError(raw);}}
  async function restore(){try{const selection=await api.selectPath("package",context.generation);if(!selection)return;const destination=await api.selectPath("restore",context.generation);if(destination)await run("restore",{selection,destination,passphrase});}catch(raw){error=api.asCommandError(raw);}}

 return {get target(){return target;},set target(next:typeof target){target=next;},get targetEntity(){return targetEntity;},set targetEntity(next:typeof targetEntity){targetEntity=next;},get targetLabel(){return targetLabel;},set targetLabel(next:string){targetLabel=next;},get evidenceId(){return evidenceId;},set evidenceId(next:typeof evidenceId){evidenceId=next;},
get online(){return online;},set online(next:typeof online){online=next;},
get action(){return action;},set action(next:typeof action){action=next;},
get error(){return error;},set error(next:typeof error){error=next;},
get busy(){return busy;},set busy(next:typeof busy){busy=next;},
get batchRunning(){return batchRunning;},set batchRunning(next:typeof batchRunning){batchRunning=next;},
get stopBatch(){return stopBatch;},set stopBatch(next:typeof stopBatch){stopBatch=next;},
get operation(){return operation;},set operation(next:typeof operation){operation=next;},
get result(){return result;},set result(next:typeof result){result=next;},
get url(){return url;},set url(next:typeof url){url=next;},
get confirmed(){return confirmed;},set confirmed(next:typeof confirmed){confirmed=next;},
get name(){return name;},set name(next:typeof name){name=next;},
get notes(){return notes;},set notes(next:typeof notes){notes=next;},
get type(){return type;},set type(next:typeof type){type=next;},
get types(){return types;},set types(next:typeof types){types=next;},
get entityChoices(){return entityChoices;},set entityChoices(next:typeof entityChoices){entityChoices=next;},
get entityQuery(){return entityQuery;},set entityQuery(next:typeof entityQuery){entityQuery=next;},
get left(){return left;},set left(next:typeof left){left=next;},
get right(){return right;},set right(next:typeof right){right=next;},
get namespace(){return namespace;},set namespace(next:typeof namespace){namespace=next;},
get value(){return value;},set value(next:typeof value){value=next;},
get relation(){return relation;},set relation(next:typeof relation){relation=next;},
get reason(){return reason;},set reason(next:typeof reason){reason=next;},
get dimension(){return dimension;},set dimension(next:typeof dimension){dimension=next;},
get assessedValue(){return assessedValue;},set assessedValue(next:typeof assessedValue){assessedValue=next;},
get scales(){return scales;},set scales(next:typeof scales){scales=next;},
get citations(){return citations;},set citations(next:typeof citations){citations=next;},
get role(){return role;},set role(next:typeof role){role=next;},
get evidenceKind(){return evidenceKind;},set evidenceKind(next:typeof evidenceKind){evidenceKind=next;},
get passphrase(){return passphrase;},set passphrase(next:typeof passphrase){passphrase=next;},
get exclusions(){return exclusions;},set exclusions(next:typeof exclusions){exclusions=next;},
get mergeConfirmed(){return mergeConfirmed;},set mergeConfirmed(next:typeof mergeConfirmed){mergeConfirmed=next;},
get preview(){return preview;},set preview(next:typeof preview){preview=next;},previewMerge,loadOptions,findEntities,toggleNetwork,addCitation,grounding,run,poll,cancel,importFile,collect,extract,batchExtract,write,exportCase,verify,restore};
}
export type CaseOperations=ReturnType<typeof createCaseOperations>;
