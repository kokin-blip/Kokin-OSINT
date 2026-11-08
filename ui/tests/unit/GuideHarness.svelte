<script lang="ts">
 import {createGuide} from '../../src/lib/features/onboarding/guide.svelte';
 import InvestigationGuide from '../../src/lib/features/onboarding/InvestigationGuide.svelte';
 export const guide=createGuide(()=>1);
</script>
<InvestigationGuide {guide} onnavigate={()=>{}} onimport={()=>{}}/>
