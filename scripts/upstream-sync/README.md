# Upstream release synchronization

The workflow checks published stable releases and release candidates hourly or on manual dispatch.
It selects the highest supported semantic version and prepares a merge commit on
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
reports. Do not run production release automation until the cutover and signing transition in
`../fork-release/README.md` are complete. Run the detector tests using
`python3 -m unittest discover -s scripts/upstream-sync -v`.
