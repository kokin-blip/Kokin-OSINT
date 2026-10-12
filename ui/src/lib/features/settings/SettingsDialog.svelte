<script lang="ts">
 import ErrorNotice from '../../ErrorNotice.svelte';
 import {asCommandError,type CommandError} from '../../bindings';
 import {onMount} from 'svelte';
 import {preferences,type Theme} from '../../stores/preferences.svelte';
 let {onclose,casePath,online,onnetwork}:{onclose:()=>void;casePath?:string;online?:boolean;onnetwork?:(online:boolean)=>Promise<void>}=$props();
 let dialog:HTMLDialogElement;
 let networkBusy=$state(false);let networkError=$state<CommandError|null>(null);
 onMount(()=>{const previous=document.activeElement;dialog.showModal();return()=>{dialog.close();if(previous instanceof HTMLElement&&previous.isConnected)previous.focus();};});
 async function switchNetwork(){if(!onnetwork)return;networkBusy=true;networkError=null;try{await onnetwork(!online);}catch(raw){networkError=asCommandError(raw);}finally{networkBusy=false;}}
</script>
<dialog bind:this={dialog} aria-labelledby="settings-title" onclose={onclose}>
 <header><h2 id="settings-title">Settings</h2><button type="button" onclick={onclose}>Close settings</button></header>
 <p class="hint">Display preferences stay on this device. Case data, credentials and network mode are never saved in these preferences.</p>
 {#if preferences.storageError}<p role="status">Preferences are active for this session but could not be saved.</p>{/if}
 <section aria-labelledby="appearance-title"><h3 id="appearance-title">Appearance</h3>
  <label>Theme<select value={preferences.value.theme} onchange={e=>preferences.update({theme:e.currentTarget.value as Theme})}><option value="dark">Night Ledger dark</option><option value="light">Light</option><option value="system">Use system appearance</option></select></label>
  <label class="check"><input type="checkbox" checked={preferences.value.showArtwork} onchange={e=>preferences.update({showArtwork:e.currentTarget.checked})}/>Show decorative case artwork</label>
 </section>
 <section aria-labelledby="accessibility-title"><h3 id="accessibility-title">Accessibility and motion</h3>
  <label class="check"><input type="checkbox" checked={preferences.value.reduceMotion} onchange={e=>preferences.update({reduceMotion:e.currentTarget.checked})}/>Reduce motion</label>
  <p class="hint">The operating system’s reduced-motion preference always takes priority.</p>
  <label class="check"><input type="checkbox" checked={preferences.value.strongFocus} onchange={e=>preferences.update({strongFocus:e.currentTarget.checked})}/>Stronger keyboard focus indicators</label>
  <p class="hint">Resize the inspector with Left/Right arrows; Shift changes width in larger steps. Home/End select its minimum/maximum width.</p>
 </section>
 <section aria-labelledby="storage-title"><h3 id="storage-title">Storage and privacy</h3><p class="path">{casePath??'No case is open. Leave the new-case location blank to use the app’s local data folder.'}</p><p>Case contents are encrypted locally. No telemetry, external fonts or remote interface assets are loaded.</p></section>
 <section aria-labelledby="network-title"><h3 id="network-title">Offline mode and connectors</h3><p>{#if online===undefined}Collection controls are available after opening a case.{:else}Collection is {online?'enabled':'offline'}. Every collection still requires a URL and explicit consent.{/if}</p>{#if onnetwork}<button type="button" disabled={networkBusy} onclick={switchNetwork}>{online?'Switch offline':'Enable online collection'}</button>{/if}{#if networkError}<ErrorNotice error={networkError}/>{/if}<p class="hint">Additional connectors and AI configuration are planned; no model is installed or downloaded here.</p></section>
 <section aria-labelledby="performance-title"><h3 id="performance-title">Layout and performance</h3><p>Lists stay paginated. Hide case artwork to reduce decorative image rendering. Inspector width: {preferences.value.inspectorWidth} pixels.</p><button type="button" onclick={()=>preferences.update({inspectorWidth:330})}>Reset inspector width</button></section>
 <footer><button type="button" onclick={()=>preferences.reset()}>Reset display preferences</button><p class="hint">Kokin-OSINT 0.0.1 · Experimental</p></footer>
</dialog>
<style>
 dialog{width:min(620px,calc(100vw - 40px));max-height:calc(100vh - 48px);padding:24px;border:1px solid var(--border-strong);border-radius:var(--radius-panel);background:var(--surface-panel);color:var(--foreground);overflow:auto;animation:ledger-enter var(--motion-disclosure) var(--motion-ease);}
 dialog::backdrop{background:#0009;}header{display:flex;justify-content:space-between;align-items:center;gap:16px;}h2{margin:0;font-size:24px;}h3{font-size:16px;margin:0 0 12px;}section{padding:20px 0;border-bottom:1px solid var(--border);}label{display:grid;gap:8px;margin:12px 0;}.check{display:flex;align-items:center;gap:10px;}.check input{flex:0 0 auto;}.hint{color:var(--foreground-secondary);font-size:12px;}.path{overflow-wrap:anywhere;}footer{margin-top:20px;}
</style>
