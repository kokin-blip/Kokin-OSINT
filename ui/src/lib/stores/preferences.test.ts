import {afterEach,expect,it,vi} from 'vitest';
import {decodePreferences,defaults,preferences} from './preferences.svelte';
afterEach(()=>{vi.restoreAllMocks();localStorage.clear();preferences.value={...defaults};preferences.storageError=false;});
it('validates stored preferences, clamps widths and discards unknown case data',()=>{
 expect(decodePreferences('{broken')).toEqual(defaults);
 expect(decodePreferences(JSON.stringify({theme:'invalid',inspectorWidth:900,root:'/private/case',passphrase:'secret',reduceMotion:'yes'}))).toEqual({...defaults,inspectorWidth:480});
 expect(decodePreferences('null')).toEqual(defaults);
});
it('persists only known display preferences and restores them',()=>{
 preferences.update({theme:'light',inspectorWidth:400,reduceMotion:true});
 const stored=JSON.parse(localStorage.getItem('kokin.display.v1')!);expect(Object.keys(stored).sort()).toEqual(Object.keys(defaults).sort());
 preferences.value={...defaults};preferences.load();expect(preferences.value.theme).toBe('light');expect(preferences.value.inspectorWidth).toBe(400);
});
it('keeps session preferences usable when storage refuses reads or writes',()=>{
 vi.spyOn(Storage.prototype,'setItem').mockImplementation(()=>{throw new Error('blocked');});
 preferences.update({reduceMotion:true});expect(preferences.value.reduceMotion).toBe(true);expect(preferences.storageError).toBe(true);
 vi.spyOn(Storage.prototype,'getItem').mockImplementation(()=>{throw new Error('blocked');});
 preferences.load();expect(preferences.value).toEqual(defaults);expect(preferences.storageError).toBe(true);
});

it('honors OS reduced motion over the user setting and removes media listeners',async()=>{
 const {mount,unmount,flushSync}=await import('svelte');
 const {default:Harness}=await import('../../../tests/unit/PreferencesHarness.svelte');
 const listeners:ReturnType<typeof vi.fn>[]=[];
 vi.stubGlobal('matchMedia',vi.fn((query:string)=>{
  const remove=vi.fn();listeners.push(remove);
  return {matches:query.includes('reduced-motion'),addEventListener:vi.fn(),removeEventListener:remove};
 }));
 const component=mount(Harness,{target:document.body});flushSync();
 expect(document.documentElement.dataset.reducedMotion).toBe('true');preferences.update({reduceMotion:false});flushSync();expect(document.documentElement.dataset.reducedMotion).toBe('true');
 await unmount(component);expect(listeners.every(listener=>listener.mock.calls.length===1)).toBe(true);
 vi.unstubAllGlobals();delete document.documentElement.dataset.theme;delete document.documentElement.dataset.reducedMotion;delete document.documentElement.dataset.strongFocus;
});
