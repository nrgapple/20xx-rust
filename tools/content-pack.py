#!/usr/bin/env python3
"""Verify and stage derived graphics. Originals stay on the developer machine.

Without local converted files, hydrate the pinned public pack (CI path).
A hash mismatch fails the build rather than silently dropping runtime content.
"""
import argparse
import hashlib
import json
import pathlib
import shutil
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = ROOT / 'research/content-manifest.json'
BASE = 'https://20xx-rust.champtoday.workers.dev/content/'
IDS = ['fox', 'mario', 'marth', 'falcon', 'final-destination']


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--record', action='store_true')
    parser.add_argument('--source', type=pathlib.Path, default=ROOT / 'generated/content-v2')
    parser.add_argument('--dest', type=pathlib.Path, default=ROOT / 'web/dist/content')
    args = parser.parse_args()
    if args.record:
        assets = {}
        for name in IDS:
            metadata = (args.source / f'{name}.json').read_bytes()
            binary = (args.source / f'{name}.bin').read_bytes()
            source = json.loads(metadata)
            mh, bh = digest(metadata), digest(binary)
            assets[name] = {
                'metadata': f'{name}.{mh[:16]}.json', 'metadata_sha256': mh,
                'binary': f'{name}.{bh[:16]}.bin', 'binary_sha256': bh,
                'source': source['source'], 'source_sha256': source['source_sha256'],
            }
        MANIFEST.write_text(json.dumps({'version': 1, 'assets': assets}, indent=2) + '\n')
    manifest = json.loads(MANIFEST.read_text())
    args.dest.mkdir(parents=True, exist_ok=True)
    for asset_id, asset in manifest['assets'].items():
        for kind, extension in [('metadata', 'json'), ('binary', 'bin')]:
            name, expected = asset[kind], asset[f'{kind}_sha256']
            if pathlib.Path(name).name != name:
                raise ValueError('invalid content filename')
            local = args.source / f'{asset_id}.{extension}'
            if local.exists():
                data = local.read_bytes()
            else:
                request = urllib.request.Request(BASE + name, headers={"User-Agent": "20xx-content-pack/1.0"})
                with urllib.request.urlopen(request, timeout=30) as response:
                    data = response.read(25 * 1024 * 1024 + 1)
            if len(data) > 25 * 1024 * 1024 or digest(data) != expected:
                raise ValueError(f'size or checksum mismatch for {name}')
            (args.dest / name).write_bytes(data)
    shutil.copyfile(MANIFEST, args.dest / 'manifest.json')
    print('Verified graphics content copied; no ROM required at runtime.')


if __name__ == '__main__':
    main()
