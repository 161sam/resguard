
# Security Policy

## Overview

Resguard modifies systemd resource controls and can optionally run `resguardd` (freeze watchdog).
Security and safety are core design constraints:

- explicit system changes
- rollback/revert first
- minimal privileges and hardening by default

## Supported Versions

Currently supported:

| Version | Supported |
|-------|--------|
| 0.2.x | yes |

## Reporting Security Issues

Please report security issues privately to `security@resguard.dev`.

Include:

- affected version
- reproduction steps
- relevant logs

Do not open public issues for security vulnerabilities.

## Threat Model

Assumptions:

- attacker may control user processes and workload behavior
- attacker may attempt CPU/memory pressure exhaustion
- attacker may try to trigger unsafe watchdog actions repeatedly

Security goals:

- preserve system responsiveness
- keep privileged services operational
- keep actions auditable and reversible

See [docs/threat-model.md](docs/threat-model.md) for concrete boundaries.

## Attack Surfaces and Controls

### Profile and Config Files

Inputs include profile YAML and daemon YAML.

Controls:

- strict parsing/validation
- bounded numeric values in daemon config:
  - `memory_avg10_threshold`, `cpu_avg10_threshold` in `(0,100]`
  - `hold_seconds`, `cooldown_seconds`, `action_duration_seconds` > `0`
  - `poll_interval_ms >= 200`
- action type restricted to known enum values (`panic`, `set-property`)

### Command Execution

Resguard executes `systemctl`, `systemd-run`, and (for watchdog panic) `resguard panic`.

Controls:

- direct command execution (no `sh -c` / `bash -c`)
- fixed command names and explicit arguments
- no shell interpolation of untrusted input

### File Writes

Resguard writes only managed paths and state files (including daemon ledger):

- `/etc/systemd/system/...` and `/etc/systemd/user/...` managed units/drop-ins
- `/var/lib/resguard/...` state/backup files
- `/var/lib/resguard/daemon-ledger.jsonl` (or configured `--state-dir`)

Controls:

- transactional apply model for profile changes
- backups + manifests + rollback
- no overwrite of unrelated files

### Daemon Surface (`resguardd`)

`resguardd` is optional and not auto-enabled by packaging.

Service hardening includes:

- `NoNewPrivileges=true`
- `ProtectSystem=strict`
- `ProtectHome=true`
- `PrivateTmp=true`
- `CapabilityBoundingSet=`
- `RestrictAddressFamilies=AF_UNIX`
- `LockPersonality=true`
- `MemoryDenyWriteExecute=true`

## Action Restrictions and Revert Guarantees

Watchdog action restrictions:

- finite cooldown and hold gating to avoid action thrash
- `--once` mode for single-cycle decision tests without long-running daemon

Revert behavior:

- `set-property` action stores prior `MemoryHigh/MemoryMax` and reverts after duration
- on SIGINT/SIGTERM during active `set-property` window, daemon attempts early revert before exit
- ledger records include `revert_ok` for auditability

## Privilege Model

Commands requiring root (system scope):

- `apply`
- `rollback`
- `panic`

Daemon behavior depends on systemd property writes and is expected to run with sufficient privileges.

## Responsible Use

Before applying on production systems:

1. `resguard apply <profile> --dry-run`
2. `resguard setup --suggest` (preview wrappers/suggestions)
3. review rollback path: `resguard rollback --last`
