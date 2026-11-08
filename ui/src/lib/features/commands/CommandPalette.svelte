<script lang="ts">
 import {onMount,tick} from 'svelte';
 export interface Command {id:string;label:string;description:string;keywords?:string;disabled?:string;run:()=>void|Promise<void>;}
 let {commands,onclose,onerror}:{commands:Command[];onclose:()=>void;onerror:(error:unknown)=>void}=$props();
 let dialog:HTMLDialogElement;let input:HTMLInputElement;let query=$state('');
 let filtered=$derived(commands.filter(command=>query.trim().toLowerCase().split(/\s+/).every(word=>(command.label+' '+command.description+' '+(command.keywords??'')).toLowerCase().includes(word))));
 onMount(()=>{const previous=document.activeElement;dialog.showModal();input.focus();return()=>{dialog.close();if(previous instanceof HTMLElement&&previous.isConnected)previous.focus();};});
 async function choose(command:Command){if(command.disabled)return;onclose();await tick();try{await command.run();}catch(raw){onerror(raw);}}
 function keyboard(event:KeyboardEvent){
  if(event.altKey||event.ctrlKey||event.metaKey)return;
  const choices=[...dialog.querySelectorAll<HTMLButtonElement>('.commands button:not(:disabled)')];
  if(event.key==='ArrowDown'||event.key==='ArrowUp'){
   event.preventDefault();if(!choices.length)return;const current=choices.indexOf(document.activeElement as HTMLButtonElement);const delta=event.key==='ArrowDown'?1:-1;
   choices[current<0?(delta>0?0:choices.length-1):(current+delta+choices.length)%choices.length].focus();
  }else if(event.key==='Enter'&&event.target===input){event.preventDefault();const command=filtered.find(c=>!c.disabled);if(command)void choose(command);}
 }
</script>
<dialog bind:this={dialog} aria-labelledby="palette-title" onclose={onclose} onkeydown={keyboard}>
 <header><h2 id="palette-title">Commands</h2><button type="button" onclick={onclose}>Close commands</button></header>
 <label>Find a command<input bind:this={input} type="search" bind:value={query} placeholder="Search actions, navigation or settings" autocomplete="off"/></label>
 <p class="hint">Arrow keys choose an action. Enter runs it. Escape closes. These are app commands; use case search to find evidence.</p>
 <ul class="commands">{#each filtered as command (command.id)}<li><button type="button" disabled={Boolean(command.disabled)} onclick={()=>void choose(command)}><strong>{command.label}</strong><span>{command.disabled??command.description}</span></button></li>{/each}</ul>
 {#if !filtered.length}<p role="status">No commands match. Try evidence, graph, import or settings.</p>{/if}
</dialog>
<style>
 dialog{width:min(600px,calc(100vw - 40px));max-height:calc(100vh - 48px);background:var(--surface-panel);color:var(--foreground);border:1px solid var(--border-strong);border-radius:var(--radius-panel);padding:24px;overflow:auto;animation:ledger-enter var(--motion-disclosure) var(--motion-ease);}
 dialog::backdrop{background:#0009;}header{display:flex;justify-content:space-between;align-items:center;gap:16px;}h2{font-size:24px;margin:0;}label{display:grid;gap:8px;margin-top:20px;}input{width:100%;}.hint{font-size:12px;color:var(--foreground-secondary);}ul{list-style:none;margin:16px 0 0;padding:0;display:grid;gap:8px;}li button{width:100%;text-align:left;padding:12px;background:transparent;}li button span{display:block;color:var(--foreground-secondary);font-size:12px;margin-top:4px;}
</style>
