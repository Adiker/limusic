# Upstream release synchronization

The workflow checks the newest published stable release and release candidate hourly or on manual
dispatch, starting with the higher version. If an older release is not in the current history,
it opens an issue for manual integration without downgrading the application. Otherwise it prepares a merge commit on
`automation/upstream-release`. Review the PR, approve any Actions runs created with `GITHUB_TOKEN`,
and merge with a merge commit after CI passes. Never squash or rebase a synchronization PR: the
upstream commit must remain in the history.

A manually maintained `codex/sync-upstream-*` PR or an automation branch with extra commits is
never overwritten. The workflow opens an issue when a newer release requires manual handling or a
merge has conflicts. Resolve the conflict on a branch from the latest default branch, merge the
exact upstream tag, run the checks and open a PR named `codex/sync-upstream-<version>`. When a
newer tag arrives while the automation branch has not been manually modified, it updates the same
PR with a force-with-lease push.

GitHub settings must allow Actions to create pull requests. Issues must be enabled for conflict
reports. If upstream changes `.github/workflows/`, GitHub can reject a push made with `GITHUB_TOKEN`.
For unattended sync, set `UPSTREAM_SYNC_TOKEN` to a fine-grained token scoped only to this repository,
with repository permissions **Contents: read and write** and **Workflows: read and write**. The token
is used only by checkout/git; issue and pull-request API calls still use `GITHUB_TOKEN`. Without it,
workflow-changing syncs need manual handling; the detector reports the rejected push in an issue.
Do not run production release automation until the cutover and signing transition in
`../fork-release/README.md` are complete. Run the detector tests using
`python3 -m unittest discover -s scripts/upstream-sync -v`.
