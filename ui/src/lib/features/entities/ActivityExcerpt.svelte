<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from '../../bindings';
  import ErrorNotice from '../../ErrorNotice.svelte';
  let { onopen, onhistory }: { onopen: (kind: string, id: string) => void; onhistory: () => void } = $props();
  let activity = $state<api.ActivityView | null>(null);
  let error = $state<api.CommandError | null>(null);
  let live = true;
  onMount(()=>{void api.activityPage().then(result=>{if(live)activity=result;}).catch(raw=>{if(live)error=api.asCommandError(raw);});return()=>{live=false;};});
</script>
<section aria-label="Recent case activity"><h4>Activity</h4>
  {#if error}<ErrorNotice {error}/>{:else if !activity}<p role="status">Reading recent case activity…</p>{:else}
    <ol>{#each activity.page.rows.slice(0,3) as event (event.id)}<li>{#if event.subject_id && ['entity','identifier','relationship','observation','artifact'].includes(event.subject_kind??'')}<button class="event-title" type="button" title="Open recorded subject" onclick={()=>onopen(event.subject_kind!,event.subject_id!)}>{event.action.replaceAll('.',' ').replaceAll('_',' ')}</button>{:else}<strong>{event.action.replaceAll('.',' ').replaceAll('_',' ')}</strong>{/if}<time>{event.occurred_utc}</time><span>{event.actor}</span></li>{:else}<li>No activity recorded.</li>{/each}</ol>
    <p class="chain">Audit chain: {activity.chain_status}</p><p class="hint">Showing {Math.min(3,activity.page.rows.length)} recent events of {activity.page.total}. Tamper-evident, not chain of custody.</p><button type="button" onclick={onhistory}>View activity history</button>
  {/if}
</section>
<style>
  section { border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface-panel); padding: 20px; height: 100%; min-width: 0; }
  h4 { font-size: 14px; margin: 0 0 20px; }
  ol { list-style: none; padding: 0 0 0 12px; margin: 0; border-left: 1px solid var(--border-strong); }
  li { position: relative; padding: 0 0 24px 8px; overflow-wrap: anywhere; }
  li::before { content: ''; position: absolute; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); left: -17px; top: 6px; }
  strong, time, span { display: block; }
  time, span, .hint { color: var(--foreground-secondary); font-size: 12px; }
  .event-title { text-align: left; text-transform: capitalize; font-size: 14px; font-weight: 600; border: none; background: transparent; padding: 0; min-height: 24px; color: var(--foreground); }
  .chain { font-size: 12px; } .hint { margin-bottom: 16px; }
</style>
