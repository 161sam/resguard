
# Resguard Design

## Ziel
`resguard` ist ein natives Linux System-Tool (Ubuntu/systemd/cgroups v2), das CPU/RAM-Ressourcen so partitioniert und priorisiert, dass das OS und definierte System-/Rescue-Prozesse selbst unter extremer Last zuverlässig bedienbar bleiben.

Kernversprechen:
- Terminal + Prozess-Tools (z. B. `htop`, `kill`, `journalctl`) bleiben startbar.
- User-Workloads (Browser/IDE/Docker/…​) können systemweite Freezes nicht mehr auslösen.
- Alle Änderungen sind **idempotent**, **diffbar**, **rollbackbar** und klar als „Managed by resguard“ markiert.

Betriebsmodell:
- **Agentless by default:** Resguard läuft nicht dauerhaft. Nach `apply` übernimmt **systemd** als dauerhafte Enforcement-Layer.
- Optional später: `resguardd` (v0.3) für Events/Panic/Auto Rules.

---

## Technische Grundlage
- systemd (Slices/Units/Drop-ins)
- cgroups v2 (über systemd verwaltet)
- systemd-oomd (PSI pressure; optional via slice properties)
- `/proc` (Pressure/Meminfo) für Status/Diagnose
- `systemctl` / `systemd-run` als MVP-Backend (kein D-Bus in v0.1)

---

## Architektur (Workspace)

