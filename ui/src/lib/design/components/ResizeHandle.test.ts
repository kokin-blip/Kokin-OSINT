import {mount,unmount,flushSync} from 'svelte';
import {afterEach,expect,it,vi} from 'vitest';
import ResizeHandle from './ResizeHandle.svelte';
let component:ReturnType<typeof mount>|undefined;
afterEach(async()=>{if(component)await unmount(component);document.body.replaceChildren();});
it('supports keyboard resizing, bounds and explicit collapse',()=>{
 const commit=vi.fn(),collapse=vi.fn();component=mount(ResizeHandle,{target:document.body,props:{value:330,oncommit:commit,oncollapse:collapse,controls:'inspector'}});flushSync();
 const divider=document.querySelector<HTMLElement>('[role="separator"]')!;
 const key=(key:string,shiftKey=false)=>{divider.dispatchEvent(new KeyboardEvent('keydown',{key,shiftKey,bubbles:true,cancelable:true}));flushSync();};
 key('ArrowLeft');expect(commit).toHaveBeenLastCalledWith(340);key('ArrowRight',true);expect(commit).toHaveBeenLastCalledWith(300);
 key('End');expect(divider.getAttribute('aria-valuenow')).toBe('480');key('ArrowLeft');expect(commit).toHaveBeenLastCalledWith(480);
 key('Home');expect(commit).toHaveBeenLastCalledWith(260);key('Enter');expect(collapse).toHaveBeenCalledTimes(1);expect(divider.getAttribute('aria-controls')).toBe('inspector');
});

it('handles a desktop mouse drag beyond the divider and stops listening on teardown',async()=>{
 const commit=vi.fn();component=mount(ResizeHandle,{target:document.body,props:{value:330,oncommit:commit}});flushSync();
 const divider=document.querySelector<HTMLElement>('[role="separator"]')!;
 divider.dispatchEvent(new MouseEvent('mousedown',{clientX:200,button:0,bubbles:true}));window.dispatchEvent(new MouseEvent('mousemove',{clientX:160}));flushSync();expect(divider.getAttribute('aria-valuenow')).toBe('370');
 window.dispatchEvent(new MouseEvent('mouseup'));flushSync();expect(commit).toHaveBeenLastCalledWith(370);
 await unmount(component);component=undefined;window.dispatchEvent(new MouseEvent('mousemove',{clientX:100}));window.dispatchEvent(new MouseEvent('mouseup'));expect(commit).toHaveBeenCalledTimes(1);
});
