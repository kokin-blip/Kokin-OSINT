<script lang="ts">
  import type { CaseView } from '../bindings';
  import Panel from '../design/components/Panel.svelte';
  import Badge from '../design/components/Badge.svelte';
  import Icon from '../design/components/Icon.svelte';
  import banner from '../design/assets/night-ledger-banner.png';
  let { caseView, online, evidenceTotal, onimport }: { caseView?: CaseView; online: boolean; evidenceTotal: number | null; onimport?:()=>void } = $props();
</script>
<Panel label="Active investigation">
  <img src={banner} alt="" width="600" height="200" loading="eager" decoding="sync"/>
  <h4 title={caseView?.case_id}>{caseView?.case_id ?? 'Current case'}</h4>
  <p class="path" title={caseView?.root}>{caseView?.root ?? 'Local investigation workspace'}</p>
  <div class="state"><Icon name="document-text"/><span>{evidenceTotal === null ? 'Evidence count not loaded' : `${evidenceTotal} evidence ${evidenceTotal===1?'file':'files'}`}</span></div>
  <div class="state"><Icon name="lock-closed"/><span>Case unlocked · <Badge tone={online ? 'warning' : 'success'}>{online ? 'Online collection enabled' : 'Offline'}</Badge></span></div>
  <p class="hint">Evidence and decisions are stored locally.</p>
  {#if onimport}<button type="button" class="primary import" onclick={onimport}><Icon name="plus"/>Import evidence</button>{/if}
</Panel>
<style>
  img { display: block; width: 100%; height: auto; aspect-ratio: 3 / 1; object-fit: cover; border-radius: 10px; margin-bottom: 16px; }
  h4 { margin: 0 0 4px; font-size: 17px; overflow-wrap: anywhere; }
  .path { color: var(--foreground-secondary); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin: 0 0 20px; }
  .state { display: flex; gap: 12px; align-items: center; margin: 12px 0; }
  .hint { color: var(--foreground-muted); font-size: 12px; margin: 16px 0 0; }
  .import { display: flex; align-items: center; justify-content: center; gap: 8px; width: 100%; margin-top: 20px; }
</style>
