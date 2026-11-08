import {mount,unmount,flushSync} from 'svelte';
import {expect,it,vi} from 'vitest';
import CommandPalette from './CommandPalette.svelte';
it('filters commands, skips unavailable actions, cycles with arrows and runs the selected command',async()=>{
 const originalShow=Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype,'showModal'),originalClose=Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype,'close');
 Object.defineProperty(HTMLDialogElement.prototype,'showModal',{configurable:true,value:function(this:HTMLDialogElement){this.setAttribute('open','');}});
 Object.defineProperty(HTMLDialogElement.prototype,'close',{configurable:true,value:function(this:HTMLDialogElement){this.removeAttribute('open');}});
 const target=document.createElement('div');document.body.append(target);const run=vi.fn(),close=vi.fn(),error=vi.fn();
 const component=mount(CommandPalette,{target,props:{commands:[{id:'a',label:'Evidence',description:'Browse',run},{id:'b',label:'Graph',description:'Explore',disabled:'Select a record first',run}],onclose:close,onerror:error}});
 try{flushSync();const input=target.querySelector('input')!;expect(document.activeElement).toBe(input);
 input.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowUp',bubbles:true,cancelable:true}));expect(document.activeElement?.textContent).toContain('Evidence');
 input.value='graph';input.dispatchEvent(new Event('input',{bubbles:true}));flushSync();expect(target.querySelector('.commands button')?.hasAttribute('disabled')).toBe(true);
 input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));await Promise.resolve();expect(run).not.toHaveBeenCalled();
 input.value='evidence';input.dispatchEvent(new Event('input',{bubbles:true}));flushSync();input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true,cancelable:true}));await new Promise(r=>setTimeout(r,0));expect(close).toHaveBeenCalled();expect(run).toHaveBeenCalledOnce();expect(error).not.toHaveBeenCalled();
 }finally{await unmount(component);target.remove();if(originalShow)Object.defineProperty(HTMLDialogElement.prototype,'showModal',originalShow);else delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).showModal;if(originalClose)Object.defineProperty(HTMLDialogElement.prototype,'close',originalClose);else delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).close;vi.restoreAllMocks();}
});
