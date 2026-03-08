# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Planned (v0.4.0)
- TUI visualizer for pressure/slice/cgroup observability.
- Optional freeze watchdog with guarded panic actions and explicit safety controls.

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
