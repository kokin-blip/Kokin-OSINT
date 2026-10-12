<script lang="ts">import type {ActivityView} from '../../bindings';let {activity,onopen}:{activity:ActivityView;onopen:(route:{kind:string;id:string})=>void}=$props();</script>

      <aside class="coverage">
<strong>Audit chain: {activity.chain_status}</strong>
<p>{activity.chain_detail}</p>
<p>Tamper-evident, not chain of custody. Anyone holding the case key can rewrite this history.</p>
</aside>
      {#each activity.page.rows as event (event.id)}
        <article>
<strong>{event.action}</strong>
<p>{event.occurred_utc} · {event.actor}</p>
          {#if event.subject_id && ["entity","identifier","relationship","observation","artifact"].includes(event.subject_kind??"")}<button type="button" onclick={()=>onopen({kind:event.subject_kind!,id:event.subject_id!})}>Open subject</button>{/if}
          <details>
<summary>Recorded detail</summary>
<pre>{event.payload_json}</pre>
</details>
        </article>
      {/each}
<style>
.coverage{border:1px solid var(--border);background:var(--surface-raised);border-radius:var(--radius-control);padding:16px;margin-bottom:20px}
  .coverage p,article p{color:var(--foreground-secondary)}
  article{padding:16px 0;border-bottom:1px solid var(--border)}
  pre{overflow-wrap:anywhere;white-space:pre-wrap;max-height:320px;overflow:auto}
  details{margin-top:12px}

</style>
