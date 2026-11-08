#!/usr/bin/env python3
"""Relocate installed native engines into an isolated build directory.

Developer packaging action only, never run on application startup. No downloads.
The native dependency bill and license review must pass before release publication.
"""
import hashlib,json,shutil,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
NATIVE=ROOT/'src-tauri/native'
OCR=NATIVE/'ocr';OCR.mkdir(parents=True,exist_ok=True)
registry=json.loads((ROOT/'resources/native-media-mac-arm64.json').read_text())['ocr_libraries']
BIN=ROOT/'src-tauri/binaries';BIN.mkdir(parents=True,exist_ok=True)
worker=BIN/'kokin-media-worker-aarch64-apple-darwin'
profile='release' if '--release' in sys.argv else 'debug'
shutil.copy2(ROOT/f'target/{profile}/kokin-media-worker',worker)
face_worker=BIN/'kokin-face-worker-aarch64-apple-darwin'
shutil.copy2(ROOT/f'target/{profile}/kokin-face-worker',face_worker)
queue=[worker,face_worker];copied={}
def dependencies(path):
 return [line.strip().split(' (')[0] for line in subprocess.check_output(['otool','-L',str(path)],text=True).splitlines()[1:]]
while queue:
 path=queue.pop()
 for dependency in dependencies(path):
  if dependency.startswith('@rpath/'):
   source=(Path('/opt/homebrew/lib')/Path(dependency).name).resolve()
  elif dependency.startswith('/opt/homebrew/'):
   source=Path(dependency).resolve()
  else:continue
  if not str(source).startswith('/opt/homebrew/'):raise SystemExit('Unexpected dependency source')
  name=Path(dependency).name;dest=OCR/name
  if name not in copied:
   if not source.is_file():raise SystemExit(f'Missing native dependency {source}')
   entry=registry.get(name)
   if not entry or hashlib.sha256(source.read_bytes()).hexdigest()!=entry['source_sha256']:
    raise SystemExit(f'Unreviewed native engine identity: {name}')
   if dest.exists():dest.chmod(0o755)
   shutil.copy2(source,dest);dest.chmod(0o755);copied[name]={'source':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest()};queue.append(dest)
  replacement=f'@executable_path/../Resources/native/ocr/{name}' if path in [worker,face_worker] else f'@loader_path/{name}'
  subprocess.run(['install_name_tool','-change',dependency,replacement,str(path)],check=True)
 if path not in [worker,face_worker]:subprocess.run(['install_name_tool','-id',f'@loader_path/{path.name}',str(path)],check=True)
for path in [*OCR.glob('*.dylib'),NATIVE/'pdfium/libpdfium.dylib',worker,face_worker]:
 path.chmod(0o755)
 subprocess.run(['codesign','--force','--sign','-',str(path)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
NATIVE.joinpath('installed-engine-bill.json').write_text(json.dumps(copied,indent=2)+'\n')
print('Pinned media engines relocated; release still requires all subsystem acceptance checks.')

# Stage the same relocated engines for contained developer acceptance.
probe=ROOT/'target/media-probe'
(probe/'MacOS').mkdir(parents=True,exist_ok=True)
shutil.copy2(worker,probe/'MacOS/kokin-media-worker')
shutil.copy2(face_worker,probe/'MacOS/kokin-face-worker')
def stage_copy(source,destination):
 destination=Path(destination)
 if destination.is_symlink():raise SystemExit(f'Refusing staged symlink: {destination}')
 if destination.exists():destination.chmod(0o755)
 return shutil.copy2(source,destination)
shutil.copytree(NATIVE,probe/'Resources/native',dirs_exist_ok=True,copy_function=stage_copy)
