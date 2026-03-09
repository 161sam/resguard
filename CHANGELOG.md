# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Planned (v0.4.0)
- TUI visualizer for pressure/slice/cgroup observability.
- Optional freeze watchdog with guarded panic actions and explicit safety controls.

### Fixed
- Debian packaging now ships a real separate optional daemon package `resguard-daemon` instead of overloading the `resguard` package artifact.

## [0.4.0] - 2026-03-09

### Added
- Policy autopilot decision engine with conservative memory/cpu pressure actions and cooldown gating.
- Runtime adaptive class-limit apply/revert flow with typed operations and no-op handling.
- Service-level daemon autopilot orchestration (`observe -> decide -> act`) used by `resguardd`.
- Improved feature-gated TUI view with pressure summary, class slice limits/usage, and recent daemon ledger actions.

### Changed
- `resguardd` now delegates autopilot logic through services instead of local ad-hoc watchdog logic.
- Daemon ledger records are structured around tick/decision/actions for clearer operator auditing.
- Release/install docs now explicitly describe package split: core package vs optional daemon package, and source-only TUI feature status.
- Packaging metadata/version bumped to `0.4.0`.

## [0.3.0] - 2026-03-08

### Added
- Practical `suggest --apply` flow for common Ubuntu Snap desktop apps with threshold-gated auto-wrap behavior.
- Stronger desktop doctor guidance for session/launcher refresh after wrapper and slice changes.
- Output hardening tests for suggest planning/apply behavior and metrics/doctor/status formatting helpers.

### Changed
- Desktop discovery now includes Ubuntu Snap desktop path handling and safe alias resolution for common IDs (for example `firefox.desktop` -> `firefox_firefox.desktop` when unique).
- Wrapper rendering now consistently forces `DBusActivatable=false` for wrapped desktop entries to avoid launcher bypass of wrapper `Exec`.
- `doctor`, `status`, and `metrics` output now use more stable sections and clearer action hints while retaining script-friendly key/value lines.
- Packaging metadata/version bumped to `0.3.0`.

### Docs
- Updated install/release docs and v0.3.0 hardening checklist to reflect real Ubuntu field results and release readiness decisions.

## [0.2.1] - 2026-03-07
