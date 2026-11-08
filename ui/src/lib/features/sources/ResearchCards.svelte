<script lang="ts">
 import * as api from '../../bindings';
 let {fields,onerror,onreview}:{fields:Record<string,unknown>;onerror:(error:unknown)=>void;onreview?:(index:number,action:'create_entity'|'add_identifier'|'relate')=>void}=$props();
 interface Card {module:string;kind:string;fields:Record<string,unknown>;origins?:Record<string,string>;provider_assertion?:boolean;}
 let coverage=$derived(fields.coverage&&typeof fields.coverage==='object'?fields.coverage as Record<string,unknown>:null);
 let cards=$derived(Array.isArray(fields.cards)?fields.cards as Card[]:[]);
 const display=(v:unknown)=>v===null?'Unknown':typeof v==='object'?JSON.stringify(v):String(v);
 function url(v:unknown){if(typeof v!=='string')return false;try{const u=new URL(v);return ['http:','https:'].includes(u.protocol)&&!u.username&&!u.password;}catch{return false;}}
</script>
{#if fields.usage&&typeof fields.usage==='object'}{@const usage=fields.usage as Record<string,unknown>}<section aria-label="Provider usage observation"><h4>Provider usage</h4><p>State: {display(usage.state)} · Credits: {typeof usage.credits==='number'?usage.credits:'Unknown'} · Eligibility: {display(usage.eligibility)}</p><p>{display(usage.coverage)}</p></section>{/if}
{#if cards.length}<section class="cards" aria-label="Normalized source profiles">{#each cards as card,i}<details><summary>{card.module} · {card.kind.replaceAll('_',' ')} · profile {i+1}{card.provider_assertion?' · provider assertion':''}</summary>
 <dl>{#each Object.entries(card.fields) as [key,value]}<dt>{key.replaceAll('_',' ')}</dt><dd>{display(value)}{#if ['verified','private'].includes(key)&&value===null} · not supplied{/if}{#if ['profile_url','website','picture_url','banner_url'].includes(key)&&url(value)} <button onclick={()=>api.researchOpenUrl(String(value)).catch(onerror)}>Open {key.replaceAll('_',' ')} externally</button>{/if}{#if card.origins?.[key]}<small>Evidence JSON pointer: <code>{card.origins[key]}</code></small>{/if}</dd>{/each}</dl>
 {#if onreview&&card.kind==='account'&&card.provider_assertion}<div class="actions"><button onclick={()=>onreview?.(i,'create_entity')}>Review provider account record</button><button onclick={()=>onreview?.(i,'add_identifier')}>Review provider identifier</button><button onclick={()=>onreview?.(i,'relate')}>Review provider relationship</button></div>{/if}
 <p>Unlisted metadata is unknown. Source assertions require review. Remote images load only when explicitly opened in your browser.</p>
 </details>{/each}</section>{/if}
{#if coverage}<div class="coverage"><p>{coverage.modules??'Unknown'} source modules · {coverage.unclassified_modules??'Unknown'} unclassified · {coverage.omitted_items??0} omitted items · {coverage.malformed_fields??0} unsupported fields.</p>{#if typeof coverage.breach_coverage==='string'}<p>{coverage.breach_coverage}</p>{/if}</div>{:else if fields.coverage}<p class="coverage">{display(fields.coverage)}</p>{/if}
<style>
 .cards{display:grid;gap:12px;margin-block:12px}.cards details{padding-block:8px;border-bottom:1px solid var(--border)}dl{display:grid;grid-template-columns:minmax(100px,1fr) minmax(0,3fr);gap:8px 16px}dt{color:var(--foreground-secondary)}dd{margin:0;overflow-wrap:anywhere}small{display:block;color:var(--foreground-secondary)}p{max-width:75ch;color:var(--foreground-secondary)}.coverage{overflow-wrap:anywhere}@media(max-width:700px){dl{grid-template-columns:1fr}dd{margin-bottom:8px}}
</style>
