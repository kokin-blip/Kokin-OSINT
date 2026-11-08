import type {Source,RangeResponse} from 'pmtiles';import * as api from '../../bindings';
/** The PMTiles reader never receives an HTTPSource, filesystem path or remote URL. */
export class LocalArchiveSource implements Source{
 constructor(readonly archive:api.MapArchiveView){}
 getKey(){return `kokin-local://${this.archive.id}`;}
 async getBytes(offset:number,length:number,signal?:AbortSignal):Promise<RangeResponse>{
  if(signal?.aborted)throw new DOMException('Aborted','AbortError');
  if(!Number.isSafeInteger(offset)||!Number.isSafeInteger(length)||offset<0||length<=0||length>4*1024*1024||offset>=this.archive.bytes)throw new Error('Local map range exceeds its limits');
  const raw=await api.mapRange(this.archive.id,offset,Math.min(length,this.archive.bytes-offset));if(signal?.aborted)throw new DOMException('Aborted','AbortError');
  const bytes=Uint8Array.from(atob(raw),c=>c.charCodeAt(0));return{data:bytes.buffer};
 }
}
export async function boundedDecompress(data:ArrayBuffer,compression:number):Promise<ArrayBuffer>{
 if(compression===1)return data;if(compression!==2)throw new Error('Only none/gzip map compression is supported');
 const reader=new Blob([data]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();let total=0;const chunks:Uint8Array[]=[];
 try{for(;;){const{done,value}=await reader.read();if(done)break;total+=value.length;if(total>16*1024*1024)throw new Error('Map decompression budget reached');chunks.push(value);}}finally{await reader.cancel();}
 const bytes=new Uint8Array(total);let offset=0;for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.length;}return bytes.buffer;
}
