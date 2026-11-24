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


def latest_releases(releases):
    candidates = [r for r in releases if not r.get('draft') and version(r.get('tag_name', ''))]
    newest = {}
    for release in candidates:
        channel = 'rc' if '-rc.' in release['tag_name'] else 'stable'
        if channel not in newest or version(release['tag_name']) > version(newest[channel]['tag_name']):
            newest[channel] = release
    return sorted(newest.values(), key=lambda r: version(r['tag_name']), reverse=True)


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


def find_pending(releases, base_sha):
    for release in releases:
        tag = release['tag_name']
        git('fetch', '--no-tags', 'https://github.com/' + UPSTREAM + '.git', f'refs/tags/{tag}')
        upstream_sha = git('rev-parse', 'FETCH_HEAD^{commit}').stdout.strip()
        ancestry = git('merge-base', '--is-ancestor', upstream_sha, base_sha, check=False)
        if ancestry.returncode == 1:
            return release, upstream_sha
        if ancestry.returncode != 0:
            raise RuntimeError(f'Cannot check upstream ancestry: {ancestry.stderr}')
        print(f'{tag} ({upstream_sha}) is already included')
    return None


def main():
    repo = os.environ['GITHUB_REPOSITORY']
    base = os.environ.get('DEFAULT_BRANCH', 'master')
    releases = json.loads(gh('api', '--paginate', '--slurp', f'repos/{UPSTREAM}/releases?per_page=100'))
    candidates = latest_releases([r for page in releases for r in page])
    if not candidates:
        raise RuntimeError('No supported published upstream release found')
    git('fetch', 'origin', f'+refs/heads/{base}:refs/remotes/origin/{base}')
    base_sha = git('rev-parse', f'origin/{base}').stdout.strip()
    pending = find_pending(candidates, base_sha)
    if not pending:
        print(f'Latest stable and RC releases are already included in {base}')
        return
    release, upstream_sha = pending
    tag = release['tag_name']
    source_config = json.loads(git('show', f'{base_sha}:src-tauri/tauri.conf.json').stdout)
    current = version('v' + source_config['version'])
    if current is None:
        raise RuntimeError('Current app version is not a supported stable or RC version')
    if version(tag) < current:
        notice(f'Upstream {tag} requires manual sync',
               f'[{tag}]({release["html_url"]}) (`{upstream_sha}`) is not included in `{base}` '
               f'but is older than the current application version `{source_config["version"]}`. '
               'Review and integrate its changes without downgrading the release version.')
        return

    prs = json.loads(gh('pr', 'list', '--repo', repo, '--state', 'open', '--limit', '100', '--json', 'number,headRefName,headRefOid,body'))
    sync_prs = [p for p in prs if p['headRefName'] == BRANCH or p['headRefName'].startswith('codex/sync-upstream-')]
    if len(sync_prs) > 1:
        raise RuntimeError('Multiple sync PRs found')
    pr = sync_prs[0] if sync_prs else None
    old_sha = ''
    if pr:
        if pr['headRefName'] != BRANCH:
            notice(f'Upstream {tag} requires manual sync', f'Existing sync PR #{pr["number"]} has manual changes. Review it before syncing [{tag}]({release["html_url"]}) (`{upstream_sha}`).')
            return
        m = re.search(r'<!-- upstream-sync (\{[^\n]+\}) -->', pr.get('body') or '')
        metadata = json.loads(m.group(1)) if m else {}
        old_sha = metadata.get('head', '')
        if old_sha != pr['headRefOid']:
            notice(f'Upstream {tag} requires manual sync', f'Upstream release [{tag}]({release["html_url"]}) ({upstream_sha}) needs review, but PR #{pr["number"]} has manual changes. Preserve those changes and update the PR manually if needed.')
            return
        if metadata.get('upstream') == upstream_sha:
            print(f'PR #{pr["number"]} already tracks {tag}')
            return
    else:
        existing = git('ls-remote', '--heads', 'origin', f'refs/heads/{BRANCH}').stdout.strip()
        if existing:
            notice('Upstream sync branch needs manual recovery',
                   f'Branch `{BRANCH}` exists without an open synchronization PR. '
                   'This can happen if PR creation failed after a successful push. '
                   'Review the branch and open a PR or remove it before retrying; it will not be overwritten.')
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
    pushed = git('push', lease, 'origin', f'HEAD:refs/heads/{BRANCH}', check=False)
    if pushed.returncode:
        if 'workflow' in pushed.stderr.lower() and ('permission' in pushed.stderr.lower() or 'scope' in pushed.stderr.lower()):
            notice(f'Upstream {tag} needs workflow permission',
                   f'GitHub rejected the push for [{tag}]({release["html_url"]}) (`{upstream_sha}`) '
                   'because it modifies workflow files. Configure the repository-scoped '
                   '`UPSTREAM_SYNC_TOKEN` as documented, or merge the tag manually.')
        raise RuntimeError(f'Unable to push sync branch: {pushed.stderr}')
    title = f'Sync upstream {tag}'
    if pr:
        gh('pr', 'edit', str(pr['number']), '--repo', repo, '--title', title, '--body', body)
    else:
        try:
            gh('pr', 'create', '--repo', repo, '--base', base, '--head', BRANCH, '--title', title, '--body', body)
        except subprocess.CalledProcessError:
            notice('Upstream sync branch needs manual recovery',
                   f'The branch `{BRANCH}` for [{tag}]({release["html_url"]}) was pushed, '
                   'but the PR could not be created. Check Actions PR permissions and open a PR '
                   f'targeting `{base}` or remove the branch before retrying.')
            raise
    print(f'Prepared {tag} at {head}')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, ValueError) as exc:
        print(f'::error::{exc}', file=sys.stderr)
        if isinstance(exc, subprocess.CalledProcessError) and exc.stderr:
            print(exc.stderr, file=sys.stderr)
        sys.exit(1)
