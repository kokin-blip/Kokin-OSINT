#!/usr/bin/env python3
"""Fail if a reviewed dependency backport is bypassed or silently modified."""
import hashlib,json,tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
manifest=tomllib.loads((ROOT/'Cargo.toml').read_text())
lock=tomllib.loads((ROOT/'Cargo.lock').read_text())
for row in json.loads((ROOT/'resources/dependency-patches.json').read_text())['patches']:
    name=row['crate'];expected=row['path']
    assert manifest['patch']['crates-io'][name]['path']==expected, f'{name}: required source patch removed'
    versions=[p for p in lock['package'] if p['name']==name]
    assert len(versions)==1 and versions[0]['version']==row['version'] and 'source' not in versions[0], f'{name}: an unreviewed registry/version entered the lock'
    source=ROOT/expected/row['guarded_file']
    assert hashlib.sha256(source.read_bytes()).hexdigest()==row['patched_sha256'],f'{name}: reviewed patch changed; re-review and test before updating its guard'
print('OK: exact source and lock guards for time/plist/tokenizers compatibility patches')
