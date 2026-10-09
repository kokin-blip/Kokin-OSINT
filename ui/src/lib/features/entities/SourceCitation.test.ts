import { mount, unmount, flushSync } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import SourceCitation, { type SourcePreview } from './SourceCitation.svelte';
let component: ReturnType<typeof mount> | undefined;
afterEach(async()=>{if(component)await unmount(component);document.body.replaceChildren();});
const preview = (quote:SourcePreview['quote']):SourcePreview=>({observation_id:'stored-observation',value:'The recorded assertion',kind:'page.text',superseded:null,quote,lineage:{artifact:{artifact_id:'source',media_type:'text/html',byte_length:100,content_hash:'hash',collected_utc:'2026-10-08',canonical_locator:null,observations:1,extraction:'complete'}}});
it('quotes the backend source excerpt and exposes a disagreement with the recorded assertion',()=>{
 const onopen=vi.fn();component=mount(SourceCitation,{target:document.body,props:{source:preview({agreement:'differs',excerpt:{before:'',quoted:'Different source bytes',after:'',lossy:false}}),onopen}});flushSync();
 expect(document.querySelector('blockquote')?.textContent).toBe('Different source bytes');expect(document.body.textContent).toContain('The source differs');expect(document.body.textContent).toContain('Recorded value: The recorded assertion');
 document.querySelector<HTMLButtonElement>('button')!.click();expect(onopen).toHaveBeenCalledWith('observation','stored-observation');
});
it('does not present an unverified assertion as a source quotation',()=>{
 component=mount(SourceCitation,{target:document.body,props:{source:preview({agreement:'unavailable',document_state:'lost'})}});flushSync();
 expect(document.querySelector('blockquote')).toBeNull();expect(document.body.textContent).toContain('Source excerpt unavailable');expect(document.body.textContent).toContain('Recorded observation: The recorded assertion');
});
