import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const button=(text)=>$(`button=${text}`);
const input=(label)=>$(`//label[span[text()="${label}"]]/input`);
describe('Encrypted desktop lifecycle',()=>{
  let root;
  after(async()=>{if(root)await rm(root,{recursive:true,force:true});});
  it('creates, checks recovery key, navigates offline, closes and reopens',async()=>{
    root=await mkdtemp(join(tmpdir(),'kokin-desktop-'));
    await button('Create new').waitForDisplayed();await button('Create new').click();
    await input('Case folder').setValue(join(root,'workflow.kokincase'));
    await input('Case name').setValue('Desktop workflow');
    await input('Passphrase').setValue('desktop synthetic passphrase');
    await button('Create case').click();await $('.key').waitForDisplayed();
    const segments=await $$('.key .segment');const challenge=await segments[segments.length-2].getText();
    await $('[role="dialog"] input[type="checkbox"]').click();await $('[role="dialog"] input[type="text"]').setValue(challenge);
    await $('[role="dialog"] button.continue').click();await $('nav[aria-label="Case sections"]').waitForDisplayed();
    for(const section of ['Entities','Search','Activity','Evidence']){await $(`//nav/button[normalize-space(.)="${section}"]`).click();}
    assert.match(await $('main').getText(),/Offline/);
    if(process.env.KOKIN_DESKTOP_INSPECT==='1')await browser.pause(60000);
    await button('Close case').click();await button('Open existing').waitForDisplayed();
    await input('Case folder').setValue(join(root,'workflow.kokincase'));
    await input('Passphrase').setValue('desktop synthetic passphrase');await button('Open case').click();
    await $('nav[aria-label="Case sections"]').waitForDisplayed();assert.match(await $('.case .name').getText(),/Desktop workflow/);
    await button('Close case').click();
  });
});
