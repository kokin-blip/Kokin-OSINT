import {onMount} from 'svelte';
import * as api from '../../bindings';
export type GuideEvent='observation_read'|'source_read'|'graph_relationship_read'|'package_exported';
export function createGuide(generation:()=>number){
 let status=$state<api.GuideStatus|null>(null),events=$state<Partial<Record<GuideEvent,boolean>>>({}),dismissed=$state(false),error=$state<api.CommandError|null>(null);let revision=0,live=true,queue=Promise.resolve(),dismissalIntent:boolean|undefined;
 async function refresh(){try{const result=await api.guideStatus(generation());if(live){status=result;error=null;}}catch(raw){if(live)error=api.asCommandError(raw);}}
 async function initialize(){try{const row=await api.workspaceRead(generation(),'investigation-guide');if(live){revision=row.revision;events=(row.payload.events as typeof events)??{};dismissed=dismissalIntent??row.payload.dismissed===true;}}catch(raw){if(api.asCommandError(raw).code!=='not_found'&&live)error=api.asCommandError(raw);}await refresh();}
 onMount(()=>{queue=initialize();return()=>{live=false;};});
 function persist(){queue=queue.then(async()=>{if(!live)return;try{const row=await api.workspaceSave(generation(),{id:'investigation-guide',kind:'guide',name:'Investigation guide',expected_revision:revision,payload:{version:1,events:$state.snapshot(events),dismissed}});if(live)revision=row.revision;}catch(raw){if(live)error=api.asCommandError(raw);}});}
 function observe(event:GuideEvent){if(events[event])return;queue=queue.then(()=>{if(live&&!events[event]){events[event]=true;persist();}});void refresh();}
 function show(){dismissalIntent=false;dismissed=false;persist();void refresh();}function hide(){dismissalIntent=true;dismissed=true;persist();}
 return {get status(){return status;},get events(){return events;},get dismissed(){return dismissed;},get error(){return error;},refresh,observe,show,hide};
}
export type InvestigationGuide=ReturnType<typeof createGuide>;
