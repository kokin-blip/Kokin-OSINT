export interface Camera {x:number;y:number;scale:number;}
export interface Bounds {left:number;top:number;width:number;height:number;}
export const clampScale=(value:number)=>Math.min(3,Math.max(.01,value));
/** Keep the world point beneath the cursor stationary while zooming. */
export function zoomAt(camera:Camera,factor:number,x:number,y:number):Camera {
 const scale=clampScale(camera.scale*factor),ratio=scale/camera.scale;
 return {scale,x:x-(x-camera.x)*ratio,y:y-(y-camera.y)*ratio};
}
export function fitCamera(bounds:Bounds,width:number,height:number):Camera {
 const scale=clampScale(Math.min((width-48)/Math.max(1,bounds.width),(height-48)/Math.max(1,bounds.height),1.5));
 return {scale,x:(width-bounds.width*scale)/2-bounds.left*scale,y:(height-bounds.height*scale)/2-bounds.top*scale};
}
export function centerCamera(camera:Camera,x:number,y:number,width:number,height:number):Camera {
 const scale=Math.max(1,camera.scale);return {scale,x:width/2-x*scale,y:height/2-y*scale};
}
