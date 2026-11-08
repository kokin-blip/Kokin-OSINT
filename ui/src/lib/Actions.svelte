<script lang="ts">
 import type {EntityView} from './bindings';
 import OperationsPanel from './features/operations/OperationsPanel.svelte';
 import type {CaseOperations} from './features/operations/controller.svelte';
 let {route,entity,operations,showPackages=true}:{route:{kind:string;id:string}|null;entity:EntityView|null;operations:CaseOperations;showPackages?:boolean}=$props();
 export async function importFile(){await operations.importFile();}
</script>
<OperationsPanel {route} {entity} {operations} {showPackages}/>
