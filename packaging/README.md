# Resguard DEB Packaging

This directory contains Debian packaging assets for `resguard`.

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
resguard_0.1.0_amd64.deb
```

## Install

```bash
sudo dpkg -i resguard_0.1.0_amd64.deb
resguard --help
```
