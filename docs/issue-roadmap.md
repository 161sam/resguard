# Resguard Roadmap

## Version Overview

- v0.1: Agentless core (implemented baseline)
- v0.2: Desktop Wrapper Generator (fully implemented, not only stubs)
- v0.3.0: Auto-classification and suggestions (daemon optional)
- v0.4.0: Power UX and resilience (TUI + optional watchdog)

---

## Milestone v0.1 — Agentless Core

Goal:

- systemd is permanent enforcement layer
- resguard performs explicit, rollbackable config/state changes
- `run --class` launches workloads into slices

Deliverables:

- profile schema + validation
- init/apply/diff/rollback/status/run commands
- transactional backups + state file
- testable `--root` isolation for integration tests

Status: baseline implemented.

---

## Milestone v0.2 — Desktop Wrapper Generator

Goal:

- users launch apps from desktop/launcher as usual
- wrappers route selected apps through `resguard run --class ...`
- no daemon required

### Issue #23 — XDG discovery engine

- resolve desktop entries across:
  - `$XDG_DATA_HOME/applications` (fallback `~/.local/share/applications`)
  - `/usr/local/share/applications`
  - `/usr/share/applications`
  - `$XDG_DATA_DIRS/*/applications`
- detect effective entry precedence and duplicates

### Issue #24 — `resguard desktop list`

- list available desktop entries with source path and effective desktop-id
- expose conflicts/overrides in output

### Issue #25 — `resguard desktop wrap`

- create managed wrapper in user-local applications dir
- preserve and rewrite `Exec` safely (no shell wrapping)
- preserve placeholder semantics: `%u/%U/%f/%F`
- set wrapper markers:
  - `X-Resguard-Managed=true`
  - `X-Resguard-SourceDesktopId=...`
  - `X-Resguard-Class=...`

### Issue #26 — `resguard desktop unwrap`

- remove only managed wrapper files
- never mutate/remove non-managed upstream desktop files

### Issue #27 — `resguard desktop doctor`

- validate wrapper integrity and marker consistency
- validate class/slice readiness
- detect stale wrappers and duplicate wrappers

Exit criteria for v0.2:

- wrapper workflow (`list/wrap/unwrap/doctor`) works end-to-end
- launcher-driven apps can start through resguard classes
- no daemon required

---

## Milestone v0.3.0 — Auto-classification and Suggestions

Goal:

- improve class assignment quality with mapping + suggestions
- keep daemon optional

### Issue #28 — classification mapping model

- persistent mapping: desktop-id -> class
- mapping store versioning and safe migration

### Issue #29 — suggestions engine

- suggest class assignments based on observed wrapper usage
- non-destructive suggestions (no auto-apply by default)

### Issue #30 — optional `resguardd` hooks

- optional daemon hooks for richer events (journal/PSI context)
- no hard dependency: core workflow must work without daemon

### Issue #31 — policy UX

- CLI for reviewing/accepting/rejecting suggestions
- audit trail for accepted mappings

Exit criteria for v0.3:

- classification mappings are reliable and reversible
- suggestions are useful and safe by default
- daemon remains optional

---

## Milestone v0.4.0 — TUI Visualizer + Freeze Watchdog

Goal:

- provide operator-grade observability and optional automated freeze mitigation

### Issue #32 — TUI Visualizer

- terminal UI that reads from:
  - `/proc/pressure/*`
  - `systemctl show ...`
  - cgroup filesystem (`/sys/fs/cgroup`)
- show pressure, limits, and top slice usage live

### Issue #33 — Freeze Watchdog (optional `resguardd` feature)

- detect freeze-risk via PSI thresholds and stall indicators
- execute safe panic action with bounded scope
- explicit guardrails and cooldown windows

### Issue #34 — Watchdog safety controls

- dry-run simulation mode
- max action frequency/rate limits
- emergency disable switch

Exit criteria for v0.4:

- TUI is stable and useful under load
- watchdog is optional, guarded, and auditable

---

## Boundaries

- No daemon requirement for core functionality through v0.3.
- Watchdog automation enters only in v0.4 and remains optional.
- All system-changing actions must remain explicit, diffable, and rollback-aware.
