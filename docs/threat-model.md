# Resguard Threat Model (Short)

## Scope

In scope:

- `resguard` CLI (profile/apply/rollback/run/setup/suggest/desktop flows)
- optional `resguardd` watchdog
- systemd unit/drop-in writes under managed paths

Out of scope:

- kernel/cgroup implementation bugs
- compromised root account
- security of third-party desktop applications

## Assets to Protect

- system responsiveness under pressure
- integrity of systemd configuration managed by Resguard
- recoverability (rollback/revert)
- operator trust via auditable logs/state

## Attacker Model

Potential attacker can:

- run unprivileged workloads
- generate CPU/memory pressure spikes
- attempt to provoke repeated watchdog actions
- provide malformed/unexpected profile/config content

## Entry Points

- profile/config YAML input
- runtime process metadata (scope names, ExecStart, pressure data)
- privileged command execution (`systemctl`, `systemd-run`)
- daemon service lifecycle signals (SIGINT/SIGTERM)

## Key Controls

- strict parsing and config validation
- direct exec (no shell interpolation)
- transactional apply + manifest backups + rollback
- watchdog hold/cooldown gating
- action restrictions (`panic` or bounded `set-property` only)
- `set-property` revert on completion and best-effort early revert on termination
- action ledger (`${state_dir}/daemon-ledger.jsonl`) for postmortem/audit
- hardened systemd service unit for daemon

## Residual Risks

- misconfigured thresholds may still cause operational disruption
- `panic` action can degrade user-session behavior during emergency windows
- revert may fail if systemd is unavailable or denies property writes

## Operational Recommendations

1. Test with `--dry-run` and staged roots first.
2. Use `resguardd --once` for decision-path checks before enabling service.
3. Monitor ledger and journald for repeated triggers/action failures.
4. Keep daemon disabled unless explicitly needed.
