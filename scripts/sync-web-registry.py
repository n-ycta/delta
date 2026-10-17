#!/usr/bin/env python3
"""Publish only a verified stable Delta Web release into the updater catalog."""
import hashlib
import io
import json
import re
import tarfile
import urllib.request
from pathlib import Path

REPO = 'https://github.com/n-ycta/delta-web'
ROOT = Path(__file__).resolve().parents[1]


def version(value):
    if not re.fullmatch(r'\d+\.\d+\.\d+', value):
        raise ValueError('Only stable x.y.z releases are supported')
    return tuple(map(int, value.split('.')))


def update(catalog, release, archive, checksums):
    if release.get('draft') or release.get('prerelease'):
        raise ValueError('Release is not stable')
    tag = release['tag_name']
    if not tag.startswith('v'):
        raise ValueError('Expected a version tag')
    new_version = tag[1:]
    version(new_version)
    item = next(p for p in catalog['plugins'] if p['id'] == 'dev.delta.web')
    if version(new_version) < version(item['version']):
        raise ValueError('Refusing catalog downgrade')
    digest = hashlib.sha256(archive).hexdigest()
    matches = [line.split()[0] for line in checksums.splitlines()
               if len(line.split()) == 2 and line.split()[1].lstrip('*') == 'delta-web.tar.gz']
    if matches != [digest]:
        raise ValueError('Release checksum mismatch or ambiguous checksum')
    with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as tar:
        entries = [m for m in tar.getmembers() if m.name == 'manifest.json']
        if len(entries) != 1 or not entries[0].isfile() or entries[0].size > 2_000_000:
            raise ValueError('Missing, duplicate or oversized manifest')
        manifest = json.load(tar.extractfile(entries[0]))
    ui = manifest['metadata']['ui']
    if (manifest['id'], manifest['version'], ui['id'], ui['version']) != (
            'web', new_version, 'dev.delta.web', new_version):
        raise ValueError('Manifest identity/version mismatch')
    if item['version'] == new_version:
        if item['sha256'] != digest or item['manifest'] != ui:
            raise ValueError('Published release changed without a version bump')
        return False
    item.update(version=new_version, asset=f'{REPO}/releases/download/{tag}/delta-web.tar.gz',
                sha256=digest, manifest=ui)
    return True


def download(url, limit):
    request = urllib.request.Request(url, headers={'User-Agent': 'Delta-registry-sync'})
    with urllib.request.urlopen(request, timeout=180) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError('Download exceeds size limit')
    return data


def main():
    release = json.loads(download('https://api.github.com/repos/n-ycta/delta-web/releases/latest', 2_000_000))
    tag = release['tag_name']
    if not re.fullmatch(r'v\d+\.\d+\.\d+', tag):
        raise ValueError('Invalid release tag')
    base = f'{REPO}/releases/download/{tag}/'
    assets = {a['name']: a['browser_download_url'] for a in release['assets']}
    for name in ('delta-web.tar.gz', 'SHA256SUMS'):
        if assets.get(name) != base + name:
            raise ValueError('Missing or unexpected release asset URL')
    archive = download(assets['delta-web.tar.gz'], 50_000_000)
    checksums = download(assets['SHA256SUMS'], 1_000_000).decode('utf-8')
    path = ROOT / 'registry/plugins.json'
    catalog = json.loads(path.read_text(encoding='utf-8'))
    if update(catalog, release, archive, checksums):
        path.write_text(json.dumps(catalog, ensure_ascii=True, indent=2) + '\n', encoding='utf-8')
        print(f'Updated web catalog to {tag}; SHA-256 and manifest verified')
    else:
        print(f'Web catalog already matches verified {tag}')


if __name__ == '__main__':
    main()
