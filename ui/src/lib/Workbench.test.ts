import {mount,unmount,flushSync} from 'svelte';
import {afterEach,expect,it,vi} from 'vitest';
import Workbench from './Workbench.svelte';
import * as api from './bindings';
vi.mock('./bindings',async()=>{
 const actual=await vi.importActual<typeof import('./bindings')>('./bindings');
 return {...actual,artifactPage:vi.fn().mockResolvedValue({rows:[],total:151,offset:0,limit:50}),entityPage:vi.fn().mockResolvedValue({rows:[],total:0,offset:0,limit:50}),entityTypes:vi.fn().mockResolvedValue([]),scaleOptions:vi.fn().mockResolvedValue([]),networkMode:vi.fn().mockResolvedValue(false),searchCase:vi.fn().mockResolvedValue({hits:[{subject_id:'obs',subject_kind:'observation',title:'Match',facet:'text',snippet:'synthetic',superseded:false,canonical_id:null}],total:1,facets:[],coverage:{artifacts:2,complete:1,never_attempted:1,in_progress:0,partial:0,failed:0}}),activityPage:vi.fn().mockResolvedValue({page:{rows:[],total:0,offset:0,limit:50},chain_status:'Intact',chain_detail:'Verified'})};
});
let component:ReturnType<typeof mount>|undefined;
afterEach(async()=>{if(component)await unmount(component);document.body.replaceChildren();vi.restoreAllMocks();vi.clearAllMocks();vi.mocked(api.entityPage).mockResolvedValue({rows:[],total:0,offset:0,limit:50});});
async function settle(){await new Promise(r=>setTimeout(r,10));flushSync();}
async function setup(){component=mount(Workbench,{target:document.body,props:{generation:1}});await settle();}
function button(label:string){return [...document.querySelectorAll('button')].find(b=>b.textContent?.trim()===label)!;}
it('paginates beyond the old list cap using advertised totals',async()=>{await setup();expect(document.body.textContent).toContain('of 151');button('Next').click();await settle();expect(api.artifactPage).toHaveBeenLastCalledWith(50);});
it('shows coverage even alongside nonempty search results and audit limitations',async()=>{await setup();document.querySelector<HTMLButtonElement>('nav button:nth-child(3)')!.click();await settle();expect(document.body.textContent).toContain('Match');expect(document.body.textContent).toContain('1 of 2 documents fully read');expect(document.body.textContent).toContain('No match does not establish absence');document.querySelector<HTMLButtonElement>('nav button:nth-child(4)')!.click();await settle();expect(document.body.textContent).toContain('Tamper-evident, not chain of custody');});

it('focuses case search with Control K and waits for explicit submission',async()=>{
 await setup();document.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true}));
 const input=document.querySelector<HTMLInputElement>('#case-global-query')!;expect(document.activeElement).toBe(input);
 const reads=vi.mocked(api.searchCase).mock.calls.length;input.value='stored keyword';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();expect(api.searchCase).toHaveBeenCalledTimes(reads);
 document.querySelector<HTMLFormElement>('.global-search')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await settle();
 expect(api.searchCase).toHaveBeenLastCalledWith(expect.objectContaining({query:'stored keyword',offset:0}));expect(document.querySelector('nav button[aria-current="page"]')?.textContent).toBe('Search');
});

it('routes the case-card import through the generation-bound native picker',async()=>{
 const picker=vi.spyOn(api,'selectPath').mockResolvedValue(null);await setup();button('Import evidence').click();await settle();expect(picker).toHaveBeenCalledWith('import',1);
});

it('discards a citation preview that arrives after the dossier is closed',async()=>{
 const record:api.EntityView={requested_id:'person',entity_id:'person',redirected:false,type_key:'person',display_name:'Stored person',notes:'',merged_from:[],identifiers:[],history:[],confidence:[],evidence:[{link_id:'link',entity_id:'person',evidence_kind:'observation',evidence_id:'delayed',role:'supports'}]};
 vi.mocked(api.entityPage).mockResolvedValue({rows:[{entity_id:'person',type_key:'person',display_name:'Stored person',identifier_count:0,evidence_count:1,merged_count:0,assessed_dimensions:0,total_dimensions:7}],total:1,offset:0,limit:50});
 vi.spyOn(api,'entity').mockResolvedValue(record);
 vi.spyOn(api,'relationships').mockResolvedValue({rows:[],total:0,offset:0,limit:50});
 let rejectPreview!:(error:unknown)=>void;
 vi.spyOn(api,'observation').mockImplementation(()=>new Promise((_,reject)=>rejectPreview=reject));
 await setup();button('Entities').click();await settle();document.querySelector<HTMLButtonElement>('section.list button.row')!.click();await settle();
 expect(document.querySelector('section.detail')).not.toBeNull();button('Close detail').click();rejectPreview({code:'evidence_unavailable',message:'Late preview failure'});await settle();
 expect(document.querySelector('section.detail')).toBeNull();expect(document.body.textContent).not.toContain('Late preview failure');
 vi.restoreAllMocks();
});
