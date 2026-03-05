# Releases

This document describes the release cut flow for Resguard.

## Scope and Version Gates

- `resguard` CLI is currently in the `0.2.x` line (bugfix/stabilization).
- `tui` remains feature-gated and must be built explicitly with `--features tui`.
- `resguardd` is optional and must never be assumed as always-on.
- Planned milestones:
  - `v0.3.0`: classification/suggestions + optional daemon hooks
  - `v0.4.0`: TUI visualizer + optional freeze watchdog

## 1. Prepare Version + Changelog

1. Update crate/package version fields to the target version (for `0.2.x` bugfix releases: `0.2.N`).
2. Update `CHANGELOG.md`:
   - move shipped changes into the release section
   - keep future work in `Planned (v0.3.0)` / `Planned (v0.4.0)`
3. Run checks:

```bash
cargo build
cargo build --features tui
cargo build -p resguard-daemon
cargo test
```

## 2. Build Debian Artifact

Default (CLI only):

```bash
./scripts/build-deb.sh
```

Optional daemon binary in package:

```bash
RESGUARD_DEB_WITH_DAEMON=1 ./scripts/build-deb.sh
```

Notes:

- `resguardd` inclusion is packaging-only optionality.
- No automatic service enablement should be added in packaging scripts.

## 3. Cut Tag

```bash
git checkout master
git pull --ff-only

git tag -a v0.2.1 -m "resguard v0.2.1"
git push origin v0.2.1
```

For the next bugfix release, use the matching `v0.2.N` tag.

## 4. Release Verification Checklist

- `cargo build` (default features) passes.
- `cargo build --features tui` passes.
- `cargo build -p resguard-daemon` passes.
- Built DEB artifact version matches `packaging/deb/control`.
- `CHANGELOG.md` and roadmap docs mention `v0.3.0` / `v0.4.0` split correctly.