```

resguard/
crates/
resguard-cli      # bin: CLI
resguard-core     # Domain: Profile/Planner/Diff/Validation
resguard-system   # Side effects: systemd files, exec, /proc, inspect
resguard-config   # Profile Store /etc/resguard
resguard-state    # Backups/State/Rollback (transactional best-effort)

````

### Verantwortlichkeiten
- **core**: rein funktional, ohne Root/IO testbar
- **system**: Linux-/systemd-spezifische Adapter
- **state**: Persistenz von Apply-Transaktionen (Backups + Restore)
- **cli**: UX, Output-Formate, Exit-Codes

---

## FHS / Dateipfade

### Konfiguration
- `/etc/resguard/config.yml` (optional, global)
- `/etc/resguard/profiles/*.yml` (Profile Store; v0.1)

### State & Backups
- `/var/lib/resguard/state.json` (aktiver Zustand, applied files, checksums)
- `/var/lib/resguard/backups/<ts>/...` (vollständige Backups aller betroffenen Dateien)

### systemd Generierung (nur resguard-managed)
Resguard schreibt systemd Artefakte in **zwei Instanzen**:

1) **System manager** (PID 1 / root):
- Drop-ins:
  - `/etc/systemd/system/user.slice.d/50-resguard.conf`
  - `/etc/systemd/system/system.slice.d/50-resguard.conf`
- Klassen-Slices:
  - `/etc/systemd/system/resguard-<class>.slice`

2) **User manager** (pro User session):
- Klassen-Slices:
  - `/etc/systemd/user/resguard-<class>.slice`

Warum beide?
- GUI/Session Apps laufen stabil über `systemd-run --user`
- root/system Workloads laufen über `sudo systemd-run`

Alle generierten Dateien enthalten:
- Header: `# Managed by resguard. DO NOT EDIT.`
- Optional: Referenz auf Profile-Name und Timestamp

---

## Profile Modell (v1)
Profile sind YAML-Dateien mit:
- **memory**: system-reserve & user caps (MemoryLow/High/Max)
- **cpu**: optional cpuset reservation (AllowedCPUs)
- **oomd**: optionale slice properties (ManagedOOM…)
- **classes**: benannte Klassen → slice units (system + user)
- **rules**: Best-effort Klassifikation (v0.1 nur persistiert, nicht erzwungen)
- **protect**: best-effort Schutz bestimmter Rescue-Kommandos (v0.1: Hinweise/Rescue; keine globalen Hooks)

---

## Planner → Actions
`resguard` arbeitet in 3 Schritten:

1. **Load + Validate**
   - YAML parse
   - Sanity checks:
     - memory strings parsebar (K/M/G/T)
     - `MemoryMax >= MemoryHigh` (wenn beide gesetzt)
     - CPU sets plausibel (z. B. "0", "1-7")
     - Klassen-Slice-Namen enden auf `.slice`
     - keine Pfadtraversal in Names

2. **Plan**
   - Desired state → Liste von Actions:
     - `WriteFile(path, content, mode)`
     - `EnsureDir(path)`
     - `SystemctlDaemonReload(system|user)`
     - `SystemctlRestart(service)`
   - Zusätzlich: `Diff` gegen Ist-Zustand (aktuelle Dateien + state.json)

3. **Apply**
   - Transactional best-effort:
     - Backup aller existierenden Target-Dateien
     - Writes in sicherer Reihenfolge
     - `systemctl daemon-reload` (system)
     - User daemon-reload: best-effort (siehe unten)
     - falls nötig: restart `systemd-oomd`
   - On error:
     - automatisches Restore aus Backup
     - erneutes daemon-reload
     - klarer Exit-Code + Hinweis auf Backup-ID

---

## `init` (v0.1): Auto-Profil Generierung
`resguard init` erkennt Hardware (RAM/CPU) und generiert ein sinnvolles Default-Profil:

- RAM Reserve (Faustregeln):
  - 8GB → 1–1.5GB
  - 16GB → 2GB
  - 32GB → 4GB
  - 64GB → 6GB
- `user.memoryMax = total - reserve`
- `user.memoryHigh = userMax - (min(2GB, 10% userMax))`
- CPU reserve:
  - wenn ≥ 4 cores: 1 core für system (opt-in default on)
  - sonst: CPU reserve default off
- Default classes (browsers/ide/heavy) mit konservativen caps

`init` schreibt:
- als root: direkt nach `/etc/resguard/profiles/<name>.yml`
- ohne root: nach `./<name>.yml` (Import/Apply dann später)

---

## Prozess-Klassen & `run --class` (v0.1)
Klassifikation wird im MVP **aktiv** über Wrapper-Start gelöst:

- `resguard run --class browsers -- firefox ...`

Implementiert via transient scope:

- User mode:
  - `systemd-run --user --scope -p Slice=resguard-browsers.slice -- <cmd...>`
- System mode (root):
  - `systemd-run --scope -p Slice=resguard-browsers.slice -- <cmd...>`

Vorteile:
- stabil (kein PID-Umziehen)
- systemd verwaltet cgroups korrekt
- funktioniert mit verschachtelten child-procs

---

## systemd Artefakte (v0.1 Output)

### Drop-in: user.slice (system manager)
`/etc/systemd/system/user.slice.d/50-resguard.conf`

```ini
# Managed by resguard. DO NOT EDIT.
[Slice]
MemoryHigh=12G
MemoryMax=14G
AllowedCPUs=1-7
ManagedOOMMemoryPressure=kill
ManagedOOMMemoryPressureLimit=60%
````

### Drop-in: system.slice (system manager)

`/etc/systemd/system/system.slice.d/50-resguard.conf`

```ini
# Managed by resguard. DO NOT EDIT.
[Slice]
MemoryLow=2G
AllowedCPUs=0
```

### Class slice unit (system manager)

`/etc/systemd/system/resguard-browsers.slice`

```ini
# Managed by resguard. DO NOT EDIT.
[Unit]
Description=Resguard browsers slice (system)

[Slice]
MemoryMax=6G
CPUWeight=80
ManagedOOMMemoryPressure=kill
ManagedOOMMemoryPressureLimit=55%
```

### Class slice unit (user manager)

`/etc/systemd/user/resguard-browsers.slice`

```ini
# Managed by resguard. DO NOT EDIT.
[Unit]
Description=Resguard browsers slice (user)

[Slice]
MemoryMax=6G
CPUWeight=80
ManagedOOMMemoryPressure=kill
ManagedOOMMemoryPressureLimit=55%
```

---

## User daemon-reload (v0.1 Verhalten)

Das Reloaden des user-managers ist in Multi-User/Display-Manager-Setups nicht immer trivial “aus root heraus”.

v0.1 Strategie:

* `systemctl daemon-reload` (system) immer
* user reload:

  * best-effort, wenn `SUDO_USER` vorhanden:

    * `sudo -u $SUDO_USER systemctl --user daemon-reload`
  * falls das fehlschlägt: klare Hinweiszeile im Output:

    * `Run: systemctl --user daemon-reload (in your session)`

---

## Status/Diagnose (v0.1)

`resguard status` sammelt:

* aktives Profil (state.json)
* system slice properties (`systemctl show user.slice system.slice ...`)
* class slices (system + user) aus state
* `systemd-oomd` aktiv? (`systemctl is-active systemd-oomd`)
* PSI pressure summary (optional, aus `/proc/pressure/memory`, `/proc/pressure/cpu`)
* quick hints: „user.slice MemoryMax fehlt“, „oomd disabled“ etc.

---

## Roadmap

### v0.1

* Profile Store CRUD + Validate
* `init` (auto profile)
* apply/diff/rollback/status
* `run --class`
* Idempotent writes + Backups

### v0.2

* Desktop wrapper generator (XDG .desktop wraps)
* Rules aktiv nutzbar über wrapper mapping
* Rescue/Inspect polish

### v0.3

* Optional `resguardd` (Events/Panic/Auto rules)
* optional: D-Bus backend (zbus), weniger Shell-Outs
