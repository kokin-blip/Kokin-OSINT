#!/usr/bin/env python3
"""Bundle verbatim production npm notices and the scoped Rustls data notice."""
import json
import shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
lock=json.loads((ROOT/'ui/package-lock.json').read_text())
output=ROOT/'src-tauri/native/frontend-notices';output.mkdir(parents=True,exist_ok=True)
for relative,entry in lock['packages'].items():
    if not relative or entry.get('dev'):continue
    source=ROOT/'ui'/relative
    files=[p for p in source.iterdir() if p.is_file() and p.name.lower().startswith(('license','licence','copying'))]
    if not files:
        files=[p for p in source.glob('README*') if 'Permission is hereby granted' in p.read_text(errors='replace')]
    if not files and relative=='node_modules/pmtiles':files=[ROOT/'resources/notices/pmtiles-LICENSE']
    if not files:raise SystemExit(f'No verbatim notice for runtime dependency: {relative}')
    directory=output/relative.removeprefix('node_modules/').replace('/','__');directory.mkdir(exist_ok=True)
    for path in files:shutil.copyfile(path,directory/path.name)
    (directory/'version.txt').write_text(entry['version']+'\n')
roots=ROOT/'src-tauri/native/trust-roots';roots.mkdir(exist_ok=True)
registry=Path.home()/'.cargo/registry/src'
source=list(registry.glob('*/webpki-roots-1.0.9/LICENSE'))
if len(source)!=1:raise SystemExit('Install the reviewed webpki-roots crate before preparing notices')
shutil.copyfile(source[0],roots/'LICENSE')
