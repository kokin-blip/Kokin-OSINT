import type {GraphView} from '../../bindings';
import type {Camera} from './viewport';
export interface BoardState extends Record<string,unknown>{version:1;view:GraphView|null;camera:Camera;positions:Record<string,{x:number;y:number;pinned:boolean}>;hops:number;limit:number;perNode:number;direction:string;relationshipKind:string;visibleQuery?:string;roots:string[];}
export function mergeViews(previous:GraphView|null,next:GraphView):GraphView {
 const allNodes=new Map((previous?.nodes??[]).map(n=>[n.id,n]));next.nodes.forEach(n=>allNodes.set(n.id,n));
 const nodes=[...allNodes.values()].slice(0,300),ids=new Set(nodes.map(n=>n.id));
 const allEdges=new Map((previous?.edges??[]).map(e=>[e.id,e]));next.edges.forEach(e=>allEdges.set(e.id,e));
 const valid=[...allEdges.values()].filter(e=>ids.has(e.from)&&ids.has(e.to));const edges=valid.slice(0,1000);
 const reasons=new Set([...(previous?.truncation_reasons??[]),...(next.truncation_reasons??[])]);
 if(allNodes.size>300)reasons.add('board_node_limit');if(valid.length>1000)reasons.add('board_edge_limit');
 return {nodes,edges,completed_hops:next.completed_hops,truncated:Boolean(previous?.truncated||next.truncated||reasons.size),truncation_reasons:[...reasons]};
}
/** Validate stored UI state before a saved or restored case can mount a canvas. */
export function decodeBoard(payload:Record<string,unknown>):BoardState {
 if(payload.version!==1)throw new Error('This board format is unsupported.');
 const view=(payload.view??null) as GraphView|null;
 if(view&&(!Array.isArray(view.nodes)||!Array.isArray(view.edges)||view.nodes.length>300||view.edges.length>1000||view.nodes.some(n=>!n||typeof n.id!=='string'||typeof n.name!=='string'||typeof n.canonical_id!=='string'||!Number.isFinite(n.depth))||view.edges.some(e=>!e||typeof e.id!=='string'||typeof e.from!=='string'||typeof e.to!=='string'||typeof e.kind!=='string')))throw new Error('The saved graph structure is invalid.');
 const number=(value:unknown,fallback:number,min:number,max:number)=>typeof value==='number'&&Number.isFinite(value)?Math.min(max,Math.max(min,value)):fallback;
 const rawCamera=payload.camera as Partial<Camera>|undefined;
 const positions:BoardState['positions']={};for(const [id,value]of Object.entries((payload.positions??{}) as Record<string,BoardState['positions'][string]>)){if(value&&Number.isFinite(value.x)&&Number.isFinite(value.y))positions[id]={x:value.x,y:value.y,pinned:value.pinned===true};}
 return {...payload,version:1,view,camera:{x:number(rawCamera?.x,0,-1e9,1e9),y:number(rawCamera?.y,0,-1e9,1e9),scale:number(rawCamera?.scale,1,.01,3)},positions,hops:number(payload.hops,2,1,4),limit:number(payload.limit,40,1,300),perNode:number(payload.perNode,16,1,128),direction:['both','incoming','outgoing'].includes(String(payload.direction))?String(payload.direction):'both',relationshipKind:typeof payload.relationshipKind==='string'?payload.relationshipKind:'',visibleQuery:typeof payload.visibleQuery==='string'?payload.visibleQuery:'',roots:Array.isArray(payload.roots)?payload.roots.filter((id):id is string=>typeof id==='string').slice(0,300):(view?.nodes.filter(n=>n.depth===0).map(n=>n.id)??[])};
}
