#!/usr/bin/env python3
"""Explicit OpenCV reference acceptance, using only the public-domain fixtures.
Requires cv2/numpy in the caller's Python environment; no downloads or installs.
"""
import argparse,json,os,struct,subprocess
from pathlib import Path
import cv2
import numpy as np
ROOT=Path(__file__).resolve().parent.parent
p=argparse.ArgumentParser();p.add_argument('--models-dir',type=Path,required=True);p.add_argument('--worker',type=Path,default=ROOT/'target/debug/kokin-face-worker');p.add_argument('--native',type=Path,default=ROOT/'src-tauri/native');args=p.parse_args()
detector=cv2.FaceDetectorYN.create(str(args.models_dir/'face_detection_yunet_2023mar.onnx'),'',(640,640),0.9,0.3,5000)
recognizer=cv2.FaceRecognizerSF.create(str(args.models_dir/'face_recognition_sface_2021dec.onnx'),'')
for name,media in [('astronaut.png','image/png'),('grace_hopper.jpg','image/jpeg')]:
    fixture=ROOT/'crates/kokin-faces/tests/fixtures'/name
    image=cv2.imread(str(fixture));h,w=image.shape[:2];scale=640/max(w,h)
    resized=cv2.resize(image,(round(w*scale),round(h*scale)));canvas=np.zeros((640,640,3),dtype=np.uint8);canvas[:resized.shape[0],:resized.shape[1]]=resized
    _,faces=detector.detect(canvas);assert faces is not None
    reference=faces[0].copy();reference[:14]/=scale
    vector=recognizer.feature(recognizer.alignCrop(image,reference)).flatten();vector/=np.linalg.norm(vector)
    request=json.dumps(dict(version=1,media_type=media,page=0,start_ms=0,end_ms=None,preview=False)).encode()
    completed=subprocess.run([str(args.worker)],input=struct.pack('<I',len(request))+request+fixture.read_bytes(),capture_output=True,env={'KOKIN_MEDIA_NATIVE':str(args.native.resolve()),'KOKIN_FACE_MODELS':str(args.models_dir.resolve())},check=True)
    detected=json.loads(completed.stdout)['detections'];assert detected
    candidate=detected[0];similarity=float(np.dot(vector,np.array(candidate['vector'])))
    box_error=float(np.max(np.abs(reference[:4]-candidate['bbox'])))
    print(json.dumps(dict(fixture=name,reference_embedding_cosine=similarity,bbox_max_error_pixels=box_error,opencv=cv2.__version__)))
    assert similarity>=0.99, 'SFace preprocessing drift from OpenCV reference'
    assert box_error<=4, 'YuNet postprocessing drift from OpenCV reference'
