<script lang="ts">
 import type {ToolBranch} from './directory';
 import type {DirectoryTool} from '../../bindings';
 import DirectoryTree from './DirectoryTree.svelte';
 let {branch,onselect,expanded=false,selectedId}:{branch:ToolBranch;onselect:(tool:DirectoryTool,trigger:HTMLElement)=>void;expanded?:boolean;selectedId?:string}=$props();
</script>
<ul class="branches">
 {#each branch.children as child (child.path)}<li><details open={expanded}><summary>{child.name}</summary><DirectoryTree branch={child} {onselect} {expanded} {selectedId}/></details></li>{/each}
 {#each branch.tools as tool (tool.id)}<li><button aria-pressed={selectedId===tool.id} title={tool.description} onclick={e=>onselect(tool,e.currentTarget)}>{tool.name}</button></li>{/each}
</ul>
<style>.branches{padding-left:22px;list-style:none;margin:6px 0}.branches>li{margin:6px 0}button[aria-pressed="true"]{background:var(--accent-surface);color:var(--foreground)}summary{cursor:pointer;padding:7px 0}button{text-align:left;background:transparent;border-color:transparent;font-weight:400;max-width:100%;overflow-wrap:anywhere}</style>
