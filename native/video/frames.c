/* SPDX-License-Identifier: MIT
 * Copyright (c) 2026 Kokin-OSINT contributors.
 */
/* LGPL FFmpeg decoder helper. Source bytes stay in bounded, seekable memory.
 * stdout: repeated little-endian [timestamp:u64,w:u32,h:u32,rotation:i32,RGB24].
 * Trailer: w=h=0; timestamp is number of eligible frames sampled (1801 => cap).
 */
#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/display.h>
#include <libswscale/swscale.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
typedef struct { uint8_t *bytes; int64_t size, position; } Memory;
static int read_memory(void *opaque,uint8_t *out,int size){Memory*m=opaque;int64_t left=m->size-m->position;if(left<=0)return AVERROR_EOF;if(size>left)size=(int)left;memcpy(out,m->bytes+m->position,size);m->position+=size;return size;}
static int64_t seek_memory(void*opaque,int64_t off,int whence){Memory*m=opaque;if(whence==AVSEEK_SIZE)return m->size;int base=whence&~AVSEEK_FORCE;int64_t p=base==SEEK_SET?off:base==SEEK_CUR?m->position+off:base==SEEK_END?m->size+off:-1;if(p<0||p>m->size)return AVERROR(EINVAL);m->position=p;return p;}
static void put32(uint32_t x){for(int i=0;i<4;i++)putchar((x>>(8*i))&255);}
static void header(uint64_t t,uint32_t w,uint32_t h,int rotation){put32((uint32_t)t);put32((uint32_t)(t>>32));put32(w);put32(h);put32((uint32_t)rotation);}
static int frames(AVCodecContext*c,AVFrame*f,AVStream*s,int rotation,int64_t start,int64_t end,int64_t*next,int*count){
 int status;
 while((status=avcodec_receive_frame(c,f))>=0){
   if(f->width<=0||f->height<=0||(int64_t)f->width*f->height>16000000)return -1;
   int64_t pts=f->best_effort_timestamp;if(pts==AV_NOPTS_VALUE)continue;
   int64_t origin=s->start_time==AV_NOPTS_VALUE?0:s->start_time;
   int64_t ms=av_rescale_q(pts-origin,s->time_base,(AVRational){1,1000});
   if(ms<start||ms<*next)continue;if(ms>=end)return 1;
   if(++*count>1800)return 1;*next=ms+2000;
   struct SwsContext*sw=sws_getContext(f->width,f->height,(enum AVPixelFormat)f->format,f->width,f->height,AV_PIX_FMT_RGB24,SWS_BILINEAR,NULL,NULL,NULL);if(!sw)return -1;
   size_t len=(size_t)f->width*f->height*3;uint8_t*rgb=av_malloc(len);if(!rgb){sws_freeContext(sw);return -1;}
   uint8_t*dest[4]={rgb,0,0,0};int lines[4]={f->width*3,0,0,0};
   int rows=sws_scale(sw,(const uint8_t*const*)f->data,f->linesize,0,f->height,dest,lines);
   if(rows!=f->height){av_free(rgb);sws_freeContext(sw);return -1;}
   header((uint64_t)ms,(uint32_t)f->width,(uint32_t)f->height,rotation);
   if(fwrite(rgb,1,len,stdout)!=len){av_free(rgb);sws_freeContext(sw);return -1;}
   av_free(rgb);sws_freeContext(sw);av_frame_unref(f);
 }
 return status==AVERROR(EAGAIN)||status==AVERROR_EOF?0:-1;
}
int main(int argc,char**argv){
 if(argc!=3)return 2;av_log_set_level(AV_LOG_QUIET);
 int64_t start=strtoll(argv[1],NULL,10),end=strtoll(argv[2],NULL,10),next=start;
 if(start<0||end<=start||end>86400000)return 2;
 Memory m={0};size_t cap=1024*1024;m.bytes=av_malloc(cap);if(!m.bytes)return 2;
 for(;;){if((size_t)m.size==cap){if(cap>=512u*1024*1024){int extra=getchar();if(extra!=EOF)return 2;break;}cap*=2;uint8_t*b=av_realloc(m.bytes,cap);if(!b)return 2;m.bytes=b;}
 size_t n=fread(m.bytes+m.size,1,cap-m.size,stdin);m.size+=(int64_t)n;if(!n){if(ferror(stdin))return 2;break;}}
 if(!m.size)return 2;
 uint8_t*buffer=av_malloc(65536);if(!buffer)return 2;
 AVIOContext*io=avio_alloc_context(buffer,65536,0,&m,read_memory,NULL,seek_memory);if(!io)return 2;
 AVFormatContext*format=avformat_alloc_context();if(!format)return 2;format->pb=io;format->flags|=AVFMT_FLAG_CUSTOM_IO;format->max_streams=16;format->probesize=8*1024*1024;format->max_analyze_duration=10*AV_TIME_BASE;
 if(avformat_open_input(&format,NULL,NULL,NULL)<0||avformat_find_stream_info(format,NULL)<0)return 2;
 const AVCodec*codec=NULL;int index=av_find_best_stream(format,AVMEDIA_TYPE_VIDEO,-1,-1,&codec,0);if(index<0||!codec)return 2;
 AVStream*stream=format->streams[index];AVCodecContext*c=avcodec_alloc_context3(codec);if(!c)return 2;
 if(avcodec_parameters_to_context(c,stream->codecpar)<0)return 2;c->thread_count=2;c->max_pixels=16000000;
 if(avcodec_open2(c,codec,NULL)<0)return 2;
 int rotation=0;const AVPacketSideData*matrix=av_packet_side_data_get(stream->codecpar->coded_side_data,stream->codecpar->nb_coded_side_data,AV_PKT_DATA_DISPLAYMATRIX);
 if(matrix){if(matrix->size<9*sizeof(int32_t))return 2;double angle=-av_display_rotation_get((const int32_t*)matrix->data);if(!isfinite(angle))return 2;rotation=((int)lround(angle)%360+360)%360;if(rotation%90)return 2;}

 AVPacket*p=av_packet_alloc();AVFrame*f=av_frame_alloc();if(!p||!f)return 2;int count=0,done=0,read_status=0;
 while(!done&&(read_status=av_read_frame(format,p))>=0){if(p->stream_index==index){if(avcodec_send_packet(c,p)<0)return 2;int status=frames(c,f,stream,rotation,start,end,&next,&count);if(status<0)return 2;done=status;}av_packet_unref(p);}
 if(!done){if(read_status!=AVERROR_EOF)return 2;if(avcodec_send_packet(c,NULL)<0)return 2;if(frames(c,f,stream,rotation,start,end,&next,&count)<0)return 2;}
 header((uint64_t)count,0,0,0);int failed=fflush(stdout)||ferror(stdout);
 av_frame_free(&f);av_packet_free(&p);avcodec_free_context(&c);avformat_close_input(&format);av_freep(&io->buffer);avio_context_free(&io);av_free(m.bytes);return failed?2:0;
}
