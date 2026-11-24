# Release migration — not ready for production

The automation in this branch is under review. The original repository still serves the live updater.
The release workflow is restricted to `Adiker/limusic` so it cannot publish from the staging copy.

## Signing and compatibility

The migration replaces the old Tauri signing key because its password is unavailable. Existing
installations cannot trust releases signed with the new key. Users need to install one release
manually before the new updater can work. Never publish an update signed with the new key as if it
were compatible with the old installs. The new encrypted key and password are backed up locally and
are configured as repository secrets; verify they survive any repository replacement. Do not commit
or print either secret.

## Before cutover

1. Review the upstream merge, offline downloads, and link entry; verify all six CI jobs.
2. Complete dry-run builds for Linux, Windows, macOS, Arch and RPM, including stable and RC.
3. Test the release coordinator end to end in an isolated repository with test keys: existing and
   conflicting tags; all five build successes; each build failure; partial manifests; failures and
   retries before and after publishing; concurrent uploads; signature checks; beta and stable channel
   ordering; releases created by a merged sync PR and by manual retry. Inspect actual artifacts.
4. Confirm that the automated sync PR can be created, its checks approved and run, conflicts reported,
   and manually resolved branches preserved. Test both a stable release and an RC.
5. Review release asset URLs and manifests after the destination has its final name. The updater
   endpoints are deliberately set to `Adiker/limusic`; the current staging repository has a different
   name, so end-to-end updater testing needs an isolated configuration or the completed cutover.
6. Prepare a documented, verified backup of every release and PR and confirm the key and password can
   be restored. Review the final name swap and preserve the original until the replacement is confirmed.
7. Publish the historical releases in the intended order only after verification, make sure the old
   updater and downloads remain available, then release the new-key version with clear manual-install
   instructions. Verify the `latest` and `beta` manifests, signatures and all platform packages.

The sync detector uses Actions' repository token; workflow runs for PRs it creates can require manual
approval. Repository settings must allow Actions to create pull requests, and issues must be enabled
for conflict reports. Protect the default branch and require the CI checks. The old
`upstream-release.yml` is removed by this branch because it bypasses the release gate.

The first synchronization PR must be merged before this workflow exists on the default branch, so
its closing event cannot trigger the new workflow. After merging both PRs and completing cutover,
dispatch `fork-release.yml` on `master` with the upstream `tag` and the synchronization PR's exact
`merge_sha`. The workflow validates the merged PR and tag before creating any release. Subsequent
sync PRs trigger it when merged. Add `fork-automation` to required checks once this branch is on
the default branch; requiring it earlier would block the first synchronization PR.

Run local checks with:

```
python3 -m unittest discover -s scripts/fork-release -v
python3 -m unittest discover -s scripts/upstream-sync -v
```

Private integration tests exercised real RC and stable publication with synthetic signed assets,
channel ordering, retry of an older RC and the GitHub Actions job graph for success, failure and an
already-published release. They do not replace a full run using real platform builds.
