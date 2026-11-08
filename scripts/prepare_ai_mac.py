#!/usr/bin/env python3
"""Explicit developer preparation of the pinned native ONNX engine, never models.

Run --offline after preparation. No startup, UI, or default test invokes this.
"""
import argparse
import hashlib
import json
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--offline', action='store_true')
args = parser.parse_args()
registry = json.loads((ROOT / 'resources/native-onnx-mac-arm64.json').read_text())
destination = ROOT / 'src-tauri/native/onnx'
library = destination / 'libonnxruntime.1.22.0.dylib'
if args.offline:
    for name in ['libonnxruntime.1.22.0.dylib', 'LICENSE', 'ThirdPartyNotices.txt', 'VERSION_NUMBER']:
        if not (destination / name).is_file():
            raise SystemExit(f'Missing bundled engine file: {name}')
    subprocess.run(['codesign', '--verify', '--strict', str(library)], check=True)
    raise SystemExit(0)

with tempfile.TemporaryDirectory(prefix='kokin-onnx-engine-') as temporary:
    archive = Path(temporary) / 'runtime.tgz'
    with urllib.request.urlopen(registry['url'], timeout=60) as response:
        data = response.read(registry['bytes'] + 1)
    if len(data) != registry['bytes'] or hashlib.sha256(data).hexdigest() != registry['sha256']:
        raise SystemExit('Native engine artifact identity mismatch')
    archive.write_bytes(data)
    prefix = 'onnxruntime-osx-arm64-1.22.0/'
    members = {prefix + 'lib/libonnxruntime.1.22.0.dylib': library.name,
               **{prefix + name: name for name in ['LICENSE', 'ThirdPartyNotices.txt', 'VERSION_NUMBER']}}
    destination.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as source:
        for member, name in members.items():
            item = source.getmember(member)
            if not item.isfile() or item.size > 40 * 1024 * 1024:
                raise SystemExit('Unexpected native engine archive member')
            contents = source.extractfile(item).read()
            if name == library.name and hashlib.sha256(contents).hexdigest() != '2b885992d3d6fa4130d39ec84a80d7504ff52750027c547bb22c86165f19406a':
                raise SystemExit('Native library identity mismatch')
            target = destination / name
            if target.exists():
                target.chmod(0o644)
            target.write_bytes(contents)
    subprocess.run(['codesign', '--force', '--sign', '-', str(library)], check=True)
    subprocess.run(['codesign', '--verify', '--strict', str(library)], check=True)
