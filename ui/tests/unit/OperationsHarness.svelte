<script lang="ts">
 import {createCaseOperations} from '../../src/lib/features/operations/controller.svelte';
 let {changed}:{changed:()=>void}=$props();
 export const operations=createCaseOperations({generation:1,route:()=>null,onchange:()=>changed()});
</script>
