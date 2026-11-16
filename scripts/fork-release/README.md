# Fork release gate (draft)

This branch is a draft and must not be merged or used for production releases yet.
It is based on the upstream-sync branch. The legacy `upstream-release.yml` still runs and can
create releases without the new gate. The new PR discovery is deliberately manual-only.

The intended gate creates a tag only for the merge commit of an upstream-sync PR, creates a draft,
then builds Linux, Windows, macOS, RPM and Arch. Promotion validates assets and signed manifests.
Retry of a published release skips asset rebuilds and rechecks promotion; tags are never moved.

Before enabling: replace the legacy workflow; review the trust and provenance checks; integrate
recovery for manual conflict PRs; exercise draft creation, all five builds (including an RC RPM),
failed jobs, retries, manifest races, beta creation and promotion end-to-end in a disposable repo;
confirm the Actions-created PR workflow approval behavior and enable Actions PR creation; configure
branch protection; document the full operation. GitHub Actions currently returns HTTP 422 for
this repository despite its settings reporting enabled, so no remote integration test was possible.

Run `python3 -m unittest discover -s scripts/fork-release -v` and
`python3 -m unittest discover -s scripts/upstream-sync -v` for the local checks.
