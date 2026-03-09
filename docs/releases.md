# Releases

This document defines the release path for Resguard.

## One-time publishing setup

Set up APT signing + GitHub Pages once:

```bash
./scripts/bootstrap-publishing.sh --repo 161sam/resguard
```

What this does (when `gh` auth is valid):

1. generates a dedicated APT signing keypair
2. uploads `RESGUARD_APT_GPG_PRIVATE_KEY` to GitHub Actions secrets
3. configures GitHub Pages for workflow deployment (best-effort)

If `gh` auth is unavailable, the script still generates key material and prints the exact remaining `gh` commands.
If local key generation is blocked (for example restricted `gpg-agent`), provide existing key material:

```bash
./scripts/bootstrap-publishing.sh \
  --repo 161sam/resguard \
  --private-key-file /path/to/RESGUARD_APT_GPG_PRIVATE_KEY.asc \
  --public-key-file /path/to/RESGUARD_APT_GPG_PUBLIC_KEY.asc
```

## Tag-and-go release flow

For each release:

1. bump versions and stage local artifacts:

```bash
./scripts/release.sh --version <x.y.z>
```

2. commit version bump:

```bash
git add crates/*/Cargo.toml packaging/deb/control
git commit -m "chore(release): cut v<x.y.z>"
```

3. tag + push:

```bash
git tag -a v<x.y.z> -m "resguard v<x.y.z>"
git push origin v<x.y.z>
```

On tag push (`v*`), GitHub Actions runs:

- `.github/workflows/release-upload.yml`
  - builds:
    - `resguard_<version>_amd64.deb`
    - `resguard_<version>_amd64_daemon.deb`
    - `SHA256SUMS`
  - uploads/overwrites assets on GitHub Release
- `.github/workflows/apt-pages.yml`
  - builds both `.deb` artifacts
  - generates signed APT metadata (`Release`, `InRelease`, `Release.gpg`, `Packages`, `Packages.gz`)
  - exports `pubkey.gpg`
  - deploys `apt/` to GitHub Pages

## Signing secret fallback behavior

If `RESGUARD_APT_GPG_PRIVATE_KEY` is missing:

- release assets are still published by `release-upload.yml`
- `APT Repository Pages` fails explicitly during key import (no unsigned APT publish)
- after adding the secret, rerun `APT Repository Pages` via `workflow_dispatch` with `release_tag=<tag>` and `source_ref=<ref>`

## Backfill an existing tag

Use this when a release tag already exists (for example `v0.3.0`) but release/APT assets were not published yet.

For each workflow (`Release Upload` and `APT Repository Pages`), run `workflow_dispatch` with:

- `release_tag`: existing tag to publish to (for example `v0.3.0`)
- `source_ref`: branch/commit containing the publishing automation (default `main`)

Notes:

- `source_ref` controls which scripts are checked out and executed.
- `release_tag` controls the target GitHub Release tag and expected version.
- workflows fail if `packaging/deb/control` version from `source_ref` does not match `release_tag` (for example source version `0.3.1` with `release_tag=v0.3.0`).

## GitHub Pages environment requirements

`APT Repository Pages` deploys through the `github-pages` environment (`deploy` job in `.github/workflows/apt-pages.yml`).

Required repository settings:

1. **Pages is enabled** and configured for **GitHub Actions** as source.
2. Environment `github-pages` exists (or is auto-created) and is not blocking the workflow actor.
3. If environment protection rules are enabled:
   - either allow tag refs used for releases (for example `refs/tags/v*`), or
   - allow the selected branch ref used for manual backfill (recommended: `main`).
4. Required reviewers (if configured) must approve the deployment run.

If you see:

`Tag "vX.Y.Z" is not allowed to deploy to github-pages due to environment protection rules`

then the release assets can still be published while APT Pages deploy is blocked by environment policy.

Recommended recovery/backfill (safe):

1. Keep `release_tag=vX.Y.Z` so uploaded assets and version checks stay tied to the intended release.
2. Run `APT Repository Pages` via `workflow_dispatch` with:
   - `release_tag=vX.Y.Z`
   - `source_ref=main`
3. Ensure `main` still contains `packaging/deb/control` version `X.Y.Z` (workflow enforces this).
4. Approve the environment deployment if reviewers are required.

This preserves version-consistency guarantees while avoiding blocked tag-based Pages deploys.

## Verification checklist

- `scripts/release.sh --dry-run` succeeds.
- `scripts/build-deb.sh` validates version consistency across crates and packaging.
- `release-upload.yml` and `apt-pages.yml` both enforce tag/version consistency.
