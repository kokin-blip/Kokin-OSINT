import {mount,unmount,flushSync} from 'svelte';
import {it,expect,vi} from 'vitest';
import GraphPanel from './GraphPanel.svelte';
import * as api from './bindings';
vi.mock('./bindings',async()=>({...await vi.importActual('./bindings'),startOperation:vi.fn().mockResolvedValue('graph-job'),cancelOperation:vi.fn(),operationStatus:vi.fn().mockResolvedValue({status:'completed',result_json:JSON.stringify({nodes:[{id:'a',canonical_id:'a',name:'Alpha',depth:0},{id:'b',canonical_id:'b',name:'Beta',depth:1}],edges:[{id:'edge',from:'a',to:'b',kind:'knows'}],truncated:true,completed_hops:1})})}));
it('discloses truncation and opens evidence without assigning confidence from layout',async()=>{
 const open=vi.fn();const target=document.createElement('div');document.body.append(target);const component=mount(GraphPanel,{target,props:{seed:'a',generation:1,onopen:open}});
 try{target.querySelector('summary')!.click();target.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await new Promise(r=>setTimeout(r,10));flushSync();expect(target.textContent).toContain('Truncated by traversal limits');expect(target.textContent).toContain('do not indicate confidence');target.querySelector<HTMLAnchorElement>('a[aria-label="Open knows relationship"]')!.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true}));expect(open).toHaveBeenCalledWith('relationship','edge');}finally{await unmount(component);target.remove();}
});
it('pans and zooms without changing evidence links, and focuses a loaded record',async()=>{
 const target=document.createElement('div');document.body.append(target);const component=mount(GraphPanel,{target,props:{seed:'a',generation:1,onopen:vi.fn()}});
 try{target.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await new Promise(r=>setTimeout(r,10));flushSync();const drawing=target.querySelector<HTMLElement>('.drawing')!,group=target.querySelector('g')!;
 const before=group.getAttribute('transform');drawing.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true,cancelable:true}));flushSync();expect(group.getAttribute('transform')).not.toBe(before);
 drawing.dispatchEvent(new MouseEvent('mousedown',{button:0,clientX:10,clientY:10,bubbles:true,cancelable:true}));window.dispatchEvent(new MouseEvent('mousemove',{clientX:20,clientY:30}));window.dispatchEvent(new MouseEvent('mouseup'));flushSync();expect(group.getAttribute('transform')).not.toBe(before);
 const panned=group.getAttribute('transform');drawing.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',ctrlKey:true,altKey:true,bubbles:true}));flushSync();expect(group.getAttribute('transform')).toBe(panned);drawing.dispatchEvent(new WheelEvent('wheel',{deltaY:100,bubbles:true,cancelable:true}));flushSync();expect(group.getAttribute('transform')).toBe(panned);
 [...target.querySelectorAll('button')].find(b=>b.textContent==='Focus seed')!.click();flushSync();expect(target.textContent).toContain('Focused: Alpha');expect(target.querySelectorAll('svg a')).toHaveLength(3);
 }finally{await unmount(component);target.remove();}
});

it('cancels a graph job whose ID arrives after its case view unmounts',async()=>{
 let resolve!:(id:string)=>void;vi.mocked(api.startOperation).mockImplementationOnce(()=>new Promise(r=>resolve=r));const target=document.createElement('div');document.body.append(target);const component=mount(GraphPanel,{target,props:{seed:'a',generation:1,onopen:vi.fn()}});flushSync();
 target.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await unmount(component);resolve('late-job');await new Promise(r=>setTimeout(r,0));expect(api.cancelOperation).toHaveBeenCalledWith('late-job');target.remove();
});

it('opens a stored alias ID so the backend can preserve its redirect and attribution',async()=>{const open=vi.fn();const target=document.createElement('div');document.body.append(target);const component=mount(GraphPanel,{target,props:{seed:'alias',generation:1,onopen:open,initial:{version:1,roots:['alias'],camera:{x:0,y:0,scale:1},positions:{},hops:2,limit:40,perNode:16,direction:'both',relationshipKind:'',view:{nodes:[{id:'alias',canonical_id:'survivor',name:'Original alias',depth:0}],edges:[],completed_hops:0,truncated:false}}}});flushSync();target.querySelector('svg a')?.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true}));expect(open).toHaveBeenCalledWith('entity','alias');await unmount(component);});
