#!/usr/bin/env python3
"""Explicit developer bootstrap for the pinned Mac media bundle.

Not called on app startup or by the deterministic suite. Uses installed reviewed
OCR engines, verifies pinned artifacts/notices, and prepares the first-party worker.
"""
import argparse,hashlib,json,os,subprocess,tarfile,tempfile,urllib.request
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
parser=argparse.ArgumentParser();parser.add_argument('--release',action='store_true');parser.add_argument('--offline',action='store_true');args=parser.parse_args()
registry=json.loads((ROOT/'resources/native-media-mac-arm64.json').read_text())
pdf=ROOT/'src-tauri/native/pdfium'
if not (pdf/'libpdfium.dylib').is_file():
 if args.offline:raise SystemExit('Pinned PDFium engine is absent; explicit bootstrap download required.')
 with tempfile.TemporaryDirectory(prefix='kokin-native-engine-') as temporary:
  archive=Path(temporary)/'pdfium.tgz'
  with urllib.request.urlopen(registry['pdfium']['url'],timeout=60) as response:
   data=response.read(4*1024*1024)
  if hashlib.sha256(data).hexdigest()!=registry['pdfium']['archive_sha256']:raise SystemExit('PDFium artifact identity mismatch')
  archive.write_bytes(data)
  with tarfile.open(archive) as source:
   for member in source.getmembers():
    if member.name not in ('lib/libpdfium.dylib','LICENSE','VERSION','args.gn') and not member.name.startswith('licenses/'):continue
    if not member.isfile() or member.size>20*1024*1024 or '..' in Path(member.name).parts:raise SystemExit('Unexpected PDFium archive member')
    destination=pdf/(Path(member.name).name if member.name.startswith('lib/') else member.name)
    destination.parent.mkdir(parents=True,exist_ok=True);destination.write_bytes(source.extractfile(member).read())
 for line in ['pdf_enable_v8 = false','pdf_enable_xfa = false']:
  if line not in (pdf/'args.gn').read_text():raise SystemExit('PDF JavaScript/XFA configuration is not approved')
for name,entry in registry['ocr_libraries'].items():
 source=Path('/opt/homebrew/Cellar')/entry['version_path']
 if not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest()!=entry['source_sha256']:raise SystemExit(f'Install the reviewed native engine build first: {name}')
for relative,digest in registry['notice_sha256'].items():
 notice=ROOT/'src-tauri/native'/relative
 if not notice.is_file():raise SystemExit(f'Reviewed notice must be supplied before packaging: {relative}')
 if hashlib.sha256(notice.read_bytes()).hexdigest()!=digest:raise SystemExit(f'Notice identity mismatch: {relative}')
command=['cargo','build','-p','kokin-media','--features','engine']
if args.release:command.append('--release')
env=dict(os.environ);env['PKG_CONFIG_PATH']='/opt/homebrew/lib/pkgconfig'
subprocess.run(command,cwd=ROOT,env=env,check=True)
subprocess.run(['python3',str(ROOT/'scripts/bundle_media_mac.py'),*(['--release'] if args.release else [])],cwd=ROOT,check=True)
