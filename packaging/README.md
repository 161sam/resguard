# Resguard DEB Packaging

This directory contains Debian packaging assets for two packages:

- `resguard` (core CLI)
- `resguard-daemon` (optional daemon package)

## Install layout

`resguard` installs and/or ensures these paths:

- `/usr/bin/resguard`
- `/etc/resguard/`
- `/etc/resguard/profiles/`
- `/var/lib/resguard/`
- `/usr/share/doc/resguard/`

`resguard-daemon` installs and/or ensures these paths:

- `/usr/bin/resguardd`
- `/usr/share/resguard-daemon/systemd/resguardd.service`
- `/usr/share/resguard-daemon/resguardd.yml`
- `/etc/systemd/system/resguardd.service` (installed by `postinst`)
- `/etc/resguard/resguardd.yml` (created only if missing)

## Files

- `packaging/deb/core/control|postinst|prerm` - core package metadata/scripts
- `packaging/deb/daemon/control|postinst|prerm` - daemon package metadata/scripts
- `packaging/systemd/resguardd.service` - service unit template
- `packaging/etc/resguard/resguardd.yml` - daemon config template

## Build

```bash
RESGUARD_DEB_PACKAGE=core ./scripts/build-deb.sh
RESGUARD_DEB_PACKAGE=daemon ./scripts/build-deb.sh
```

Expected output artifacts:

```bash
resguard_<version>_amd64.deb
resguard-daemon_<version>_amd64.deb
```

Service enablement remains explicit (`systemctl enable/start` by operator).

## GitHub Pages APT repository

Generate a Pages-ready APT repository layout under `apt/`:

```bash
./scripts/generate-apt-repo.sh \
  --repo-dir ./apt \
  --input-dir ./release-assets \
  --distribution stable \
  --component main \
  --arch amd64
```

Generate signed metadata (`Release.gpg`, `InRelease`) and export `pubkey.gpg`:

```bash
./scripts/generate-apt-repo.sh \
  --repo-dir ./apt \
  --input-dir ./release-assets \
  --distribution stable \
  --component main \
  --arch amd64 \
  --sign-key <GPG_KEY_ID> \
  --export-pubkey ./apt/pubkey.gpg
```

The workflow `.github/workflows/apt-pages.yml` deploys `apt/` to GitHub Pages on version tags (`v*`).
Required repository secrets:

- `RESGUARD_APT_GPG_PRIVATE_KEY` (ASCII armored private key for signing)
- `RESGUARD_APT_GPG_PASSPHRASE` (optional, only for passphrase-protected keys)

Bootstrap one-time signing/pages setup:

```bash
./scripts/bootstrap-publishing.sh --repo 161sam/resguard
```

If GitHub auth is unavailable, the script prints the exact remaining `gh secret set` / Pages commands.

## Install

```bash
sudo dpkg -i resguard_<version>_amd64.deb
sudo dpkg -i resguard-daemon_<version>_amd64.deb
resguard --help
resguardd --help
```
