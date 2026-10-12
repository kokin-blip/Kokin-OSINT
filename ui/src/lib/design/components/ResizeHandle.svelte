<script lang="ts">
 import {onMount} from 'svelte';
 let {value=$bindable(330),oncommit,controls,oncollapse}:{value?:number;oncommit:(value:number)=>void;controls?:string;oncollapse?:()=>void}=$props();
 let start:{x:number;value:number;mouse:boolean}|null=null;
 const clamp=(n:number)=>Math.min(480,Math.max(260,Math.round(n)));
 function begin(event:PointerEvent){if(event.button!==0)return;event.preventDefault();event.currentTarget instanceof HTMLElement&&event.currentTarget.focus();start={x:event.clientX,value,mouse:false};(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);}
 function move(event:PointerEvent){if(start&&!start.mouse)value=clamp(start.value+start.x-event.clientX);}
 function mouseBegin(event:MouseEvent){if(start||event.button!==0)return;event.preventDefault();(event.currentTarget as HTMLElement).focus();start={x:event.clientX,value,mouse:true};}
 onMount(()=>{
  const move=(event:MouseEvent)=>{if(start?.mouse)value=clamp(start.value+start.x-event.clientX);};
  const up=()=>{if(start?.mouse)finish();};
  window.addEventListener('mousemove',move);window.addEventListener('mouseup',up);window.addEventListener('blur',cancel);
  return()=>{window.removeEventListener('mousemove',move);window.removeEventListener('mouseup',up);window.removeEventListener('blur',cancel);cancel();};
 });
 function finish(){if(start){start=null;oncommit(value);}}
 function cancel(){if(start){value=start.value;start=null;}}
 function key(event:KeyboardEvent){const delta=event.shiftKey?40:10;let next:number;
  if(event.key==='Enter'&&oncollapse){event.preventDefault();oncollapse();return;}
  if(event.key==='ArrowLeft')next=value+delta;else if(event.key==='ArrowRight')next=value-delta;else if(event.key==='Home')next=260;else if(event.key==='End')next=480;else return;
  event.preventDefault();value=clamp(next);oncommit(value);
 }
</script>
<!-- Focusable separator implements the ARIA APG window splitter pattern. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div class="resize" role="separator" aria-label="Inspector" aria-controls={controls} title="Resize inspector with arrow keys; Enter hides the inspector" aria-orientation="vertical" aria-valuemin="260" aria-valuemax="480" aria-valuenow={value} aria-valuetext={`${value} pixels`} tabindex="0" onmousedown={mouseBegin} onpointerdown={begin} onpointermove={move} onpointerup={finish} onpointercancel={cancel} onlostpointercapture={finish} onkeydown={key}></div>
<style>
 .resize{flex:1;cursor:col-resize;touch-action:none;min-width:12px;align-self:stretch;position:relative;}
 .resize::after{content:'';position:absolute;top:16px;bottom:16px;left:5px;width:2px;background:var(--border);border-radius:2px;transition:background var(--motion-feedback);}
 .resize:hover::after,.resize:focus-visible::after{background:var(--accent);}
</style>
