#!/usr/bin/env python3
"""Package the exact manifest inventory, using shared reads instead of exclusive opens."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import uuid
import zipfile


def build(payload, output):
    payload, output = Path(payload), Path(output)
    if output.exists():
        raise ValueError('output already exists')
    with (payload / 'manifest.json').open('rb') as source:
        manifest_bytes = source.read(1024 * 1024 + 1)
    if len(manifest_bytes) > 1024 * 1024:
        raise ValueError('manifest exceeds size limit')
    manifest = json.loads(manifest_bytes)
    files = manifest['files']
    names = [entry['name'] for entry in files]
    if not names or len(names) > 512 or any(
        not isinstance(name, str) or name in ('', '.', '..', 'manifest.json')
        or '/' in name or '\\' in name or ':' in name for name in names
    ):
        raise ValueError('invalid manifest inventory')
    if len({name.casefold() for name in names}) != len(names):
        raise ValueError('duplicate manifest filename')
    if any(not isinstance(f['bytes'], int) or f['bytes'] < 0 for f in files) or sum(f['bytes'] for f in files) > 512 * 1024 * 1024:
        raise ValueError('payload exceeds size limit')
    expected = set(names) | {'manifest.json'}
    entries = list(payload.iterdir())
    if {p.name for p in entries} != expected or any(p.is_symlink() or not p.is_file() for p in entries):
        raise ValueError('payload differs from manifest inventory')
    temporary = output.with_name(output.name + '.' + uuid.uuid4().hex + '.tmp')
    try:
        with zipfile.ZipFile(temporary, 'w', zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
            sizes = {entry['name']: entry['bytes'] for entry in files}
            sizes['manifest.json'] = len(manifest_bytes)
            for name in sorted(expected):
                path = payload / name
                if path.stat().st_size != sizes[name]:
                    raise ValueError('payload size differs from manifest')
                info = zipfile.ZipInfo.from_file(path, name)
                info.compress_type = zipfile.ZIP_DEFLATED
                with path.open('rb') as source, archive.open(info, 'w') as target:
                    remaining = sizes[name]
                    while remaining:
                        chunk = source.read(min(256 * 1024, remaining))
                        if not chunk:
                            raise ValueError('payload ended before expected size')
                        target.write(chunk)
                        remaining -= len(chunk)
                    if source.read(1):
                        raise ValueError('payload grew during packaging')
        with zipfile.ZipFile(temporary) as archive:
            if archive.testzip() is not None or archive.read('manifest.json') != manifest_bytes:
                raise ValueError('archive integrity check failed')
            for entry in files:
                info = archive.getinfo(entry['name'])
                if info.file_size != entry['bytes']:
                    raise ValueError('archive size differs from manifest')
                digest = hashlib.sha256()
                with archive.open(entry['name']) as source:
                    for chunk in iter(lambda: source.read(256 * 1024), b''):
                        digest.update(chunk)
                if digest.hexdigest() != entry['sha256']:
                    raise ValueError('archive hash differs from manifest')
        temporary.replace(output)
    finally:
        temporary.unlink(missing_ok=True)
    return len(expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--payload', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    try:
        count = build(args.payload, args.output)
    except (OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        print('Portable ZIP failed: ' + str(error), file=sys.stderr)
        return 1
    print(f'Portable ZIP verified: {count} files')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
