import {expect,it} from 'vitest';
import {zoomAt,fitCamera,centerCamera} from './viewport';
it('keeps the cursor world coordinate stationary and clamps zoom',()=>{
 const camera={x:20,y:-30,scale:1.2},point={x:200,y:140};const next=zoomAt(camera,1.25,point.x,point.y);
 expect((point.x-next.x)/next.scale).toBeCloseTo((point.x-camera.x)/camera.scale);
 expect((point.y-next.y)/next.scale).toBeCloseTo((point.y-camera.y)/camera.scale);
 expect(zoomAt(camera,100,0,0).scale).toBe(3);expect(zoomAt(camera,.001,0,0).scale).toBe(.01);
});
it('fits ordinary bounds with padding and centers a record without losing zoom',()=>{
 const fit=fitCamera({left:10,top:-20,width:700,height:300},900,420);
 expect(fit.x+10*fit.scale).toBeGreaterThanOrEqual(24);
 expect(fit.y-20*fit.scale).toBeGreaterThanOrEqual(24);
 const focused=centerCamera({x:0,y:0,scale:2},120,75,900,420);
 expect(focused.x+120*focused.scale).toBe(450);expect(focused.y+75*focused.scale).toBe(210);expect(focused.scale).toBe(2);
});

it('fits the maximum permitted 300-record column inside the viewport',()=>{
 const bounds={left:0,top:-20,width:300,height:21060};const camera=fitCamera(bounds,900,420);
 expect(camera.y+bounds.top*camera.scale).toBeGreaterThanOrEqual(24);
 expect(camera.y+(bounds.top+bounds.height)*camera.scale).toBeLessThanOrEqual(396);
});
