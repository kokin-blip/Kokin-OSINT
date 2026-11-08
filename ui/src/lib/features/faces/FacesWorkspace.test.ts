import {mount,unmount,flushSync} from 'svelte';
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import FacesWorkspace from './FacesWorkspace.svelte';
import * as api from '../../bindings';
import type {CaseOperations} from '../operations/controller.svelte';
vi.mock('../../bindings',async()=>{const actual=await vi.importActual<typeof import('../../bindings')>('../../bindings');return{...actual,faceStatus:vi.fn(),artifactPage:vi.fn(),facePage:vi.fn(),faceSearch:vi.fn(),facePreview:vi.fn().mockRejectedValue({code:'invalid_value',message:'No synthetic preview'}),resourceList:vi.fn().mockResolvedValue([]),resourceJobs:vi.fn().mockResolvedValue([]),researchBrowser:vi.fn().mockResolvedValue(undefined),entityPage:vi.fn().mockResolvedValue({rows:[]}),facePersonCreate:vi.fn().mockResolvedValue('created-person'),faceReview:vi.fn().mockResolvedValue(undefined),faceGroups:vi.fn().mockResolvedValue([])};});
const face:api.FaceView={id:'face-a',artifact_id:'image-a',run_id:'run',model_identity:'test',bbox:[1,1,20,20],image_size:[100,100],page:0,timestamp_ms:null,detection_score:0.99,crop_png:null,stale:false,entity_id:null,entity_name:null};
let component:ReturnType<typeof mount>|undefined;
const run=vi.fn().mockResolvedValue(undefined);
async function settle(){await new Promise(r=>setTimeout(r,15));flushSync();}
function button(label:string){return[...document.querySelectorAll('button')].find(b=>b.textContent?.trim()===label)!;}
function input(label:string){return[...document.querySelectorAll('label')].find(l=>l.textContent?.trim().startsWith(label))!.querySelector('input')!;}
function setInput(label:string,value:string){const el=input(label);el.value=value;el.dispatchEvent(new Event('input',{bubbles:true}));}
async function setup(){component=mount(FacesWorkspace,{target:document.body,props:{generation:7,active:true,operations:{run,busy:false,online:true,operation:null,result:null} as unknown as CaseOperations,onopen:vi.fn()}});await settle();}
beforeEach(()=>{vi.mocked(api.faceStatus).mockResolvedValue({eligible_artifacts:3,analyzed_artifacts:1,faces:1,auto_analyze:false,resources_present:true,native_available:true,model_identity:'test',limitation:'Partial coverage'});vi.mocked(api.artifactPage).mockResolvedValue({rows:[{artifact_id:'image-a',media_type:'image/png',canonical_locator:'portrait.png'} as api.ArtifactRowView],total:1,offset:0,limit:50});vi.mocked(api.facePage).mockResolvedValue({faces:[face],total:1,offset:0,limitations:[]});vi.mocked(api.faceSearch).mockResolvedValue({matches:[],total:0,offset:0,related:{hits:[],total:0,coverage:{incomplete:2} as api.CoverageView},coverage:{} as api.FaceStatusView,truncated:false});});
afterEach(async()=>{if(component)await unmount(component);document.body.replaceChildren();vi.clearAllMocks();});
it('waits for explicit submission and sends the selected face with names/topics',async()=>{await setup();document.querySelector<HTMLButtonElement>('.portrait')!.click();setInput('Names or topics','harbor project');await settle();expect(api.faceSearch).not.toHaveBeenCalled();button('Find matches').click();await settle();expect(api.faceSearch).toHaveBeenCalledWith(7,expect.objectContaining({face_id:'face-a',query:'harbor project',threshold:0.363,both:false}));expect(document.body.textContent).toContain('not the probability');});
it('web discovery requires query-specific consent and sends no attachment',async()=>{await setup();setInput('Names or topics','synthetic topic');await settle();expect(button('Search web and save response').disabled).toBe(true);const consent=input('Authorize this query');consent.checked=true;consent.dispatchEvent(new Event('change',{bubbles:true}));await settle();button('Search web and save response').click();expect(run).toHaveBeenCalledWith('web_discover',{query:'synthetic topic',page:1,consent:true});expect(JSON.stringify(run.mock.calls)).not.toContain('image-a');setInput('Names or topics','changed topic');await settle();expect(button('Search web and save response').disabled).toBe(true);});
it('creates a reviewed person through the generation-bound atomic command',async()=>{await setup();document.querySelector<HTMLButtonElement>('.portrait')!.click();await settle();button('Review person link').click();await settle();setInput('Or create person','Analyst label');const rationale=document.querySelector<HTMLTextAreaElement>('textarea')!;rationale.value='Compared sources manually';rationale.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Accept person link').click();await settle();expect(api.facePersonCreate).toHaveBeenCalledWith(7,{face_id:'face-a',name:'Analyst label',rationale:'Compared sources manually'});expect(api.faceReview).not.toHaveBeenCalled();});
it('browser handoff opens only the provider and uploads no face',async()=>{await setup();button('Open FaceCheck').click();await settle();expect(api.researchBrowser).toHaveBeenCalledWith('facecheck','');expect(run).not.toHaveBeenCalled();});

it('reads faces from the newly chosen attachment in the same change event',async()=>{await setup();const select=document.querySelector<HTMLSelectElement>('.input-grid select')!;select.value='image-a';select.dispatchEvent(new Event('change',{bubbles:true}));await settle();expect(api.facePage).toHaveBeenLastCalledWith(7,'image-a',0);expect(button('Analyze faces')).toBeDefined();});

it('discards matches arriving after a different query face is selected',async()=>{
 const second={...face,id:'face-b',artifact_id:'image-b'};
 vi.mocked(api.facePage).mockResolvedValue({faces:[face,second],total:2,offset:0,limitations:[]});
 let resolve!:(value:api.FaceSearchView)=>void;
 vi.mocked(api.faceSearch).mockImplementationOnce(()=>new Promise(r=>resolve=r));
 await setup();document.querySelectorAll<HTMLButtonElement>('.face-strip .portrait')[0].click();await settle();button('Find matches').click();await settle();document.querySelectorAll<HTMLButtonElement>('.face-strip .portrait')[1].click();
 resolve({matches:[{face:{...face,entity_name:'Stale candidate'},similarity:1,occurrences:[],occurrence_count:1}],total:1,offset:0,related:{hits:[],total:0,coverage:{incomplete:0} as api.CoverageView},coverage:{} as api.FaceStatusView,truncated:false});await settle();
 expect(document.body.textContent).not.toContain('Stale candidate');expect(document.querySelector('[aria-label="Face matches"]')).toBeNull();
});
