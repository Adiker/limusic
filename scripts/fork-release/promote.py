#!/usr/bin/env python3
"""Verify all package assets before publishing or changing update channels."""
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path


def run(*args, check=True):
    return subprocess.run(args, check=check, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def semver(value):
    m = re.fullmatch(r'(\d+)\.(\d+)\.(\d+)(?:-rc\.(\d+))?', value)
    if not m:
        raise ValueError(f'Unsupported version {value!r}')
    a, b, c, rc = m.groups()
    return int(a), int(b), int(c), int(rc is None), int(rc or 0)


def validate_assets(tag, assets, manifest):
    version = tag[1:]
    semver(version)
    names = {a['name'] for a in assets if a.get('state') == 'uploaded' and a.get('size', 0) > 0}
    required = {f'limusic_{version}_amd64.AppImage', f'limusic_{version}_amd64.AppImage.sig',
                f'limusic_{version}_x64-setup.exe', f'limusic_{version}_x64-setup.exe.sig',
                f'limusic_{version}_aarch64.dmg', f'limusic_{version}_aarch64.app.tar.gz',
                f'limusic_{version}_aarch64.app.tar.gz.sig', 'latest.json'}
    if '-rc.' not in version:
        required |= {f'limusic_{version}_amd64.deb', f'limusic_{version}_x64_en-US.msi'}
    missing = required - names
    if missing:
        raise ValueError(f'Missing assets: {sorted(missing)}')
    arch = version.replace('-', '')
    if not any(re.fullmatch(rf'limusic-{re.escape(arch)}-\d+-x86_64\.pkg\.tar\.zst', n) for n in names):
        raise ValueError('Missing Arch package')
    if not any(n.startswith('limusic-' + version.split('-')[0]) and n.endswith('.x86_64.rpm') for n in names):
        raise ValueError('Missing RPM package')
    if manifest.get('version') != version:
        raise ValueError('Manifest version does not match tag')
    platforms = manifest.get('platforms', {})
    paths = {'linux-x86_64': f'limusic_{version}_amd64.AppImage',
             'windows-x86_64': f'limusic_{version}_x64-setup.exe',
             'darwin-aarch64': f'limusic_{version}_aarch64.app.tar.gz'}
    if set(platforms) != set(paths):
        raise ValueError('Updater manifest must include all three platforms')
    repo = os.environ['GITHUB_REPOSITORY']
    for platform, asset in paths.items():
        value = platforms[platform]
        if value.get('url') != f'https://github.com/{repo}/releases/download/{tag}/{asset}' or not value.get('signature'):
            raise ValueError(f'Invalid updater entry for {platform}')


def main():
    repo, tag, sha = (os.environ[k] for k in ('GITHUB_REPOSITORY', 'RELEASE_TAG', 'RELEASE_SHA'))
    data = json.loads(run('gh', 'release', 'view', tag, '--repo', repo, '--json', 'isDraft,isPrerelease,assets').stdout)
    if data['isPrerelease'] != ('-rc.' in tag):
        raise RuntimeError('Release has the wrong channel')
    with tempfile.TemporaryDirectory() as d:
        run('gh', 'release', 'download', tag, '--repo', repo, '--pattern', 'latest.json', '--dir', d)
        manifest = json.loads(Path(d, 'latest.json').read_text())
        validate_assets(tag, data['assets'], manifest)
        run('gh', 'release', 'download', tag, '--repo', repo, '--pattern', '*.sig', '--dir', d)
        for entry in manifest['platforms'].values():
            signature = Path(d, entry['url'].rsplit('/', 1)[-1] + '.sig').read_text().strip()
            if signature != entry['signature'].strip():
                raise RuntimeError('Updater manifest signature does not match its asset')
        beta = run('gh', 'release', 'view', 'beta', '--repo', repo, '--json', 'isDraft,isPrerelease,assets', check=False)
        if beta.returncode:
            if 'release not found' not in beta.stderr.lower():
                raise RuntimeError(f'Cannot inspect beta pointer: {beta.stderr}')
            run('gh', 'release', 'create', 'beta', '--repo', repo, '--target', sha,
                '--prerelease', '--latest=false', '--title', 'Beta update channel',
                '--notes', 'Updater manifest for the latest complete fork release.')
            beta_info = {'assets': [], 'isDraft': False, 'isPrerelease': True}
        else:
            beta_info = json.loads(beta.stdout)
        if beta_info['isDraft'] or not beta_info['isPrerelease']:
            raise RuntimeError('Beta pointer is not a published prerelease')
        beta_version = None
        if any(a['name'] == 'latest.json' for a in beta_info['assets']):
            Path(d, 'beta').mkdir()
            run('gh', 'release', 'download', 'beta', '--repo', repo, '--pattern', 'latest.json', '--dir', str(Path(d, 'beta')))
            beta_version = json.loads(Path(d, 'beta', 'latest.json').read_text())['version']
        # Publishing happens only after every build and manifest check succeeded. Do not move the
        # beta pointer backwards on a retry of an older release.
        if data['isDraft']:
            run('gh', 'release', 'edit', tag, '--repo', repo, '--draft=false', '--latest=false')
        if '-rc.' not in tag:
            latest = run('gh', 'release', 'view', '--repo', repo, '--json', 'tagName', check=False)
            if latest.returncode and 'release not found' not in latest.stderr.lower():
                raise RuntimeError(f'Cannot inspect latest stable release: {latest.stderr}')
            latest_tag = json.loads(latest.stdout)['tagName'] if latest.returncode == 0 else None
            if not latest_tag or semver(tag[1:]) >= semver(latest_tag[1:]):
                run('gh', 'release', 'edit', tag, '--repo', repo, '--latest')
        if not beta_version or semver(tag[1:]) >= semver(beta_version):
            run('gh', 'release', 'upload', 'beta', str(Path(d, 'latest.json')), '--repo', repo, '--clobber')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, ValueError, KeyError, subprocess.CalledProcessError) as e:
        print(f'::error::{e}', file=sys.stderr)
        if isinstance(e, subprocess.CalledProcessError):
            print(e.stderr, file=sys.stderr)
        sys.exit(1)
