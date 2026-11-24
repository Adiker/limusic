# Updater signing key transition

The first fork release after the repository migration uses a new updater signing key. Existing
installations trust the previous key and cannot verify an update signed by the new one. The first
new-key release therefore needs a one-time manual installation. Keep the previous repository and
its releases available for rollback; changing their names does not transfer stars or pull requests.

## Draft announcement for the transition release

> This release changes the key used to verify LiMusic updates. If you already have LiMusic
> installed, download the appropriate installer or package for your operating system from this
> release and install it manually. Your existing updater cannot install this release automatically.
> After this one-time installation, future updates signed with the new key can be verified by the
> new installation. Keep a backup of your LiMusic data before changing versions.
>
> This release is a release candidate. It is intended for users who have chosen to test prereleases;
> it does not replace the stable update channel.

Before publishing, adapt the announcement to the actual release and tested installation methods.
Do not promise preservation of application data until the relevant upgrade path has been tested.
The operator must verify a subsequent signed update with an installed transition build before
declaring the new updater path operational. If rollback is necessary, account for which key each
installed version trusts; changing the published download alone does not change that trust.
