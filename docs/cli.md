# Resguard CLI Spec (v0.1, implemented behavior)

## Allgemein
Binary: `resguard`

### Globale Flags
Die folgenden Flags sind global und funktionieren vor oder nach dem Subcommand:

- `--format <table|json|yaml>` (default: `table`)
- `--verbose`
- `--quiet`
- `--no-color`
- `--root <path>` (default: `/`)
- `--state-dir <path>` (default: `/var/lib/resguard`)
- `--config-dir <path>` (default: `/etc/resguard`)

### `--root` Verhalten (konkret)
`--root` isoliert Dateipfade für Config/State/Systemd-Dateien:

- `--config-dir /etc/resguard --root /tmp/rg` → tatsächlicher Pfad: `/tmp/rg/etc/resguard`
- `--state-dir /var/lib/resguard --root /tmp/rg` → tatsächlicher Pfad: `/tmp/rg/var/lib/resguard`

Wichtig:

- `systemctl daemon-reload` wird nur ausgeführt, wenn `--root /`.
- Bei Test-Roots (`/tmp/...`) werden keine systemd reload commands ausgeführt.

### Exit-Codes (aktuell)
- `0` OK
- `1` generischer Fehler
- `2` Validation/Argumentfehler
- `3` Permission denied (nur relevant wenn `--root /`)
- `4` Apply fehlgeschlagen, Rollback-Versuch wurde ausgeführt
- `5` Rollback fehlgeschlagen
- `6` `run`-Fehler (`systemctl`/`systemd-run`)

---

## Commands

## `resguard init`
Syntax:

- `resguard init [--name <n>] [--out <path>] [--apply] [--dry-run]`

Verhalten:

- Hardware-Detect: RAM aus `/proc/meminfo`, CPU über `available_parallelism()`
- ohne root:
  - default write: `./<name>.yml`
- mit root:
  - default write: `<config-dir>/profiles/<name>.yml` (inkl. `--root` Mapping)
- `--out` überschreibt Zielpfad
- `--dry-run` druckt YAML, schreibt nichts
- `--apply` ruft intern `apply` auf (kein Shell), benötigt root wenn `--root /`

Exit:

- `0` OK
- `2` invalid args (z. B. `--dry-run` + `--apply`)
- `3` `--apply` ohne erforderliche Rechte

---

## `resguard apply <profile>`
Flags:

- `--dry-run`
- `--no-oomd`
- `--no-cpu`
- `--no-classes`
- `--force` (aktuell geparst, ohne zusätzliche Logik)
- `--user-daemon-reload`

Verhalten:

- lädt Profil aus `<config-dir>/profiles/<profile>.yml` (mit `--root` Mapping)
- validiert Profil
- plant und schreibt:
  - `${root}/etc/systemd/system/system.slice.d/50-resguard.conf`
  - `${root}/etc/systemd/system/user.slice.d/50-resguard.conf`
  - `${root}/etc/systemd/system/resguard-<class>.slice`
  - `${root}/etc/systemd/user/resguard-<class>.slice`
- `--dry-run`: zeigt Plan, macht keine Writes
- `systemctl daemon-reload` nur wenn `--root /`
- `--user-daemon-reload` (best-effort) nur wenn `--root /` und `SUDO_USER` gesetzt:
  - `sudo -u $SUDO_USER systemctl --user daemon-reload`

Transactional state/backups:

- Backup-ID (timestamp in ms)
- Backups unter `${state_dir}/backups/<backup_id>/...`
- Manifest: `${state_dir}/backups/<backup_id>/manifest.json`
- State: `${state_dir}/state.json`

Bei Fehler:

- automatischer Rollback-Versuch innerhalb derselben Transaktion
- Exit `4` wenn Rollback-Versuch erfolgreich
- Exit `5` wenn Rollback selbst fehlschlägt

---

## `resguard rollback [--last | --to <backup-id>]`
Verhalten:

- `--last`: nutzt `backupId` aus `${state_dir}/state.json`
- `--to`: nutzt explizite Backup-ID
- stellt gesicherte Dateien aus Backup wieder her
- entfernt alle in `createdPaths` markierten Dateien
- `systemctl daemon-reload` nur wenn `--root /`
- setzt `state.json` anschließend auf default/leer

Exit:

- `0` OK
- `2` ungültige Argumente (weder `--last` noch `--to`)
- `3` Rechteproblem (nur bei `--root /`)
- `5` Rollback fehlgeschlagen

---

## `resguard run --class <class> -- <cmd...>`
Syntax:

- `resguard run --class <class> [--profile <name>] [--slice <slice>] [--wait] -- <cmd...>`

Slice-Auflösung:

- `--slice` hat Vorrang
- sonst Klasse aus Profil:
  - `--profile <name>` oder
  - aktives Profil aus `${state_dir}/state.json`
- wenn keine aktive Profile-Info vorhanden: Fehler "apply profile first"

Existenzprüfung (hard fail):

- user mode: `systemctl --user cat <slice>`
- system mode: `systemctl cat <slice>`
- bei Fehler: Hinweis "apply profile first"

Mode:

- `euid == 0` → system mode
- sonst user mode

Exec:

- user: `systemd-run --user --scope -p Slice=<slice> -- <cmd...>`
- system: `systemd-run --scope -p Slice=<slice> -- <cmd...>`
- `--wait` fügt `--wait` hinzu und gibt den echten Command-Exitcode zurück

Exit:

- ohne `--wait`: `0` bei Start-OK, sonst `6`
- mit `--wait`: Exitcode des gestarteten Commands

---

## `resguard status`
Best-effort Diagnose:

- liest `${state_dir}/state.json` (active profile, managed paths)
- zeigt `systemctl show` Props für `system.slice` und `user.slice`:
  - `MemoryLow`, `MemoryHigh`, `MemoryMax`, `AllowedCPUs`
- zeigt class slices aus State (system show)
- versucht `systemctl --user show resguard-browsers.slice` (best-effort)
- prüft `systemd-oomd` via `systemctl is-active systemd-oomd`
- liest PSI `avg60` aus `/proc/pressure/memory` und `/proc/pressure/cpu`

Exit:

- `0` wenn alles lesbar
- `1` wenn Teilinformationen fehlen (Warnungen werden ausgegeben)

---

## `resguard profile ...`
Aktuell implementiert:

- `resguard profile validate <name|path>`

Status:

- `list/show/import/export/new/edit` sind derzeit CLI-Stubs.

---

## `resguard diff`
Aktuell CLI-Stub.
