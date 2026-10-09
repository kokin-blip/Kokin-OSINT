import {mount,unmount,flushSync} from 'svelte';
import {it,expect,vi} from 'vitest';
import GraphPanel from './GraphPanel.svelte';
vi.mock('./bindings',async()=>({...await vi.importActual('./bindings'),startOperation:vi.fn().mockResolvedValue('graph-job'),cancelOperation:vi.fn(),operationStatus:vi.fn().mockResolvedValue({status:'completed',result_json:JSON.stringify({nodes:[{id:'a',canonical_id:'a',name:'Alpha',depth:0},{id:'b',canonical_id:'b',name:'Beta',depth:1}],edges:[{id:'edge',from:'a',to:'b',kind:'knows'}],truncated:true,completed_hops:1})})}));
it('discloses truncation and opens evidence without assigning confidence from layout',async()=>{
 const open=vi.fn();const target=document.createElement('div');document.body.append(target);const component=mount(GraphPanel,{target,props:{seed:'a',generation:1,onopen:open}});
 try{target.querySelector('summary')!.click();target.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await new Promise(r=>setTimeout(r,10));flushSync();expect(target.textContent).toContain('Truncated by traversal limits');expect(target.textContent).toContain('do not indicate confidence');target.querySelector<HTMLAnchorElement>('a[aria-label="Open knows relationship"]')!.dispatchEvent(new MouseEvent('click',{bubbles:true,cancelable:true}));expect(open).toHaveBeenCalledWith('relationship','edge');}finally{await unmount(component);target.remove();}
});
