#!/usr/bin/env python3
"""Validate provenance and prepare a draft for one merged upstream release."""
import json
import os
import re
import subprocess
import sys

TAG = re.compile(r'^v\d+\.\d+\.\d+(?:-rc\.\d+)?$')
SHA = re.compile(r'^[0-9a-f]{40}$')


def run(*args, check=True):
    return subprocess.run(args, check=check, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def main():
    repo = os.environ['GITHUB_REPOSITORY']
    branch = os.environ['DEFAULT_BRANCH']
    sha = os.environ.get('EVENT_SHA') or os.environ.get('INPUT_SHA', '')
    if not SHA.fullmatch(sha):
        raise RuntimeError('Expected a full merge commit SHA')
    run('git', 'fetch', 'origin', branch)
    run('git', 'merge-base', '--is-ancestor', sha, 'FETCH_HEAD')
    source_parent = run('git', 'rev-parse', f'{sha}^2').stdout.strip()
    pulls = json.loads(run('gh', 'api', f'repos/{repo}/commits/{sha}/pulls').stdout)
    matching = [p for p in pulls if p.get('merged_at') and p['base']['ref'] == branch
                and p.get('merge_commit_sha') == sha and
                (p['head']['ref'].startswith('codex/sync-upstream-') or
                 p['head']['ref'] == 'automation/upstream-release')]
    if len(matching) != 1:
        raise RuntimeError('Commit must be the merge commit of a merged sync PR')
    config = json.loads(run('git', 'show', f'{sha}:src-tauri/tauri.conf.json').stdout)
    tag = 'v' + config['version']
    if not TAG.fullmatch(tag) or (os.environ.get('INPUT_TAG') and os.environ['INPUT_TAG'] != tag):
        raise RuntimeError('Tag and source version do not match')
    run('git', 'fetch', '--no-tags', 'https://github.com/SimoHypers/limusic.git', f'refs/tags/{tag}')
    upstream = run('git', 'rev-parse', 'FETCH_HEAD^{commit}').stdout.strip()
    run('git', 'merge-base', '--is-ancestor', upstream, source_parent)
    release = json.loads(run('gh', 'api', f'repos/SimoHypers/limusic/releases/tags/{tag}').stdout)
    if release.get('draft') or bool(release.get('prerelease')) != ('-rc.' in tag):
        raise RuntimeError('Upstream release is unpublished or has unexpected prerelease state')
    found = run('git', 'ls-remote', '--tags', 'origin', f'refs/tags/{tag}^{{}}', f'refs/tags/{tag}').stdout.splitlines()
    refs = {line.split('\t')[1]: line.split('\t')[0] for line in found}
    existing = refs.get(f'refs/tags/{tag}^{{}}') or refs.get(f'refs/tags/{tag}')
    if existing and existing != sha:
        raise RuntimeError(f'Refusing to replace existing tag {tag} ({existing})')
    if not existing:
        run('git', 'push', 'origin', f'{sha}:refs/tags/{tag}')
    old = run('gh', 'release', 'view', tag, '--repo', repo, '--json', 'isDraft', check=False)
    published = old.returncode == 0 and not json.loads(old.stdout)['isDraft']
    if old.returncode == 0:
        pass
    elif 'release not found' in old.stderr.lower():
        opts = ['--prerelease'] if '-rc.' in tag else []
        run('gh', 'release', 'create', tag, '--repo', repo, '--draft', '--verify-tag',
            '--title', f'Limusic {tag}', '--notes', f'Fork build of upstream {tag} from merge commit {sha}.', *opts)
    else:
        raise RuntimeError(f'Cannot inspect release: {old.stderr}')
    with open(os.environ['GITHUB_OUTPUT'], 'a') as out:
        print(f'tag={tag}\nsha={sha}\nbuild={str(not published).lower()}', file=out)
    # Platform workflows merge their signed entries into this starter manifest.
    assets = json.loads(run('gh', 'release', 'view', tag, '--repo', repo, '--json', 'assets').stdout)['assets']
    if not any(a['name'] == 'latest.json' for a in assets):
        if published:
            raise RuntimeError('Published release has no manifest; refusing to modify it')
        import datetime
        manifest = {'version': tag[1:], 'notes': f'Fork build of upstream {tag}',
                    'pub_date': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'platforms': {}}
        with open('latest.json', 'w') as f:
            json.dump(manifest, f)
        run('gh', 'release', 'upload', tag, 'latest.json', '--repo', repo)


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, ValueError, KeyError) as e:
        print(f'::error::{e}', file=sys.stderr)
        if isinstance(e, subprocess.CalledProcessError):
            print(e.stderr, file=sys.stderr)
        sys.exit(1)
