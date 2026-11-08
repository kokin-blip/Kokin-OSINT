#!/usr/bin/env python3
"""Explicit Apple Silicon face/video native preparation. Never runs at app startup.
Models remain opt-in app resources; --models-dir stages them only for development.
"""
import argparse,hashlib,json,os,platform,shutil,subprocess,tarfile,urllib.request
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
p=argparse.ArgumentParser();p.add_argument('--source-archive',type=Path);p.add_argument('--offline',action='store_true');p.add_argument('--models-dir',type=Path);args=p.parse_args()
if platform.system()!='Darwin' or platform.machine()!='arm64':raise SystemExit('Native face/video packaging is currently verified only on Apple Silicon macOS')
registry=json.loads((ROOT/'resources/native-video-mac-arm64.json').read_text())
build=ROOT/'target/face-native';native=ROOT/'src-tauri/native/video';native.mkdir(parents=True,exist_ok=True)
helper=native/'kokin-video-frames'
if args.offline:
    if not helper.is_file():raise SystemExit('Video decoder is not prepared')
    subprocess.run(['codesign','--verify','--strict',str(helper)],check=True)
    bill=json.loads((native/'build.json').read_text())
    if hashlib.sha256(helper.read_bytes()).hexdigest()!=bill['helper_sha256']:raise SystemExit('Video helper identity mismatch')
else:
    build.mkdir(parents=True,exist_ok=True)
    archive=args.source_archive or build/f"ffmpeg-{registry['version']}.tar.xz"
    if not archive.exists():
        with urllib.request.urlopen(registry['url'],timeout=120) as r:data=r.read(registry['bytes']+1)
        archive.write_bytes(data)
    data=archive.read_bytes()
    if len(data)!=registry['bytes'] or hashlib.sha256(data).hexdigest()!=registry['sha256']:raise SystemExit('FFmpeg source identity mismatch')
    source=build/f"ffmpeg-{registry['version']}"
    if not source.exists():
        with tarfile.open(archive) as t:t.extractall(build,filter='data')
    prefix=build/'install'
    flags=['--disable-all','--disable-autodetect','--disable-network','--disable-programs','--disable-doc','--disable-debug','--disable-shared','--enable-static','--enable-pic','--enable-avcodec','--enable-avformat','--enable-avutil','--enable-swscale','--enable-demuxer=mov,matroska,avi','--enable-decoder=h264,hevc,mpeg4,mjpeg,vp8,vp9','--enable-parser=h264,hevc,mpeg4video,vp8,vp9',f'--prefix={prefix}']
    with (build/'build.log').open('w') as log:
        subprocess.run([str(source/'configure'),*flags],cwd=source,stdout=log,stderr=subprocess.STDOUT,check=True)
        subprocess.run(['make','-j',str(min(8,os.cpu_count() or 2))],cwd=source,stdout=log,stderr=subprocess.STDOUT,check=True)
        subprocess.run(['make','install'],cwd=source,stdout=log,stderr=subprocess.STDOUT,check=True)
    config=(source/'config.h').read_text()
    for name in ['CONFIG_GPL','CONFIG_NONFREE','CONFIG_NETWORK']:
        if f'#define {name} 0' not in config:raise SystemExit(f'Unexpected {name} build configuration')
    subprocess.run(['clang','-O2',str(ROOT/'native/video/frames.c'),f'-I{prefix}/include',f'-L{prefix}/lib','-lavformat','-lavcodec','-lswscale','-lavutil','-lm','-lpthread','-liconv','-o',str(helper)],check=True)
    for name in ['COPYING.LGPLv2.1','LICENSE.md']:shutil.copy2(source/name,native/name)
    shutil.copy2(archive,native/'ffmpeg-source.tar.xz')
    shutil.copy2(ROOT/'native/video/frames.c',native/'frames.c')
    shutil.copy2(ROOT/'native/video/LICENSE',native/'KOKIN-HELPER-LICENSE')
    shutil.copy2(ROOT/'native/video/BUILD.md',native/'BUILD.md')
    shutil.copy2(Path(__file__),native/'prepare_faces_mac.py')
    subprocess.run(['codesign','--force','--sign','-',str(helper)],check=True)
    subprocess.run(['codesign','--verify','--strict',str(helper)],check=True)
    (native/'build.json').write_text(json.dumps(dict(source=registry,configure=flags,helper_sha256=hashlib.sha256(helper.read_bytes()).hexdigest(),helper_source_sha256=hashlib.sha256((ROOT/'native/video/frames.c').read_bytes()).hexdigest()),indent=2)+'\n')
