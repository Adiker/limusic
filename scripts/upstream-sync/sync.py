#!/usr/bin/env python3
"""Prepare (never merge) one upstream-release pull request for the fork."""
import json
import os
import re
import subprocess
import sys

UPSTREAM = 'SimoHypers/limusic'
BRANCH = 'automation/upstream-release'
MARKER = '<!-- upstream-sync '
TAG = re.compile(r'^v(\d+)\.(\d+)\.(\d+)(?:-rc\.(\d+))?$')


def version(tag):
    m = TAG.fullmatch(tag)
    if not m:
        return None
    a, b, c, rc = m.groups()
    return (int(a), int(b), int(c), 1 if rc is None else 0, int(rc or 0))


def latest_release(releases):
    candidates = [r for r in releases if not r.get('draft') and version(r.get('tag_name', ''))]
    return max(candidates, key=lambda r: version(r['tag_name']), default=None)


def run(*args, check=True):
    return subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=check)


def gh(*args):
    return run('gh', *args).stdout


def git(*args, check=True):
    return run('git', *args, check=check)


def notice(title, body):
    repo = os.environ['GITHUB_REPOSITORY']
    issues = json.loads(gh('issue', 'list', '--repo', repo, '--state', 'open', '--limit', '100', '--json', 'title,number'))
    if not any(i['title'] == title for i in issues):
        gh('issue', 'create', '--repo', repo, '--title', title, '--body', body)


def main():
    repo = os.environ['GITHUB_REPOSITORY']
    base = os.environ.get('DEFAULT_BRANCH', 'master')
    releases = json.loads(gh('api', '--paginate', '--slurp', f'repos/{UPSTREAM}/releases?per_page=100'))
    release = latest_release([r for page in releases for r in page])
    if not release:
        raise RuntimeError('No supported published upstream release found')
    tag = release['tag_name']
    git('fetch', '--no-tags', 'https://github.com/' + UPSTREAM + '.git', f'refs/tags/{tag}')
    upstream_sha = git('rev-parse', 'FETCH_HEAD^{commit}').stdout.strip()
    git('fetch', 'origin', f'+refs/heads/{base}:refs/remotes/origin/{base}')
    base_sha = git('rev-parse', f'origin/{base}').stdout.strip()
    ancestry = git('merge-base', '--is-ancestor', upstream_sha, base_sha, check=False)
    if ancestry.returncode == 0:
        print(f'{tag} ({upstream_sha}) is already included in {base}')
        return
    if ancestry.returncode != 1:
        raise RuntimeError(f'Cannot check upstream ancestry: {ancestry.stderr}')

    prs = json.loads(gh('pr', 'list', '--repo', repo, '--state', 'open', '--limit', '100', '--json', 'number,headRefName,headRefOid,body'))
    sync_prs = [p for p in prs if p['headRefName'] == BRANCH]
    if len(sync_prs) > 1:
        raise RuntimeError('Multiple sync PRs found')
    pr = sync_prs[0] if sync_prs else None
    old_sha = ''
    if pr:
        m = re.search(r'<!-- upstream-sync (\{[^\n]+\}) -->', pr.get('body') or '')
        metadata = json.loads(m.group(1)) if m else {}
        old_sha = metadata.get('head', '')
        if old_sha != pr['headRefOid']:
            notice(f'Upstream {tag} requires manual sync', f'A newer upstream release [{tag}]({release["html_url"]}) ({upstream_sha}) is available, but PR #{pr["number"]} has manual changes. Preserve those changes and update the PR manually.')
            return
        if metadata.get('upstream') == upstream_sha:
            print(f'PR #{pr["number"]} already tracks {tag}')
            return
    else:
        existing = git('ls-remote', '--heads', 'origin', f'refs/heads/{BRANCH}').stdout.strip()
        if existing:
            raise RuntimeError(f'{BRANCH} exists without an open sync PR; refusing to overwrite it')

    git('config', 'user.name', 'github-actions[bot]')
    git('config', 'user.email', '41898282+github-actions[bot]@users.noreply.github.com')
    git('checkout', '-B', BRANCH, base_sha)
    merge = git('merge', '--no-ff', '--no-edit', upstream_sha, check=False)
    if merge.returncode:
        files = git('diff', '--name-only', '--diff-filter=U').stdout.strip()
        git('merge', '--abort', check=False)
        notice(f'Upstream {tag} merge conflict', f'Cannot automatically merge [{tag}]({release["html_url"]}) (`{upstream_sha}`) into `{base}` (`{base_sha}`).\n\nConflicting files:\n```\n{files or "Merge failed before conflict paths were available"}\n```\n\nCreate a branch from the current default branch, merge the upstream tag, resolve the conflicts, run checks, and open a PR. Do not publish a release before merging it.')
        print(merge.stderr, file=sys.stderr)
        raise RuntimeError('Upstream merge failed; issue created or already exists')
    head = git('rev-parse', 'HEAD').stdout.strip()
    metadata = json.dumps({'tag': tag, 'upstream': upstream_sha, 'base': base_sha, 'head': head}, separators=(',', ':'))
    body = f'Upstream release: [{tag}]({release["html_url"]})\n\nUpstream commit: `{upstream_sha}`\nFork base: `{base_sha}`\n\nReview the changes and checks, then merge using a **merge commit**.\n\n{MARKER}{metadata} -->'
    lease = f'--force-with-lease=refs/heads/{BRANCH}:{old_sha}'
    git('push', lease, 'origin', f'HEAD:refs/heads/{BRANCH}')
    title = f'Sync upstream {tag}'
    if pr:
        gh('pr', 'edit', str(pr['number']), '--repo', repo, '--title', title, '--body', body)
    else:
        gh('pr', 'create', '--repo', repo, '--base', base, '--head', BRANCH, '--title', title, '--body', body)
    print(f'Prepared {tag} at {head}')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, ValueError) as exc:
        print(f'::error::{exc}', file=sys.stderr)
        if isinstance(exc, subprocess.CalledProcessError) and exc.stderr:
            print(exc.stderr, file=sys.stderr)
        sys.exit(1)
