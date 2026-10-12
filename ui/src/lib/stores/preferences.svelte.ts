import {onMount} from 'svelte';
export type Theme = 'dark' | 'light' | 'system';
export interface Preferences {theme:Theme;reduceMotion:boolean;strongFocus:boolean;showArtwork:boolean;sidebarCollapsed:boolean;inspectorWidth:number;}
const KEY='kokin.display.v1';
export const defaults:Preferences={theme:'dark',reduceMotion:false,strongFocus:false,showArtwork:true,sidebarCollapsed:false,inspectorWidth:330};
export function decodePreferences(raw:string|null):Preferences {
 let value:Record<string,unknown>={};
 try {const parsed:unknown=JSON.parse(raw??'{}');if(parsed && typeof parsed==='object' && !Array.isArray(parsed))value=parsed as Record<string,unknown>;} catch { /* Invalid display preferences fall back to safe defaults. */ }
 return {
  theme:['dark','light','system'].includes(String(value.theme))?value.theme as Theme:defaults.theme,
  reduceMotion:typeof value.reduceMotion==='boolean'?value.reduceMotion:defaults.reduceMotion,
  strongFocus:typeof value.strongFocus==='boolean'?value.strongFocus:defaults.strongFocus,
  showArtwork:typeof value.showArtwork==='boolean'?value.showArtwork:defaults.showArtwork,
  sidebarCollapsed:typeof value.sidebarCollapsed==='boolean'?value.sidebarCollapsed:defaults.sidebarCollapsed,
  inspectorWidth:typeof value.inspectorWidth==='number'&&Number.isFinite(value.inspectorWidth)?Math.min(480,Math.max(260,value.inspectorWidth)):defaults.inspectorWidth,
 };
}
class DisplayPreferences {
 value=$state<Preferences>({...defaults});
 storageError=$state(false);
 load(){try{this.value=decodePreferences(localStorage.getItem(KEY));this.storageError=false;}catch{this.value={...defaults};this.storageError=true;}}
 update(change:Partial<Preferences>){this.value=decodePreferences(JSON.stringify({...this.value,...change}));try{localStorage.setItem(KEY,JSON.stringify(this.value));this.storageError=false;}catch{this.storageError=true;}}
 reset(){this.update({...defaults});}
}
export const preferences=new DisplayPreferences();
/** Installed once by App; listeners are torn down when the app unmounts. */
export function installPreferences(){
 onMount(()=>{
  preferences.load();
  const scheme=window.matchMedia('(prefers-color-scheme: light)');
  const motion=window.matchMedia('(prefers-reduced-motion: reduce)');
  function apply(){document.documentElement.dataset.theme=preferences.value.theme==='system'?(scheme.matches?'light':'dark'):preferences.value.theme;document.documentElement.dataset.reducedMotion=String(preferences.value.reduceMotion||motion.matches);document.documentElement.dataset.strongFocus=String(preferences.value.strongFocus);}
  scheme.addEventListener('change',apply);motion.addEventListener('change',apply);
  const stop=$effect.root(()=>{$effect(()=>{preferences.value;apply();});});
  apply();
  return()=>{stop();scheme.removeEventListener('change',apply);motion.removeEventListener('change',apply);};
 });
}
