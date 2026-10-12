import {mkdtemp,rm,mkdir,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const button=t=>$(`//button[normalize-space(.)="${t}"]`);
const field=t=>$(`//label[starts-with(normalize-space(.),"${t}")]/input`);
const evaluate=(fn,...args)=>browser.tauri.execute(fn,...args);
const changeTheme=theme=>evaluate((_tauri,theme)=>{const select=document.querySelector('dialog select');select.value=theme;select.dispatchEvent(new Event('change',{bubbles:true}));},theme);
function rss(){const lines=execFileSync('ps',['-axo','pid=,rss=,command='],{encoding:'utf8'}).split('\n').filter(line=>line.includes('/target/debug/bundle/macos/Kokin-OSINT.app/Contents/MacOS/kokin-osint'));return lines.map(line=>{const [pid,kib]=line.trim().split(/\s+/);return {pid:Number(pid),kib:Number(kib)};});}
async function capture(name){if(!process.env.KOKIN_UI_CAPTURE_DIR||process.env.KOKIN_PERF_ONLY==='1')return;await mkdir(process.env.KOKIN_UI_CAPTURE_DIR,{recursive:true});await browser.saveScreenshot(join(process.env.KOKIN_UI_CAPTURE_DIR,`${name}.png`));}
// Pixel dimensions and process RSS below are Mac-specific; Windows needs its own native pass.
const nativeDescribe=process.platform==='darwin'?describe:describe.skip;
nativeDescribe('Night Ledger layout, accessibility and bounded performance',()=>{
 let root;after(async()=>{if(root)await rm(root,{recursive:true,force:true});});
 it('supports settings focus, keyboard resizing, responsive layout and reduced motion',async()=>{
  root=await mkdtemp(join(tmpdir(),'kokin-layout-'));
  await button('Settings').click();await button('Reset display preferences').click();await button('Close settings').click();
  await button('Create new').click();await field('Case folder').setValue(join(root,'layout.kokincase'));await field('Case name').setValue('Layout verification');await field('Passphrase').setValue('synthetic layout key');await button('Create case').click();await $('.key').waitForDisplayed();
  const segments=await $$('.key .segment');await $('[role="dialog"] input[type="checkbox"]').click();await $('[role="dialog"] input[type="text"]').setValue(await segments[segments.length-2].getText());await button('Continue to the case').click();await $('nav').waitForDisplayed();await browser.setWindowSize(2974,2180);
  const rssBefore=rss();
  await button('Settings').click();await $('dialog').waitForDisplayed();
  for(let i=0;i<18;i++){await browser.keys('Tab');assert.equal(await evaluate(()=>document.querySelector('dialog').contains(document.activeElement)),true);}
  await changeTheme('light');await browser.waitUntil(async()=>await evaluate(()=>document.documentElement.dataset.theme)==='light');
  const contrast=await evaluate(()=>{
   function luminance(color){const rgb=color.match(/[\d.]+/g).slice(0,3).map(Number).map(v=>v/255).map(v=>v<=.04045?v/12.92:((v+.055)/1.055)**2.4);return rgb[0]*.2126+rgb[1]*.7152+rgb[2]*.0722;}
   function pair(fg,bg){const a=luminance(fg),b=luminance(bg);return (Math.max(a,b)+.05)/(Math.min(a,b)+.05);}
   const dialog=getComputedStyle(document.querySelector('dialog')),hint=getComputedStyle(document.querySelector('dialog .hint')),primary=getComputedStyle(document.querySelector('.primary')),secondary=getComputedStyle(document.querySelector('dialog button'));
   return {lightBody:pair(dialog.color,dialog.backgroundColor),lightSecondary:pair(hint.color,dialog.backgroundColor),primary:pair(primary.color,primary.backgroundColor),lightButton:pair(secondary.color,secondary.backgroundColor)};
  });
  assert.ok(Object.values(contrast).every(value=>value>=4.5),JSON.stringify(contrast));
  await capture('settings-light');await browser.keys('Escape');await browser.waitUntil(async()=>!(await $('dialog').isExisting()));assert.equal(await evaluate(()=>document.activeElement.textContent.trim()),'Settings');
  const separator=await $('[role="separator"]');await separator.click();await browser.keys('ArrowLeft');await browser.waitUntil(async()=>await separator.getAttribute('aria-valuenow')==='340');await browser.keys('ArrowRight');await browser.waitUntil(async()=>await separator.getAttribute('aria-valuenow')==='330');await browser.keys('Enter');await button('Show inspector').waitForDisplayed();assert.equal(await evaluate(()=>document.activeElement.id),'toggle-inspector');await button('Show inspector').click();
  // Native pointer actions verify the divider rather than injecting a width.
  const box=await evaluate(()=>{const r=document.querySelector('[role="separator"]').getBoundingClientRect();return {x:Math.round(r.x+r.width/2),y:Math.round(r.y+40),height:r.height};});
  assert.ok(box.height>100,'Inspector divider must fill the grid row');
  await browser.action('pointer').move({x:box.x,y:box.y}).down().move({x:box.x-40,y:box.y,duration:150}).up().perform();await browser.waitUntil(async()=>await $('[role="separator"]').getAttribute('aria-valuenow')==='370');
  await button('Settings').click();await changeTheme('dark');await $('//label[contains(.,"Reduce motion")]/input').click();await $('//label[contains(.,"Stronger keyboard")]/input').click();await button('Close settings').click();
  assert.equal(await evaluate(()=>document.documentElement.dataset.reducedMotion),'true');assert.equal(await evaluate(()=>getComputedStyle(document.querySelector('.ledger')).transitionDuration),'0s');
  await capture('workspace-dark');
  // Narrow desktop and enlarged content must not introduce document-wide overflow.
  await browser.setWindowSize(1800,1400);await evaluate(()=>{document.body.style.zoom='1.25';});
  assert.equal(await evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1),true);await capture('workspace-narrow-scaled');await evaluate(()=>{document.body.style.zoom='';});await browser.setWindowSize(2974,2180);
  await button('Settings').click();await $('//label[contains(.,"Reduce motion")]/input').click();await button('Close settings').click();
  const timings=await evaluate(async()=>{
   const samples=[],frames=[];
   for(let i=0;i<12;i++){
    const start=performance.now();document.querySelector('.collapse').click();
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));samples.push(performance.now()-start);
    let last=performance.now();await new Promise(resolve=>{function step(now){frames.push(now-last);last=now;if(now-start<240)requestAnimationFrame(step);else resolve();}requestAnimationFrame(step);});
   }
   const sorted=samples.slice().sort((a,b)=>a-b),frameSorted=frames.filter(x=>x>0).sort((a,b)=>a-b);
   return {sidebarInputToTwoFramesMs:samples,sidebarP95Ms:sorted[Math.ceil(sorted.length*.95)-1],frameP95Ms:frameSorted[Math.ceil(frameSorted.length*.95)-1],frames:frames.length,jsHeapBytes:performance.memory?.usedJSHeapSize??null};
  });
  const rssWarm=rss(),rssCheckpoints=[];
  for(let round=0;round<3;round++){for(let i=0;i<20;i++){await $('nav button:nth-child('+((i%4)+1)+')').click();await browser.waitUntil(async()=>!((await $('main').getText()).includes('Reading the case…')));}rssCheckpoints.push(rss());}
  const resourceAndPreferences=await evaluate(()=>({externalResources:performance.getEntriesByType('resource').map(r=>r.name).filter(name=>/^https?:/.test(name)&&!['localhost','127.0.0.1','ipc.localhost'].includes(new URL(name).hostname)),preferenceKeys:Object.keys(JSON.parse(localStorage.getItem('kokin.display.v1')))}));
  assert.deepEqual(resourceAndPreferences.externalResources,[]);assert.ok(!resourceAndPreferences.preferenceKeys.includes('casePath'));
  const output={method:'12 sidebar toggles; event to two animation frames; frame intervals during 240ms transitions; RSS before warm-up and after three batches of 20 paginated navigation changes. Single offline native Mac run, not a startup/leak benchmark.',viewport:await evaluate(()=>[innerWidth,innerHeight]),frontendNavigation:await evaluate(()=>{const n=performance.getEntriesByType('navigation')[0];return n?{domInteractiveMs:n.domInteractive,domContentLoadedMs:n.domContentLoadedEventEnd,loadMs:n.loadEventEnd}:null;}),contrast,...timings,rssBefore,rssWarm,rssCheckpoints,rssAfter:rss(),...resourceAndPreferences};
  if(process.env.KOKIN_UI_CAPTURE_DIR)await writeFile(join(process.env.KOKIN_UI_CAPTURE_DIR,'performance.json'),JSON.stringify(output,null,2));
  await button('Settings').click();await button('Reset display preferences').click();await button('Close settings').click();await button('Close case').click();
 });
});
