<script lang="ts">
  import EntityDossier from './features/entities/EntityDossier.svelte';
  import type { EntityView } from './bindings';
  let { entity, onevidence }: { entity: EntityView; onevidence?: (kind: string, id: string) => void } = $props();
</script>
<EntityDossier {entity} {onevidence}/>
