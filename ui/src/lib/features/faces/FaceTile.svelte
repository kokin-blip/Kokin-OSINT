<script lang="ts">
 import type {FaceView} from '../../bindings';
 let {face,selected=false,onselect,onopen}:{face:FaceView;selected?:boolean;onselect?:(face:FaceView)=>void;onopen?:(face:FaceView)=>void}=$props();
 const time=(ms:number)=>`${Math.floor(ms/60000)}:${(ms/1000%60).toFixed(1).padStart(4,'0')}`;
</script>
<figure class:selected>
 <button type="button" class="portrait" aria-label={`Select ${face.entity_name??'unlabelled face'}${face.timestamp_ms!==null?' at '+time(face.timestamp_ms):' on page '+(face.page+1)}`} aria-pressed={selected} onclick={()=>onselect?.(face)}>
  {#if face.crop_png}<img src={`data:image/png;base64,${face.crop_png}`} alt="Aligned face crop from source evidence"/>{:else}<span>Crop unavailable</span>{/if}
 </button>
 <figcaption><strong>{face.entity_name??'Unlabelled face'}</strong><span>{face.timestamp_ms!==null?time(face.timestamp_ms):`Page ${face.page+1}`}{face.stale?' · stale':''}</span><button type="button" onclick={()=>onopen?.(face)}>Open source</button></figcaption>
</figure>
<style>
 figure{margin:0;width:132px;flex:0 0 132px}button.portrait{display:block;width:132px;height:132px;padding:8px;border:1px solid var(--border);background:var(--surface-panel);border-radius:var(--radius-panel)}.selected .portrait{outline:2px solid var(--accent);outline-offset:2px}.portrait:hover{background:var(--surface-raised)}.portrait img{width:112px;height:112px;object-fit:contain}figcaption{display:grid;gap:6px;padding:10px 0;font-size:12px;overflow-wrap:anywhere}figcaption strong{font-size:13px}figcaption span{color:var(--foreground-secondary)}figcaption button{text-align:left;padding:4px 0;background:transparent;border:0;text-decoration:underline;text-underline-offset:3px}button:focus-visible{outline:2px solid var(--accent);outline-offset:3px}
</style>
