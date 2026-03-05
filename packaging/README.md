# Resguard DEB Packaging

This directory contains Debian packaging assets for `resguard`.
Default package scope is CLI-only. `resguardd` stays optional and is not enabled automatically.

## Install layout

The DEB installs and/or ensures these paths:

- `/usr/bin/resguard`
- `/etc/resguard/`
- `/etc/resguard/profiles/`
- `/var/lib/resguard/`
- `/usr/share/doc/resguard/`

## Files

- `packaging/deb/control` - package metadata
- `packaging/deb/postinst` - creates runtime/config directories with `root:root` and `0755`
- `packaging/deb/prerm` - pre-remove hook (non-destructive)

## Build

```bash
./scripts/build-deb.sh
```

Expected output artifact:

```bash
resguard_0.2.1_amd64.deb
```

Optional daemon binary (`resguardd`) can be included at build time:

```bash
RESGUARD_DEB_WITH_DAEMON=1 ./scripts/build-deb.sh
```

This only ships the binary/config. Service enablement remains explicit (`systemctl enable/start` by operator).

## Install

```bash
sudo dpkg -i resguard_0.2.1_amd64.deb
resguard --help
```
