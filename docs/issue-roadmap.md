
# Resguard Roadmap

Projekt ist in drei Phasen geplant:

- v0.1: agentless + run (stabil) + init (auto profile)
- v0.2: desktop wrapper generator (80% “automatisch” ohne Daemon)
- v0.3: optional resguardd (Events/Panic/Auto rules)

---

# Milestone v0.1 — Agentless Core

Ziel:
- systemd ist das dauerhafte Enforcement-Layer
- resguard erzeugt/verwaltet nur Konfiguration + Rollbacks
- Workloads werden stabil über `run --class` gestartet

## Repo/Workspace

### Issue #1 — Workspace Struktur erstellen
- Cargo workspace
- crates: cli/core/system/config/state

### Issue #2 — Docs baseline
- docs/design.md
- docs/cli.md
- docs/safety.md
- docs/profiles.md

---

## Profiles (Schema + Store)

### Issue #3 — Profile Schema v1 (serde structs)
### Issue #4 — Profile Store (/etc/resguard/profiles)
- list/show/new/edit/import/export

### Issue #5 — Validation Engine
- memory sizes parse
- cpu set parse
- sanity: max >= high etc.

---

## Init (Auto Profile)

### Issue #6 — `resguard init` (detect + generate)
- RAM detect (/proc/meminfo)
- CPU detect
- default reserve rules
- write to:
  - root: /etc/resguard/profiles/<name>.yml
  - non-root: ./<name>.yml
- `--apply` support

---

## Planner/Diff/Apply

### Issue #7 — Plan Engine (desired → actions)
### Issue #8 — Diff Engine (files + state)

### Issue #9 — Apply engine (transactional best-effort)
- backups
- write drop-ins
- write slices

---

## systemd Artifacts (system + user manager)

### Issue #10 — Generate system drop-ins
- /etc/systemd/system/system.slice.d/50-resguard.conf
- /etc/systemd/system/user.slice.d/50-resguard.conf

### Issue #11 — Generate class slices (system)
- /etc/systemd/system/resguard-<class>.slice

### Issue #12 — Generate class slices (user)
- /etc/systemd/user/resguard-<class>.slice

### Issue #13 — Reload logic
- systemctl daemon-reload (system)
- user daemon-reload best-effort:
  - optional flag `--user-daemon-reload`
  - fallback instruction output

---

## State & Rollback

### Issue #14 — State file management
- /var/lib/resguard/state.json
- active profile + managed paths + checksums + backup id

### Issue #15 — Backup engine
### Issue #16 — Rollback implementation

---

## CLI (v0.1)

### Issue #17 — CLI skeleton (clap)
### Issue #18 — apply/diff/status/rollback commands
### Issue #19 — run --class
- system mode: systemd-run --scope
- user mode: systemd-run --user --scope
- `--wait`

---

## Testing

### Issue #20 — Unit tests: parsing/validation
### Issue #21 — Planner snapshot tests (golden diffs)
### Issue #22 — Integration tests with `--root` tempdir
- apply writes expected files
- rollback restores
- no writes outside root

---

# Milestone v0.2 — Desktop Integration (No Daemon)

Ziel:
- Nutzer startet Apps wie gewohnt über Launcher
- Resguard übernimmt Klassifikation über `.desktop` Wrapper

### Issue #23 — XDG discovery + desktop list
- find `.desktop` files in:
  - /usr/share/applications
  - /usr/local/share/applications
  - ~/.local/share/applications

### Issue #24 — `resguard desktop wrap`
- create wrapper desktop entry in ~/.local/share/applications/
- Exec becomes: `resguard run --class <class> -- <original exec> %u`
- handle TryExec/Icon/Name suffix, unique desktop-id

### Issue #25 — `resguard desktop unwrap`
- remove wrapper entry
- safety checks

### Issue #26 — `resguard desktop doctor`
- detect duplicates
- check resguard in PATH
- check user slices exist

### Issue #27 — Rules mapping (optional)
- persist mapping desktop-id -> class
- use it for wrap suggestions

---

# Milestone v0.3 — Optional resguardd

Ziel:
- Events/Panic/Auto Rules (wo sinnvoll)
- Resguard bleibt weiterhin agentless-usable

### Issue #28 — `resguardd` event watcher (system)
- PSI thresholds (cpu/memory pressure)
- journald OOM events
- write reports

### Issue #29 — Panic mode (runtime apply)
- temporary stricter profile / runtime overrides
- revert after cooldown

### Issue #30 — Auto rules (best-effort)
- suggest wrappers based on observed hogs
- optionally notify user

### Issue #31 — Systemd units
- resguardd.service
- optional resguardd-user.service

---

# Future Ideas
- TUI dashboard
- Prometheus exporter
- D-Bus backend (zbus) to reduce shell-outs
