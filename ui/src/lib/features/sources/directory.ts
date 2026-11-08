import type {DirectoryTool} from '../../bindings';
export interface ToolBranch {name:string;path:string;children:ToolBranch[];tools:DirectoryTool[];}
/** Preserve the pinned catalog's category ancestry without evaluating its content. */
export function toolTree(tools:DirectoryTool[]):ToolBranch{
 const root:ToolBranch={name:'Tools',path:'',children:[],tools:[]};
 for(const tool of tools){let node=root;for(const name of tool.category.split(' > ').filter(Boolean)){let child=node.children.find(c=>c.name===name);if(!child){child={name,path:node.path?`${node.path} > ${name}`:name,children:[],tools:[]};node.children.push(child);}node=child;}node.tools.push(tool);}
 return root;
}
