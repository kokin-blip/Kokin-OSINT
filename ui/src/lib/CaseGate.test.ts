import {mount,unmount,flushSync} from 'svelte';
import {afterEach,expect,it,vi} from 'vitest';
import CaseGate from './CaseGate.svelte';
import type {Session} from './session.svelte';
let component:ReturnType<typeof mount>;
afterEach(async()=>{await unmount(component);document.body.replaceChildren();});
function setup(){
 const create=vi.fn().mockResolvedValue(undefined);
 component=mount(CaseGate,{target:document.body,props:{session:{busy:false,create,open:vi.fn(),openWithRecoveryKey:vi.fn()} as unknown as Session}});
 flushSync();return create;
}
function button(text:string){return [...document.querySelectorAll('button')].find(b=>b.textContent?.trim()===text)!;}
function fill(label:string,value:string){const field=[...document.querySelectorAll<HTMLInputElement>('input')].find(i=>i.closest('label')?.textContent?.trim()===label)!;field.value=value;field.dispatchEvent(new Event('input',{bubbles:true}));flushSync();return field;}
it('creates with an empty path while keeping credentials and a name mandatory',async()=>{
 const create=setup();button('Create new').click();flushSync();expect(button('Create case').disabled).toBe(true);
 const path=fill('Case folder','   ');expect(path.required).toBe(false);fill('Case name','My case');fill('Passphrase','local passphrase');
 expect(button('Create case').disabled).toBe(false);
 document.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));await new Promise(resolve=>setTimeout(resolve,0));flushSync();
 expect(create).toHaveBeenCalledWith('','My case','local passphrase');expect(document.querySelector<HTMLInputElement>('input[type="password"]')!.value).toBe('');
});
it('preserves an explicitly entered destination',()=>{
 const create=setup();button('Create new').click();flushSync();fill('Case folder','/chosen/Case.kokincase');fill('Case name','Case');fill('Passphrase','passphrase');
 document.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}));expect(create).toHaveBeenCalledWith('/chosen/Case.kokincase','Case','passphrase');
});
it('still requires a folder when opening an existing case',()=>{
 setup();fill('Passphrase','passphrase');expect(document.querySelector<HTMLInputElement>('input[type="text"]')!.required).toBe(true);expect(button('Open case').disabled).toBe(true);
});
