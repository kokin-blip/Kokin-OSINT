<script lang="ts">
 import * as api from '../../bindings';
 let {artifact,generation,page,onface}:{artifact:string;generation:number;page:number;onface?:(face:api.FaceView)=>void}=$props();
 let faces=$state<api.FaceView[]>([]),error=$state(''),total=$state(0);
 $effect(()=>{const currentPage=page;let live=true;void api.facePage(generation,artifact,0,false,currentPage).then(v=>{if(live){faces=v.faces;total=v.total;}}).catch(()=>{if(live)error='Face overlays unavailable; open Faces and related evidence to retry.';});return()=>{live=false;};});
</script>
{#if faces.length}<svg viewBox={`0 0 ${faces[0].image_size[0]} ${faces[0].image_size[1]}`} preserveAspectRatio="none" aria-label="Detected face locations"><title>Detected faces; select a boundary to search</title>{#each faces as face}<rect x={face.bbox[0]} y={face.bbox[1]} width={face.bbox[2]} height={face.bbox[3]} role="button" tabindex="0" aria-label={`Search ${face.entity_name??'detected face'}`} onclick={()=>onface?.(face)} onkeydown={e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();onface?.(face);}}}/>{/each}</svg>{/if}
{#if total>50}<span class="overlay-error">Showing 50 of {total} faces on this page; use face search to browse all detections.</span>{/if}
{#if error}<span class="overlay-error">{error}</span>{/if}
<style>svg{position:absolute;inset:0;width:100%;height:100%;pointer-events:none}rect{fill:transparent;stroke:var(--accent);stroke-width:2;vector-effect:non-scaling-stroke;cursor:pointer;pointer-events:all}rect:hover,rect:focus-visible{fill:color-mix(in srgb,var(--accent) 15%,transparent);stroke-width:4;outline:none}.overlay-error{position:absolute;bottom:0;font-size:12px;background:var(--surface-panel);color:var(--foreground-secondary)}</style>
