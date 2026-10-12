<script lang="ts">let {url=$bindable(''),confirmed=$bindable(false),busy,online,onsubmit}:{url?:string;confirmed?:boolean;online:boolean;busy:boolean;onsubmit:(event:SubmitEvent)=>void}=$props();</script>

      <form onsubmit={onsubmit}>
<label>Public URL<input type="url" required bind:value={url}/>
</label>
<p>This sends the URL, including its query, and a Kokin user-agent to the named host. Static HTML only; no browser cookies or scripts. Cross-host redirects are refused.</p>
<label class="check">
<input type="checkbox" bind:checked={confirmed}/>I want to contact this public destination</label>
<button disabled={busy||!online||!confirmed}>Collect page</button>
</form>

<style>

form{display:grid;gap:12px;margin-top:16px}
  label{display:grid;gap:6px}
  .check{display:flex;gap:8px;align-items:center}
  p{color:var(--foreground-secondary);line-height:1.6}


</style>
