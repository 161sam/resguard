# Resguard Design (v0.1 implementation)

## Ziel

Resguard hält Linux-Systeme unter Last bedienbar, indem es systemd slices konfiguriert und Workloads in Klassen trennt.

## Architektur

Workspace:

- `resguard-cli`: CLI + Command Dispatch
- `resguard-core`: Profile, Validation, Planner, Renderer
- `resguard-system`: Linux/systemd Adapter (`systemctl`, `systemd-run`, `/proc`)
- `resguard-config`: Profile Store Load/Save
- `resguard-state`: State/Backups/Rollback-Manifest

## Pfadmodell

Defaults:

- `config_dir = /etc/resguard`
- `state_dir = /var/lib/resguard`
- `root = /`

`--root` wirkt als Prefix auf Dateioperationen:

- `${root}/etc/resguard/...`
- `${root}/var/lib/resguard/...`
- `${root}/etc/systemd/...`

Beispiel:

- `--root /tmp/rgtest --config-dir /etc/resguard`
- effektiver Profilpfad: `/tmp/rgtest/etc/resguard/profiles/*.yml`

## Profile Modell (aktuell)

`apiVersion: resguard.io/v1`, `kind: Profile`

- `spec.memory` (`memoryLow`, `memoryHigh`, `memoryMax`)
- `spec.cpu` (`systemAllowedCpus`, `userAllowedCpus`, ...)
- `spec.oomd` (`memoryPressure`, `memoryPressureLimit`)
- `spec.classes` und kompatibel `spec.slices.classes`

## Apply Pipeline

1. Profil laden (`config_dir/profiles/<name>.yml`)
2. Validieren
3. Plan bauen (`Action`):
   - `EnsureDir`
   - `WriteFile`
   - `Exec`
4. Bei `--dry-run`: nur Plan ausgeben
5. Bei echtem Apply:
   - vor jedem `WriteFile` Backup-Snapshot
   - Writes/Exec ausführen
   - State + Manifest schreiben

### Generierte Dateien

- `${root}/etc/systemd/system/system.slice.d/50-resguard.conf`
- `${root}/etc/systemd/system/user.slice.d/50-resguard.conf`
- `${root}/etc/systemd/system/resguard-<class>.slice`
- `${root}/etc/systemd/user/resguard-<class>.slice`

### systemctl Verhalten

- `systemctl daemon-reload` nur bei `root == "/"`
- `--user-daemon-reload` (best-effort) nur bei `root == "/"` und gesetztem `SUDO_USER`:
  - `sudo -u $SUDO_USER systemctl --user daemon-reload`

## State + Backups

`state_dir` enthält:

- `state.json`
- `backups/<backup_id>/manifest.json`
- `backups/<backup_id>/...` (gesicherte Originaldateien)

`state.json` Felder:

- `activeProfile`
- `backupId`
- `managedPaths`
- `createdPaths`

`backup_id` ist aktuell ein Millisekunden-Timestamp.

## Rollback

`rollback --last` oder `rollback --to <backup_id>`:

- stellt gebackupte Dateien wieder her
- entfernt Dateien aus `createdPaths`
- führt `systemctl daemon-reload` nur bei `root == "/"` aus
- setzt `state.json` auf default/leer

## Run (`run --class`)

Slice-Auflösung:

- `--slice` override
- sonst Klasse aus Profil:
  - via `--profile`, oder
  - via aktivem Profil aus `state.json`

Voraussetzungen:

- Slice muss existieren (`systemctl cat` bzw. `systemctl --user cat`), sonst Fehler "apply profile first"

Mode:

- root => system mode (`systemd-run --scope ...`)
- non-root => user mode (`systemd-run --user --scope ...`)

`--wait`:

- `systemd-run --wait`
- Exitcode des gestarteten Commands wird durchgereicht

## Status

`status` ist best-effort:

- liest `state.json`
- liest `systemctl show` Props (`MemoryLow/High/Max`, `AllowedCPUs`) für `system.slice`, `user.slice`, bekannte class slices
- versucht user-slice show (`resguard-browsers.slice`)
- prüft `systemd-oomd` aktiv
- liest PSI `avg60` aus `/proc/pressure/memory` und `/proc/pressure/cpu`

Wenn Teilinformationen fehlen: Warnungen + Exitcode `1`.
