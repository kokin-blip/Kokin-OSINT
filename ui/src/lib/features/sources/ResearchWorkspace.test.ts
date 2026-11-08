import {mount,unmount,flushSync} from 'svelte';
import {it,expect,vi,afterEach} from 'vitest';
import ResearchWorkspace from './ResearchWorkspace.svelte';
import * as api from '../../bindings';
vi.mock('../../bindings',async()=>({...await vi.importActual('../../bindings'),
 artifactPage:vi.fn().mockResolvedValue({rows:[{artifact_id:'query-image',media_type:'image/png',canonical_locator:'Synthetic image',byte_length:100,observations:0}],total:1,offset:0,limit:50}),researchDropListen:vi.fn().mockResolvedValue(()=>{}),researchLocalSearch:vi.fn().mockResolvedValue({version:1,scope:'current encrypted case index',external_requests:0,offset:0,limit:50,queries:[]}),researchImportSession:vi.fn(),researchRegistry:vi.fn().mockResolvedValue([{id:'github',title:'GitHub',category:'Public profiles',selectors:['username'],delivery:'native',health:'fixture-tested',limitation:'Public leads',max_calls:1}]),
 selectPath:vi.fn().mockResolvedValue('synthetic-selection'),researchImportPreview:vi.fn().mockResolvedValue({columns:['First name','Last name'],rows:[['Jane','Doe']],warnings:[]}),researchTableSuggestions:vi.fn().mockResolvedValue([{column:0,kind:'name',valid:1,sampled:1,basis:'header'}]),researchTableMap:vi.fn().mockResolvedValue([{value:'Jane Doe',kind:'name',selector:{kind:'name',original:'Jane Doe',normalized:'Jane Doe'},error:null,repeats:1,origin_ids:[]}]),researchCandidates:vi.fn().mockResolvedValue({rows:[{value:'Case@example.org',kind:'email',selector:{kind:'email',original:'Case@example.org',normalized:'Case@example.org'},error:null,repeats:1,origin_ids:['local-paste']}],warnings:['Review syntax candidates before selecting.'],segment_count:1}),researchEvidencePreview:vi.fn().mockResolvedValue({rows:[{value:'example.org',kind:'domain',selector:{kind:'domain',original:'example.org',normalized:'example.org'},error:null,repeats:1,origin_ids:['observation-1']}],warnings:['Recorded OCR needs review.'],segment_count:1}),
 researchDirectory:vi.fn().mockResolvedValue(Array.from({length:1166},(_,i)=>({id:`tool-${i}`,name:`Tool ${i}`,category:'Usernames > Public',url:'https://example.org',description:'Public directory entry',pricing:'unknown'}))),
 researchDocumentSave:vi.fn().mockImplementation((_,d)=>Promise.resolve({...d,revision:d.revision+1})),researchDocuments:vi.fn().mockResolvedValue([]),researchRuns:vi.fn().mockResolvedValue({rows:[],total:0,offset:0,limit:50}),
 researchSelector:vi.fn().mockImplementation((kind,original)=>Promise.resolve({kind,original,normalized:original})),
 researchPrepare:vi.fn().mockResolvedValue({token:'single-use',digest:'bound',plan:{max_calls:1,limitation:'Only reviewed sources',request_urls:['https://api.github.com/users/example'],provider_call_limits:{},provider_credit_upper_bounds:{}}}),
 researchBulk:vi.fn().mockResolvedValue(Array.from({length:20000},(_,i)=>({value:`user${i}`,kind:'username',selector:{kind:'username',original:`user${i}`,normalized:`user${i}`},error:null,repeats:1}))),
 startOperation:vi.fn().mockResolvedValue('research-job'),operationStatus:vi.fn().mockResolvedValue({status:'completed',stage:'Finished'}),researchResults:vi.fn().mockResolvedValue({rows:[],total:0,offset:0,limit:50}),cancelOperation:vi.fn().mockResolvedValue(undefined),researchOpenUrl:vi.fn().mockResolvedValue(undefined)
}));
let component:ReturnType<typeof mount>|undefined;
afterEach(async()=>{if(component)await unmount(component);component=undefined;document.body.replaceChildren();vi.clearAllMocks();});
async function settle(){await new Promise(r=>setTimeout(r,10));flushSync();}
const button=(name:string)=>[...document.querySelectorAll('button')].find(b=>b.textContent?.trim()===name)!;
async function setup(online=true){component=mount(ResearchWorkspace,{target:document.body,props:{generation:7,online,onopen:vi.fn()}});await settle();}
it('typing cannot start research and editing invalidates the reviewed authorization',async()=>{
 await setup();const input=document.querySelector<HTMLInputElement>('.query-form input[maxlength]')!;
 input.value='example';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();
 expect(api.researchSelector).not.toHaveBeenCalled();expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
 button('Review research request').click();await settle();expect(api.researchPrepare).toHaveBeenCalledOnce();
 expect(button('Start reviewed checks').disabled).toBe(true);
 document.querySelector<HTMLInputElement>('.disclosure input')!.click();await settle();expect(button('Start reviewed checks').disabled).toBe(false);
 input.value='changed';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();expect(document.querySelector('.disclosure')).toBeNull();expect(api.startOperation).not.toHaveBeenCalled();
});
it('keeps offline requests disabled even after local planning and consent',async()=>{
 await setup(false);const input=document.querySelector<HTMLInputElement>('.query-form input[maxlength]')!;input.value='example';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Review research request').click();await settle();document.querySelector<HTMLInputElement>('.disclosure input')!.click();await settle();expect(button('Start reviewed checks').disabled).toBe(true);expect(api.startOperation).not.toHaveBeenCalled();
});
it('renders only one bounded directory page and distinguishes listings from connectors',async()=>{
 await setup();button('Tool directory').click();await settle();expect(document.querySelectorAll('.tool-row')).toHaveLength(50);expect(document.body.textContent).toContain('not installed connectors');button('Next tools').click();await settle();expect(document.body.textContent).toContain('Tool 50');expect(document.querySelectorAll('.tool-row')).toHaveLength(50);
});
it('large previews retain all rows while mounting only 50 and selecting at most 15',async()=>{
 await setup();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();const textarea=document.querySelector('textarea')!;textarea.value='synthetic import';textarea.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Preview pasted queries').click();await settle();expect(document.body.textContent).toContain('20000 distinct rows. 15 selected');expect(document.querySelectorAll('.batch-row')).toHaveLength(50);button('Next imported rows').click();await settle();expect(document.body.textContent).toContain('user50');expect(api.startOperation).not.toHaveBeenCalled();
});

