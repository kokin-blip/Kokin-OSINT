import {mount,unmount,flushSync} from 'svelte';import {afterEach,expect,it,vi} from 'vitest';import Harness from '../../../../tests/unit/OperationsHarness.svelte';import * as api from '../../bindings';
vi.mock('../../bindings',async()=>({...await vi.importActual('../../bindings'),networkMode:vi.fn().mockResolvedValue(false),entityTypes:vi.fn().mockResolvedValue([]),scaleOptions:vi.fn().mockResolvedValue([]),entityPage:vi.fn().mockResolvedValue({rows:[],total:0,offset:0,limit:50}),operationStatus:vi.fn().mockResolvedValue({status:'completed',result_json:'{}'}),startOperation:vi.fn(),cancelOperation:vi.fn().mockResolvedValue(undefined),selectPath:vi.fn()}));
afterEach(()=>{vi.clearAllMocks();document.body.replaceChildren();});
it('cancels a job ID arriving after the originating case controller is destroyed',async()=>{let finish!:(id:string)=>void;vi.mocked(api.startOperation).mockImplementationOnce(()=>new Promise(r=>finish=r));const changed=vi.fn(),component=mount(Harness,{target:document.body,props:{changed}});flushSync();const pending=component.operations.run('extract',{});component.operations.passphrase='temporary export credential';await unmount(component);finish('late-operation');await pending;expect(api.cancelOperation).toHaveBeenCalledWith('late-operation');expect(changed).not.toHaveBeenCalled();expect(component.operations.passphrase).toBe('');});
it('does not start an import after its native selection returns to a closed case',async()=>{let finish!:(id:string)=>void;vi.mocked(api.selectPath).mockImplementationOnce(()=>new Promise(r=>finish=r));const component=mount(Harness,{target:document.body,props:{changed:vi.fn()}});flushSync();const pending=component.operations.importFile();await unmount(component);finish('stale-selection');await pending;expect(api.startOperation).not.toHaveBeenCalled();});

it('keeps a contextual assessment target while the main inspector navigates elsewhere',async()=>{vi.mocked(api.startOperation).mockResolvedValueOnce('assessment-job');const component=mount(Harness,{target:document.body,props:{changed:vi.fn()}});flushSync();await new Promise(r=>setTimeout(r,0));component.operations.target={kind:'relationship',id:'selected-edge'};component.operations.action='assess';component.operations.dimension='review_status';component.operations.assessedValue='accepted';component.operations.reason='Human-reviewed source';await component.operations.write(new Event('submit',{cancelable:true}) as SubmitEvent);expect(api.startOperation).toHaveBeenCalledWith('assess',expect.objectContaining({subject_kind:'relationship',subject_id:'selected-edge'}),1);await unmount(component);});

it('preserves a partial source result for inspection and refreshes evidence',async()=>{
 vi.mocked(api.startOperation).mockResolvedValueOnce('partial-source');
 vi.mocked(api.operationStatus).mockResolvedValueOnce({operation_id:'partial-source',status:'partial',stage:'Finished',cancellable:false,result_json:JSON.stringify({artifact_id:'saved-error-response',complete:false,processing_error:'Service error preserved'}),error:null});
 const changed=vi.fn(),component=mount(Harness,{target:document.body,props:{changed}});flushSync();await new Promise(r=>setTimeout(r,0));
 await component.operations.run('source_collect',{});
 expect(component.operations.busy).toBe(false);expect(component.operations.result?.complete).toBe(false);expect(component.operations.result?.artifact_id).toBe('saved-error-response');expect(changed).toHaveBeenCalledOnce();await unmount(component);
});

it('queues optional face analysis after a supported import without changing import defaults',async()=>{
 vi.spyOn(api,'faceStatus').mockResolvedValue({auto_analyze:true} as api.FaceStatusView);
 vi.spyOn(api,'artifactDocument').mockResolvedValue({media_type:'image/png'} as api.DocumentView);
 vi.mocked(api.startOperation).mockResolvedValueOnce('import-job').mockResolvedValueOnce('face-job');
 vi.mocked(api.operationStatus).mockResolvedValueOnce({operation_id:'import-job',status:'completed',stage:'Finished',cancellable:false,result_json:'{"artifact_id":"new-photo"}',error:null}).mockResolvedValueOnce({operation_id:'face-job',status:'completed',stage:'Finished',cancellable:false,result_json:'{"faces":1}',error:null});
 const component=mount(Harness,{target:document.body,props:{changed:vi.fn()}});flushSync();await component.operations.run('import',{selection:'native-choice'});await new Promise(r=>setTimeout(r,20));
 expect(api.startOperation).toHaveBeenNthCalledWith(2,'face_analyze',{version:1,artifact_ids:['new-photo'],page:0,start_ms:0,end_ms:null},1);await unmount(component);vi.restoreAllMocks();
});
