# Repository cutover and updater key transition

This is an operator checklist for the migration from the old fork to the standalone repository.
The old repository remains the live source until the cutover. Do not publish staging releases or
change updater channels while preparation and tests are incomplete.

## Before the cutover

1. Review and merge the current synchronization PR and the release automation PR using merge
   commits, after their required checks pass. The first release must be started manually because
   the synchronization PR was merged before the new release workflow reached the default branch.
2. Recompare all source branches, tags, releases and asset SHA-256 digests with the staging copy.
   Refresh the local Git bundle and PR/comment metadata if anything changed. Confirm the original
   seven releases and all their assets are present as drafts, including the signed `latest.json`.
3. Confirm the new updater signing key and its password can be restored, both repository secrets
   exist, and the public key in the new app matches. Keep a protected backup away from the machine.
4. Validate all platform builds, signatures, the complete promotion workflow and recovery from a
   failed build. Keep upstream synchronization and old release schedules disabled until ready.
5. Prepare a clear manual-install announcement. The old key cannot sign the transition and an
   installed app that trusts the old key cannot verify releases signed by the new key. Existing
   users must install a new-key build manually before automatic updates resume.
6. Inventory the current repository IDs, name, settings and existing PR links. A name swap changes
   which repository is served at `Adiker/limusic`; historic PR numbers and stars remain associated
   with the original repository, not the replacement. Reusing the old name prevents GitHub from
   redirecting old links to the renamed original. See GitHub's repository renaming documentation.
7. Confirm `UPSTREAM_SYNC_TOKEN` is present, has not expired, and has Contents and Workflows write
   access to the standalone repository. After the rename, verify its access and repeat the sync
   test before relying on unattended operation.
   GitHub does not reveal the value of an existing Actions secret: keep a separate recovery plan.
8. Perform a hands-on regression pass with the built transition application: download tracks for
   offline use, play them without a network connection, and open supported links. Automated CI and
   package builds do not establish that these interactive paths work on a user's machine.

## Cutover

1. Rename the old fork to an available, clearly marked legacy name, preserving it and its releases.
   Rename the prepared standalone repository to `Adiker/limusic`. Check the repository IDs after
   each rename and stop if either operation fails. Do not delete the legacy repository.
2. Restore the historical drafts as releases after checking their tag and asset digests, marking only
   `v1.3.0` as Latest. Verify the final `releases/latest/download/latest.json` and asset URLs and
   their old signatures before changing either update channel. Publication can notify subscribers.
3. Verify the Actions secrets, branch protection, permissions and disabled workflows after the
   rename. Once the release automation PR is on `master`, add `fork-automation` to the required
   status checks; adding it before that workflow reaches the base branch can block the first PR.
   Enable only the new upstream-sync automation after the release gate is reviewed.
4. Manually dispatch `fork-release.yml` on `master` using the exact upstream tag and merge commit
   SHA from the first synchronization PR. Monitor all five builds and the promotion job. RCs must
   remain prereleases and may update only the beta pointer; do not force a failed release through.
5. Verify the public release assets and updater manifests from an independent client. Distribute the
   manual transition build with instructions. Verify an installed transition build accepts a
   subsequent test update signed by the new key before declaring the updater functional.

Renaming back is a recovery option, but it does not automatically repair update trust for people who
already installed a new-key build. Treat rollback after a transition release as a separate migration
and check which key each installed version trusts before changing published channels.