it('filters all saved responses before paging and shows whole-run coverage counts',async()=>{
 vi.mocked(api.researchRuns).mockResolvedValueOnce({rows:[{id:'older-run',plan:{selectors:[{kind:'username',original:'example',normalized:'example'}]},status:'completed',expected:151,completed:151}],total:1,offset:0,limit:50} as api.ResearchPage<api.ResearchRun>);
 const row={id:'beyond-first-page',run_id:'older-run',source:'github',selector:{kind:'username',original:'example',normalized:'example'},outcome:'blocked',fields:{},limitation:'Synthetic fixture',artifact_id:null,observed_utc:'2026-10-20T00:00:00Z',from_cache:false,url:''} as api.ResearchResult;
 vi.mocked(api.researchResults).mockImplementation(async(_g,_run,offset=0,filter={outcome:'',sources:[]})=>({rows:filter.outcome==='blocked'?[row]:[],total:filter.outcome==='blocked'?1:151,run_total:151,source_counts:{github:151},outcome_counts:{unknown:150,blocked:1},offset,limit:50}));
 await setup();button('Run history').click();await settle();button('example').click();await settle();
 const outcome=document.querySelector<HTMLSelectElement>('.result-controls select')!;outcome.value='blocked';outcome.dispatchEvent(new Event('change',{bubbles:true}));await settle();
 expect(api.researchResults).toHaveBeenLastCalledWith(7,'older-run',0,{outcome:'blocked',sources:[]});
 expect(document.body.textContent).toContain('151 saved responses · 1 matching');expect(document.querySelectorAll('.results article')).toHaveLength(1);
 expect(document.querySelector('.result-controls')?.textContent).toContain('unknown (150)');
 const source=document.querySelectorAll<HTMLSelectElement>('.result-controls select')[2];source.value='github';source.dispatchEvent(new Event('change',{bubbles:true}));await settle();
 expect(api.researchResults).toHaveBeenLastCalledWith(7,'older-run',0,{outcome:'blocked',sources:['github']});
});
it('keeps tool review private and returns focus when closing details',async()=>{
 await setup();button('Tool directory').click();await settle();
 const name=document.querySelector<HTMLButtonElement>('.tool-name')!;name.focus();name.click();await settle();
 expect(document.activeElement?.getAttribute('aria-label')).toBe('Tool details');
 const notes=document.querySelector<HTMLTextAreaElement>('.tool-detail textarea')!;notes.value='Private source note';notes.dispatchEvent(new Event('input',{bubbles:true}));
 const rating=document.querySelector<HTMLSelectElement>('.tool-detail select')!;rating.value='3.5';rating.dispatchEvent(new Event('change',{bubbles:true}));button('Save private review').click();await settle();
 expect(api.researchDocumentSave).toHaveBeenLastCalledWith(7,expect.objectContaining({kind:'tool_preferences',payload:expect.objectContaining({notes:{'tool-0':'Private source note'},ratings:{'tool-0':3.5}})}));
 expect(api.researchOpenUrl).not.toHaveBeenCalled();button('Close details').click();await settle();expect(document.activeElement).toBe(name);
});