# Face workers need a runtime with the upstream macOS logger teardown fix.
# Keep the older text-search runtime unchanged; these are separate processes.
face_runtime=json.loads((ROOT/'resources/native-face-onnx-mac-arm64.json').read_text())
rt_dir=ROOT/'src-tauri/native/face-onnx';rt_dir.mkdir(parents=True,exist_ok=True)
rt_lib=rt_dir/'libonnxruntime.1.31.0.dylib'
if args.offline:
    subprocess.run(['codesign','--verify','--strict',str(rt_lib)],check=True)
    if hashlib.sha256(rt_lib.read_bytes()).hexdigest()!=json.loads((rt_dir/'installed.json').read_text())['sha256']:raise SystemExit('Face runtime identity mismatch')
else:
    rt_archive=build/'onnxruntime-osx-arm64-1.31.0.tgz'
    if not rt_archive.exists():
        with urllib.request.urlopen(face_runtime['url'],timeout=120) as response:contents=response.read(face_runtime['bytes']+1)
        rt_archive.write_bytes(contents)
    contents=rt_archive.read_bytes()
    if len(contents)!=face_runtime['bytes'] or hashlib.sha256(contents).hexdigest()!=face_runtime['sha256']:raise SystemExit('Face runtime archive mismatch')
    with tarfile.open(rt_archive) as archive:
        members={n.lstrip('./'):n for n in archive.getnames()}
        for name,digest in face_runtime['files'].items():
            member='onnxruntime-osx-arm64-1.31.0/'+('lib/' if name.endswith('.dylib') else '')+name
            contents=archive.extractfile(members[member]).read()
            if hashlib.sha256(contents).hexdigest()!=digest:raise SystemExit('Face runtime file mismatch')
            dest=rt_dir/name
            if dest.exists():dest.chmod(0o644)
            dest.write_bytes(contents)
    subprocess.run(['codesign','--force','--sign','-',str(rt_lib)],check=True)
    (rt_dir/'installed.json').write_text(json.dumps(dict(sha256=hashlib.sha256(rt_lib.read_bytes()).hexdigest(),source=face_runtime['sha256']))+'\n')
if args.models_dir:
    if args.models_dir.resolve().is_relative_to((ROOT/'src-tauri/native').resolve()):raise SystemExit('Keep test models outside the native installer bundle')
    args.models_dir.mkdir(parents=True,exist_ok=True)
    face=json.loads((ROOT/'resources/face-registry.json').read_text())
    for r in face['files']:
        dest=args.models_dir/r['file']
        if not dest.exists():
            if args.offline:raise SystemExit('Test face model missing')
            with urllib.request.urlopen(r['url'],timeout=120) as response:contents=response.read(r['bytes']+1)
            if len(contents)!=r['bytes'] or hashlib.sha256(contents).hexdigest()!=r['sha256']:raise SystemExit('Face model identity mismatch')
            dest.write_bytes(contents)
        if dest.stat().st_size!=r['bytes'] or hashlib.sha256(dest.read_bytes()).hexdigest()!=r['sha256']:raise SystemExit('Test face model identity mismatch')
print('Native video helper verified. Build kokin-face-worker with the media engine feature; models remain optional resources.')
