# Resguard Design (v0.1 implementation + v0.2+ direction)

## Ziel

Resguard hält Linux-Systeme unter Last bedienbar, indem es systemd slices konfiguriert und Workloads in Klassen trennt.

## Architektur

Workspace:

- `resguard-cli`: CLI + command dispatch
- `resguard-core`: profile model, validation, planner, renderers
- `resguard-system`: Linux/systemd adapters (`systemctl`, `systemd-run`, `/proc`)
- `resguard-config`: profile load/save
- `resguard-state`: transactional state, backups, rollback manifest

## Pfadmodell

Defaults:

- `config_dir = /etc/resguard`
- `state_dir = /var/lib/resguard`
- `root = /`

`--root` applies as filesystem prefix for config/state/systemd managed files.

Beispiel:

- `--root /tmp/rgtest --config-dir /etc/resguard`
- effective profile path: `/tmp/rgtest/etc/resguard/profiles/*.yml`

## Profile Modell (aktuell)

`apiVersion: resguard.io/v1`, `kind: Profile`

- `spec.memory` (`memoryLow`, `memoryHigh`, `memoryMax`)
- `spec.cpu` (`systemAllowedCpus`, `userAllowedCpus`, ...)
- `spec.oomd` (`memoryPressure`, `memoryPressureLimit`)
- `spec.classes` (+ compatibility `spec.slices.classes`)

## Apply Pipeline

1. load profile (`config_dir/profiles/<name>.yml`)
2. validate
3. build plan (`EnsureDir`, `WriteFile`, `Exec`)
4. dry-run prints plan only
5. real apply snapshots writes, applies actions, updates state/manifest

Generated managed files:

- `${root}/etc/systemd/system/system.slice.d/50-resguard.conf`
- `${root}/etc/systemd/system/user.slice.d/50-resguard.conf`
- `${root}/etc/systemd/system/resguard-<class>.slice`
- `${root}/etc/systemd/user/resguard-<class>.slice`

`systemctl daemon-reload` is executed only when `root == "/"`.

## State + Backups

`state_dir`:

- `state.json`
- `backups/<backup_id>/manifest.json`
- `backups/<backup_id>/...` (backed-up originals)

`state.json` fields:

- `activeProfile`
- `backupId`
- `managedPaths`
- `createdPaths`

Rollback restores backed-up files, removes `createdPaths`, and clears state.

## Run (`run --class`)

Slice resolution:

- `--slice` override, else class from profile
- profile source: `--profile` or `state.json.activeProfile`

Execution:

- root: `systemd-run --scope ...`
- non-root: `systemd-run --user --scope ...`
- `--wait` forwards command exit code

## Status/Metrics/Doctor

- `status`: best-effort slice/state/oomd/PSI summary
- `metrics`: PSI + memory + slice usage snapshots
- `doctor`: common setup diagnostics and hints

---

## Classification Model (v0.2/v0.3)

Design direction:

1. wrappers-first in v0.2
   - launcher integration via managed `.desktop` wrappers
   - explicit class assignment per wrapper
2. rules mapping in v0.3
   - persistent mapping `desktop-id -> class`
   - deterministic and user-auditable mapping updates
3. suggestions in v0.3
   - suggest mappings from observed usage patterns
   - no forced auto-apply by default

Boundary:

- no daemon is required for wrapper-based classification in v0.2.
- optional daemon hooks in v0.3 are additive, not mandatory.

---

## TUI Visualizer (v0.4)

Planned TUI data sources:

- `/proc/pressure/*` (CPU/memory/io pressure)
- `systemctl show` (slice properties and runtime state)
- cgroup filesystem (`/sys/fs/cgroup`) for usage hierarchy

Planned behavior:

- read-only observability first
- no hidden writes from TUI
- explicit action handoff to CLI commands for system changes

---

## Freeze Watchdog (v0.4, optional)

Planned watchdog signals:

- PSI threshold breaches over bounded windows
- kernel/user-space stall indicators where available

Planned safe panic action:

- bounded temporary restriction (e.g. user.slice pressure limits)
- cooldown and rate limiting
- explicit audit trail and opt-in enablement

Boundary:

- watchdog automation is optional and begins in v0.4.
- core resguard functionality remains daemonless and fully usable without watchdog.
