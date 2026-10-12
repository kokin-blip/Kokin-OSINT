<script lang="ts">import type {SubjectView} from '../../bindings';let {subject,onopen,onevidence}:{subject:SubjectView;onopen:(kind:string,id:string)=>void;onevidence:(kind:string,id:string)=>void}=$props();</script>

<section aria-label="Selected subject">        <h3>{subject.title}</h3>
<p>{subject.subject_kind} · {subject.subject_id}</p>
        <dl>{#each subject.fields as [key,value]}<dt>{key.replaceAll("_"," ")}</dt>
<dd>{#if ["entity","from_entity","to_entity"].includes(key)}<button type="button" onclick={()=>onopen('entity',value)}>{value}</button>{:else}{value}{/if}</dd>{/each}</dl>
        <h4>Supporting and opposing evidence</h4>{#each subject.evidence as e}<button type="button" onclick={()=>onevidence(e.evidence_kind,e.evidence_id)}>{e.role}: {e.evidence_kind} {e.evidence_id}</button>{/each}
        <h4>Confidence dimensions</h4>{#each subject.confidence as dimension}<p>
<strong>{dimension.title}</strong>: {dimension.agreement}</p>{#each dimension.values as value}<p>{value.label} · {value.actor} · {value.assessed_utc}<br/>{value.contributing_factors_json}</p>{/each}{/each}</section>

<style>

section{padding:20px;border:1px solid var(--border);border-radius:var(--radius-panel);background:var(--surface-panel)}
  h3{margin:0 0 8px;font-size:24px}
  h4{margin-top:24px}
  dl{display:grid;grid-template-columns:minmax(100px,1fr) 2fr;gap:12px}
  dt{color:var(--foreground-secondary)}
  dd{margin:0;overflow-wrap:anywhere}
  p{overflow-wrap:anywhere}
  button{margin:4px 4px 4px 0}
  @media(max-width:1100px){dl{grid-template-columns:1fr}
  }


</style>
