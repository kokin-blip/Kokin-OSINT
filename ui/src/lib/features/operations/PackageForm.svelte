<script lang="ts">let {passphrase=$bindable(''),exclusions=$bindable(''),busy,onsubmit,onverify,onrestore}:{passphrase?:string;exclusions?:string;onverify:()=>void;onrestore:()=>void;busy:boolean;onsubmit:(event:SubmitEvent)=>void}=$props();</script>

      <form onsubmit={onsubmit}>
<label>Package passphrase<input type="password" required bind:value={passphrase} autocomplete="new-password"/>
</label>
<label>Exclude document content hashes (optional)<textarea bind:value={exclusions}>
</textarea>
</label>
<button disabled={busy}>Export to a new package</button>
<button type="button" disabled={busy} onclick={onverify}>Verify a package</button>
<button type="button" disabled={busy||!passphrase} onclick={onrestore}>Restore to a new case directory</button>
</form>

<style>

form{display:grid;gap:12px;margin-top:16px}
  label{display:grid;gap:6px}
  textarea{min-height:80px;resize:vertical}


</style>
