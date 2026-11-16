# Upstream PR automation (work in progress)

This implementation is incomplete. Do not enable its schedule or merge it as a replacement
for the release workflow yet. It prepares a pull request only; it does not build, publish, or
promote fork releases. The existing `upstream-release.yml` still has its previous behavior.

Run `python3 -m unittest discover -s scripts/upstream-sync -v` for isolated checks. The workflow
is manual-only while release coordination and integration tests are missing. Before a real run,
GitHub Actions must be allowed to create pull requests; PR checks created with `GITHUB_TOKEN`
need a maintainer's workflow approval.

Before production: coordinate releases with the exact merged commit; prevent platform jobs from
promoting updates early; support RC versions in the RPM and Arch jobs; add a single promotion
gate and retry handling; integrate conflict resolution; protect the default branch; test the
end-to-end workflow without publishing production updates. The fork currently has no `beta`
release, so the beta updater pointer also needs initialization and verification before an RC.
