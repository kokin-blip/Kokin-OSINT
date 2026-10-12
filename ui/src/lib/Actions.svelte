<script lang="ts">
  import type {EntityView} from './bindings';
  import OperationsPanel from './features/operations/OperationsPanel.svelte';
  let {route,entity,generation,onchange,evidenceId=$bindable(''),online=$bindable(false)}:{route:{kind:string;id:string}|null;entity:EntityView|null;generation:number;onchange:()=>void;evidenceId?:string;online?:boolean}=$props();
  let panel=$state<{importFile:()=>Promise<void>}|undefined>();
  export async function importFile(){await panel?.importFile();}
</script>
<OperationsPanel bind:this={panel} {route} {entity} {generation} {onchange} bind:evidenceId bind:online/>