it('mixed pasted candidates remain unselected until explicit row review',async()=>{
 await setup();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();const input=document.querySelector('textarea')!;input.value='Contact Case@example.org';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Extract mixed candidates locally').click();await settle();
 expect(document.body.textContent).toContain('1 distinct rows. 0 selected');expect(button('Review research request').disabled).toBe(true);expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
});
it('joins explicitly mapped name columns without submitting a search',async()=>{
 await setup();const type=document.querySelector<HTMLSelectElement>('.query-form select')!;type.value='name';type.dispatchEvent(new Event('change',{bubbles:true}));await settle();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();button('Choose file for local preview').click();await settle();
 const section=document.querySelector('section[aria-label="Import column mapping"]')!;const joins=section.querySelectorAll<HTMLSelectElement>('select')[1];joins.value='1';joins.dispatchEvent(new Event('change',{bubbles:true}));await settle();button('Validate mapped column').click();await settle();
 expect(api.researchTableMap).toHaveBeenLastCalledWith(expect.objectContaining({columns:['First name','Last name']}),0,'name',[0,1]);expect(document.body.textContent).toContain('Jane Doe');expect(api.startOperation).not.toHaveBeenCalled();
});
it('evidence candidates require selection and bind observation origins to preparation',async()=>{
 await setup();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();const artifact=document.querySelector<HTMLInputElement>('.evidence-preview input[maxlength]')!;artifact.value='pdf-artifact';artifact.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Preview candidates from evidence').click();await settle();
 expect(document.body.textContent).toContain('0 selected');expect(document.body.textContent).toContain('observation-1');expect(api.researchPrepare).not.toHaveBeenCalled();document.querySelector<HTMLInputElement>('.batch-row input')!.click();await settle();button('Review research request').click();await settle();
 expect(api.researchPrepare).toHaveBeenLastCalledWith(7,expect.objectContaining({selectors:[{kind:'domain',original:'example.org',normalized:'example.org'}]}),[{selector:{kind:'domain',original:'example.org',normalized:'example.org'},observation_ids:['observation-1']}],null);expect(api.startOperation).not.toHaveBeenCalled();
});

it('clears and rebuilds a batch selection locally while retaining unselected rows',async()=>{
 await setup();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();const input=document.querySelector('textarea')!;input.value='Synthetic batch';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Preview pasted queries').click();await settle();button('Clear batch selection').click();await settle();expect(document.body.textContent).toContain('20000 distinct rows. 0 selected');button('Select next 15 unsubmitted queries').click();await settle();expect(document.body.textContent).toContain('20000 distinct rows. 15 selected');expect(api.startOperation).not.toHaveBeenCalled();expect(api.researchPrepare).not.toHaveBeenCalled();
});

it('exposes eight selector entry points without sending a lookup when switching types',async()=>{
 await setup();const type=document.querySelector<HTMLSelectElement>('.query-form select')!;
 expect([...type.options].map(o=>o.value)).toEqual(['username','email','phone','name','domain','ip','wallet','image']);
 for(const kind of ['email','phone','name','domain','ip','wallet','image','username']){type.value=kind;type.dispatchEvent(new Event('change',{bubbles:true}));await settle();}
 expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
});

