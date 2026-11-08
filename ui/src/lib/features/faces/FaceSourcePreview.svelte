<script lang="ts">
 import * as api from '../../bindings';
 import ErrorNotice from '../../ErrorNotice.svelte';
 let {generation,face,onselect}:{generation:number;face:api.FaceView;onselect?:(face:api.FaceView)=>void}=$props();
 let preview=$state<api.FacePreviewView|null>(null),loading=$state(false),error=$state<api.CommandError|null>(null);
 $effect(()=>{const id=face.id;let live=true;loading=true;error=null;preview=null;void api.facePreview(generation,id).then(v=>{if(live)preview=v;}).catch(raw=>{if(live)error=api.asCommandError(raw);}).finally(()=>{if(live)loading=false;});return()=>{live=false;};});
</script>
<section aria-label="Located face source preview"><h4>Source {face.timestamp_ms!==null?`at ${(face.timestamp_ms/1000).toFixed(2)} seconds`:`page ${face.page+1}`}</h4>
 {#if loading}<p role="status">Rendering source locally…</p>{/if}{#if error}<ErrorNotice {error}/>{/if}
 {#if preview}<div class="source"><img src={`data:image/png;base64,${preview.preview_png}`} alt="Oriented evidence source with selected face location"/><svg viewBox={`0 0 ${face.image_size[0]} ${face.image_size[1]}`} preserveAspectRatio="none" aria-label="Detected face boundary"><title>Detected face location in the source</title><rect x={face.bbox[0]} y={face.bbox[1]} width={face.bbox[2]} height={face.bbox[3]}/></svg></div><button type="button" onclick={()=>onselect?.(face)}>Use this face as query</button>{/if}
</section>
<style>section{margin:24px 0}h4{margin:0 0 12px}.source{position:relative;width:fit-content;max-width:100%}img{display:block;max-width:100%;max-height:650px;object-fit:contain}svg{position:absolute;inset:0;width:100%;height:100%;pointer-events:none}rect{fill:transparent;stroke:var(--accent);stroke-width:3;vector-effect:non-scaling-stroke}p{color:var(--foreground-secondary)}button{margin-top:12px}</style>
