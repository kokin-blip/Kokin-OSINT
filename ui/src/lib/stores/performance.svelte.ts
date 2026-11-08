/** Opt-in local timings. Never records keys, coordinates, case IDs or content. */
export interface PerformanceResult {frames:number;frameP95Ms:number|null;interactions:number;interactionP95Ms:number|null;elapsedMs:number;hiddenIntervals:number;}
const percentile=(values:number[])=>values.length?[...values].sort((a,b)=>a-b)[Math.ceil(values.length*.95)-1]:null;
class LocalPerformance {
 running=$state(false);result=$state<PerformanceResult|null>(null);private cleanup:(()=>void)|undefined;
 stop(){this.cleanup?.();this.cleanup=undefined;this.running=false;}
 start(){this.stop();this.result=null;this.running=true;const start=performance.now(),frames:number[]=[],inputs:number[]=[];let previous=0,hidden=0,frame=0;const inputFrames=new Set<number>();
  const tick=(now:number)=>{if(document.visibilityState==='visible'){if(previous)frames.push(now-previous);previous=now;}else{previous=0;hidden++;}frame=requestAnimationFrame(tick);};
  const input=()=>{if(!this.running||document.visibilityState!=='visible'||inputs.length>=1000)return;const then=performance.now();const first=requestAnimationFrame(()=>{inputFrames.delete(first);const second=requestAnimationFrame(()=>{inputFrames.delete(second);if(this.running)inputs.push(performance.now()-then);});inputFrames.add(second);});inputFrames.add(first);};
  const visibility=()=>{previous=0;if(document.visibilityState!=='visible')hidden++;};document.addEventListener('visibilitychange',visibility);
  frame=requestAnimationFrame(tick);document.addEventListener('keydown',input);document.addEventListener('pointerdown',input);document.addEventListener('click',input);
  const timer=setTimeout(()=>{this.result={frames:frames.length,frameP95Ms:percentile(frames),interactions:inputs.length,interactionP95Ms:percentile(inputs),elapsedMs:performance.now()-start,hiddenIntervals:hidden};this.stop();},5000);
  this.cleanup=()=>{clearTimeout(timer);cancelAnimationFrame(frame);inputFrames.forEach(id=>cancelAnimationFrame(id));document.removeEventListener('visibilitychange',visibility);document.removeEventListener('keydown',input);document.removeEventListener('pointerdown',input);document.removeEventListener('click',input);};
 }
}
export const localPerformance=new LocalPerformance();