it('uses the native fallback while offline and keeps external checks collapsed by default',async()=>{
 await setup(false);expect(document.querySelector<HTMLDetailsElement>('.external-research')!.open).toBe(false);
 const input=document.querySelector<HTMLInputElement>('.query-form input[maxlength]')!;input.value='example';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();
 expect(api.researchLocalSearch).not.toHaveBeenCalled();button('Search encrypted case').click();await settle();
 expect(api.researchLocalSearch).toHaveBeenCalledWith(7,[{kind:'username',original:'example',normalized:'example'}],0);
 expect(document.body.textContent).toContain('0 external requests');expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
});
it('resumes encrypted import rows unselected and binds a parent revision to reviewed batches',async()=>{
 const row=(value:string)=>({value,kind:'username' as const,selector:{kind:'username' as const,original:value,normalized:value},repeats:1,error:null,origin_ids:[]});
 const document={id:'import-1',revision:3,kind:'saved_search',name:'Private batch',payload:{version:1,purpose:'import_session',rows:[row('already'),row('next')]},archived:false,updated_utc:''};
 vi.mocked(api.researchDocuments).mockImplementationOnce(async()=>[]).mockImplementationOnce(async()=>[]).mockImplementationOnce(async()=>[document]);
 vi.mocked(api.researchImportSession).mockResolvedValue({document,submitted_keys:['username:already'],runs:{rows:[{id:'interrupted-run',status:'interrupted',expected:3,completed:1,created_utc:''}],total:1,offset:0,limit:50}});
 await setup();
 window.document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();button('Resume Private batch').click();await settle();
 expect(window.document.body.textContent).toContain('2 distinct rows. 0 selected');expect(window.document.body.textContent).toContain('interrupted');
 button('Select next 15 unsubmitted queries').click();await settle();expect(window.document.body.textContent).toContain('2 distinct rows. 1 selected');button('Review research request').click();await settle();
 expect(api.researchPrepare).toHaveBeenLastCalledWith(7,expect.objectContaining({selectors:[row('next').selector]}),[],{id:'import-1',revision:3});expect(api.startOperation).not.toHaveBeenCalled();
});
it('previews a case-bound native drop only after the analyst chooses preview',async()=>{
 let handler:((event:api.ResearchDrop)=>void)|undefined;vi.mocked(api.researchDropListen).mockImplementationOnce(async h=>{handler=h;return()=>{};});
 await setup();handler?.({generation:8,selection:'wrong-case',name:'other.csv'});await settle();expect(document.querySelector('[aria-label="Dropped research file"]')).toBeNull();
 handler?.({generation:7,selection:'native-one-use-token',name:'selected.csv'});await settle();expect(api.researchImportPreview).not.toHaveBeenCalled();expect(api.researchPrepare).not.toHaveBeenCalled();
 button('Preview dropped text file').click();await settle();expect(api.researchImportPreview).toHaveBeenCalledWith(7,'native-one-use-token','csv',true);expect(document.querySelector('[aria-label="Import column mapping"]')).not.toBeNull();expect(api.startOperation).not.toHaveBeenCalled();
});

it('sends correlated metadata filters to case storage without starting provider research',async()=>{
 vi.mocked(api.researchRuns).mockResolvedValueOnce({rows:[{id:'metadata-run',plan:{selectors:[{kind:'username',original:'example',normalized:'example'}]},status:'completed',expected:1,completed:1}],total:1,offset:0,limit:50} as api.ResearchPage<api.ResearchRun>);
 await setup();button('Run history').click();await settle();button('example').click();await settle();
 const input=[...document.querySelectorAll<HTMLInputElement>('.filters input')].find(e=>e.placeholder==='*example*')!;input.value='*private*';input.dispatchEvent(new Event('input',{bubbles:true}));
 const verified=document.querySelector<HTMLSelectElement>('.filters select')!;verified.value='unknown';verified.dispatchEvent(new Event('change',{bubbles:true}));await settle();
 expect(api.researchResults).toHaveBeenLastCalledWith(7,'metadata-run',0,expect.objectContaining({pattern:'*private*',verified:'unknown'}));expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
 button('Clear metadata filters').click();await settle();expect(api.researchResults).toHaveBeenLastCalledWith(7,'metadata-run',0,{outcome:'',sources:[]});
});

it('selects imported image references per row without uploading or retaining unrelated origins',async()=>{
 await setup();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();const input=document.querySelector('textarea')!;input.value='Synthetic';input.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Extract mixed candidates locally').click();await settle();
 button('Choose image for row 1').click();await settle();const imageButton=[...document.querySelectorAll<HTMLButtonElement>('[aria-label="Query image selection"] button')].find(b=>b.textContent?.includes('Synthetic image'))!;imageButton.click();await settle();expect(document.querySelector<HTMLSelectElement>('.batch-row select')!.value).toBe('image');expect(document.querySelector('.batch-row')!.textContent).toContain('query-image');expect(document.querySelector('.batch-row')!.textContent).not.toContain('local-paste');expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
});

it('adds and removes local rows without transmitting or silently selecting them',async()=>{
 await setup();document.querySelector<HTMLInputElement>('.query-form input[type="checkbox"]')!.click();await settle();const value=[...document.querySelectorAll<HTMLInputElement>('input')].find(i=>i.closest('label')?.textContent?.includes('Add selector value'))!;value.value='manual_user';value.dispatchEvent(new Event('input',{bubbles:true}));await settle();button('Add local row').click();await settle();expect(document.querySelector('[aria-label="Batch preview"]')!.textContent).toContain('1 distinct rows. 0 selected');button('Remove row 1').click();await settle();expect(document.querySelector('[aria-label="Batch preview"]')).toBeNull();expect(button('Add local row')).toBeDefined();expect(api.researchPrepare).not.toHaveBeenCalled();expect(api.startOperation).not.toHaveBeenCalled();
});
