# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Planned (v0.3.0)
- Auto-classification mapping model (`desktop-id -> class`) with audit-friendly updates.
- Suggestions engine for class mapping recommendations.
- Optional `resguardd` hooks for richer event input (not required for core workflows).

### Planned (v0.4.0)
- TUI visualizer for pressure/slice/cgroup observability.
- Optional freeze watchdog with guarded panic actions and explicit safety controls.

## [0.2.1] - Unreleased

### Added
- Desktop wrapper generator commands: `desktop list`, `desktop wrap`, `desktop unwrap`, `desktop doctor`.
- XDG desktop discovery (`/usr/share/applications`, `/usr/local/share/applications`, `~/.local/share/applications`).
- Wrapper generation with safe side-by-side IDs (`<desktop_id>.resguard-<class>.desktop`) and mapping store at `~/.config/resguard/desktop-mapping.yml`.
- Desktop wrapper safety hardening (desktop ID validation, wrapper path containment, safe unwrap behavior).
- Integrated diagnostics: `doctor` now includes desktop checks when mappings exist and prints actionable fix commands.

### Changed
- CLI/manpage/docs aligned for desktop wrapper workflows in v0.2.
- `resguard` remains on `0.2.x` for bugfix/stabilization.
- `tui` stays feature-gated behind `--features tui`.
- `resguardd` build/install path remains optional (not always-on by default).
- Packaging metadata/version bumped to `0.2.1`.

### Docs
- Added release process doc: `docs/releases.md` (tagging, changelog, deb build).
