<script lang="ts">
  import {onMount} from "svelte";
  import * as api from "./bindings";
  import ErrorNotice from "./ErrorNotice.svelte";
  let passphrase=$state("");let busy=$state(false);let error=$state<api.CommandError|null>(null);let result=$state("");let live=true;
  onMount(()=>()=>{live=false;});
  async function restore(){busy=true;error=null;result="";try{const state=await api.sessionStatus();const selection=await api.selectPath("package",state.session_generation);if(!selection)return;const destination=await api.selectPath("restore",state.session_generation);if(!destination)return;const id=await api.startOperation("restore",{selection,destination,passphrase},state.session_generation);let view=await api.operationStatus(id);while(live && view.status==="running"){await new Promise(r=>setTimeout(r,300));view=await api.operationStatus(id);}if(!live)return;if(view.status==="completed")result=JSON.parse(view.result_json??"{}").root;else error=view.error;}catch(raw){if(live)error=api.asCommandError(raw);}finally{passphrase="";if(live)busy=false;}}
</script>
<details><summary>Restore an exported case</summary><p>Choose an encrypted package and a new folder. Existing cases are never replaced. Packages have no recovery key or digital signature; use the export passphrase.</p><label>Export passphrase<input type="password" bind:value={passphrase} autocomplete="off"/></label><button type="button" disabled={busy||!passphrase} onclick={restore}>{busy?"Restoring…":"Choose package and restore"}</button>{#if error}<ErrorNotice {error}/>{/if}{#if result}<p role="status">Restored to {result}. Open this folder above with the export passphrase.</p>{/if}</details>
<style>details{margin-top:2rem;max-width:40rem}label{display:block;margin:.75rem 0}input{display:block;width:100%}p{overflow-wrap:anywhere}</style>
