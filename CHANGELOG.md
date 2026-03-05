# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Planned (v0.2)
- Desktop Wrapper Generator implementation (`desktop list/wrap/unwrap/doctor`) with safe XDG discovery and managed wrapper lifecycle.
- Wrapper Exec rewrite preserving `%u/%U/%f/%F` semantics without shell wrapping.
- Wrapper diagnostics and integrity checks (marker validation, duplicate/stale wrapper detection).

### Planned (v0.3)
- Auto-classification mapping model (`desktop-id -> class`) with audit-friendly updates.
- Suggestions engine for class mapping recommendations.
- Optional `resguardd` hooks for richer event input (not required for core workflows).

### Planned (v0.4)
- TUI visualizer for pressure/slice/cgroup observability.
- Optional freeze watchdog with guarded panic actions and explicit safety controls.

## [0.1.0] - Unreleased

### Added
- Initial Rust workspace scaffold with crates for CLI, core, system, config, and state.
- CLI command scaffold aligned to v0.1 spec (`init`, `profile`, `apply`, `diff`, `rollback`, `status`, `run`).
- Profile schema v1 (v0.1 fields), parsing/validation helpers, and profile load/save in config store.
- `init` hardware detection, profile generation, dry-run output, and profile write behavior by privilege level.
- Planner + apply pipeline for systemd drop-ins and class slices (system + user) with `--root` support.
- Transactional state/backups with rollback support and apply failure rollback attempt.
- `run --class` execution via `systemd-run` with user/system mode selection and `--wait` support.
- Minimal `status` command with state summary, systemd slice properties, oomd status, and PSI avg60 diagnostics.
- GitHub Actions CI quality gates for fmt, clippy, and tests.
