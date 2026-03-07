# Resguard DEB Packaging

This directory contains Debian packaging assets for `resguard`.
The package ships `resguardd` assets, but the daemon stays disabled unless explicitly enabled by the operator.

## Install layout

The DEB installs and/or ensures these paths:

- `/usr/bin/resguard`
- `/usr/bin/resguardd`
- `/etc/resguard/`
- `/etc/resguard/profiles/`
- `/etc/systemd/system/resguardd.service` (installed by `postinst`)
- `/var/lib/resguard/`
- `/usr/share/doc/resguard/`

## Files

- `packaging/deb/control` - package metadata
- `packaging/deb/postinst` - installs service/config templates into `/etc`, runs `systemctl daemon-reload`, does not enable/start daemon
- `packaging/deb/prerm` - best-effort `stop/disable` for `resguardd` on remove/purge
- `packaging/systemd/resguardd.service` - hardened service unit template

## Build

```bash
./scripts/build-deb.sh
```

Expected output artifact:

```bash
resguard_0.2.1_amd64.deb
```

Build CLI-only package (without daemon assets):

```bash
RESGUARD_DEB_WITH_DAEMON=0 ./scripts/build-deb.sh
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
sudo dpkg -i resguard_0.2.1_amd64.deb
resguard --help
```
