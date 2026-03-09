# Resguard CLI Spec (v0.1, implemented behavior)

## Allgemein
Binary: `resguard`

### Globale Flags
Die folgenden Flags sind global und funktionieren vor oder nach dem Subcommand:

- `--format <table|json|yaml>` (default: `table`)
- `--version`
- `--json-log` (optionale strukturierte Logs auf stderr)
- `--verbose`
- `--quiet`
- `--no-color`
- `--root <path>` (default: `/`)
- `--state-dir <path>` (default: `/var/lib/resguard`)
- `--config-dir <path>` (default: `/etc/resguard`)

Logging:

- alternativ zu `--json-log` kann `RESGUARD_LOG=json` gesetzt werden
- betrifft nur Logs (stderr), nicht den strukturierten Command-Output (stdout)
- Completion-Output bleibt unverändert (keine Log-Zeilen)

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

## `resguard version`
Syntax:

- `resguard version`

Verhalten:

- gibt dieselbe Versionsausgabe aus wie `resguard --version`
- Version stammt aus der Cargo-Paketversion des CLI-Binaries

Exit:

- `0` OK

---

## `resguard init`
Syntax:

- `resguard init [--name <n>] [--out <path>] [--apply] [--dry-run]`

Verhalten:

- Hardware-Detect: RAM aus `/proc/meminfo`, CPU über `available_parallelism()`
- erzeugt im Auto-Profil standardmäßig Klassen:
  - `browsers` (`resguard-browsers.slice`)
  - `ide` (`resguard-ide.slice`)
  - `heavy` (`resguard-heavy.slice`)
  - `rescue` (`resguard-rescue.slice`)
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

## `resguard run [--class <class>] <cmd...>`
Syntax:

- explizit: `resguard run --class <class> [--profile <name>] [--slice <slice>] [--wait] <cmd...>`
- optional auto-detect: `resguard run [--profile <name>] [--slice <slice>] [--wait] <cmd...>`

Slice-Auflösung:

- `--slice` hat Vorrang
- sonst Klasse:
  - explizit über `--class`, oder
  - sichere Auto-Erkennung aus Command (nur bei starker Confidence)
- danach Slice bevorzugt aus Profil:
  - `--profile <name>` oder
  - aktives Profil aus `${state_dir}/state.json`
- wenn keine Profil-Info verfügbar oder Klasse im Profil fehlt:
  - Fallback auf Standard-Slice-Name `resguard-<class>.slice`

Existenzprüfung (hard fail):

- user mode: `systemctl --user cat <slice>`
- system mode: `systemctl cat <slice>`
- bei Fehler: konkrete Hinweise auf `setup/apply` mit copy-paste Befehlen

Mode:

- `euid == 0` → system mode
- sonst user mode

Exec:

- user: `systemd-run --user --scope -p Slice=<slice> -- <cmd...>`
- system: `systemd-run --scope -p Slice=<slice> -- <cmd...>`
- `--wait` fügt `--wait` hinzu und gibt den echten Command-Exitcode zurück

Run-Ausgabe enthält immer:

- `selected.class=<class>`
- `selected.slice=<slice>`
- `resolution.source=<...>` (z. B. explizit/profil/auto-detect)

Exit:

- ohne `--wait`: `0` bei Start-OK, sonst `6`
- mit `--wait`: Exitcode des gestarteten Commands

---

## `resguard rescue`
Syntax:

- `resguard rescue [--class rescue] [--command <cmd>] [--no-ui] [--no-check]`

Default:

- startet über `run` in der Klasse `rescue`:
  - `resguard run --class rescue -- $SHELL -lc "htop || top"`
- Shell-Auflösung:
  - `$SHELL`, sonst `/bin/bash`, sonst `/bin/sh`

Flags:

- `--class <name>`: andere Klasse statt `rescue`
- `--command <cmd>`: führt benutzerdefinierten Shell-Command via `-lc` aus
- `--no-ui`: startet nur eine interaktive Shell (kein `htop/top`)
- `--no-check`: wenn Klasse/Slice nicht auflösbar ist, Fallback auf `system.slice`

Fehler-/Fix-Hinweise bei fehlendem Slice:

- zeigt konkrete Schritte:
  - Profil mit Klasse prüfen/anlegen
  - `sudo resguard apply <profile> --user-daemon-reload`
  - `resguard rescue` erneut ausführen
- optionaler Poweruser-Fallback: `resguard rescue --no-check`

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

## `resguard top`
Kompakte Klassen-/Slice-Sicht für aktive Workstation-Last.

Syntax:

- `resguard top [--scopes <n>] [--plain]`

Verhalten:

- liest aktives Profil aus `${state_dir}/state.json`
- zeigt pro Klasse:
  - Slice-Name und Quelle (`user`/`system`)
  - `MemoryCurrent`, `MemoryHigh`, `MemoryMax`, `CPUWeight`
  - konfigurierte Limits aus dem Profil
  - notable aktive Scopes innerhalb der Klasse (default: 3)
- bei fehlendem Profil oder nicht sichtbaren Slices:
  - kompakte Warnung mit nächsten Schritten (`setup/apply`)

Output-Modi:

- `--format table` (default): menschenlesbar, optional farbig (TTY, außer `--no-color`/`NO_COLOR`)
- `--plain`: script-sicheres Tabellenformat ohne ANSI
- `--format json|yaml`: strukturierter Snapshot inkl. `partial`/`warnings`

Exit:

- `0` wenn alle Daten vollständig
- `1` bei Teilinformationen (`partial=true`)

---

## `resguard tui` (Feature `tui`)
Syntax:

- `resguard tui [--interval <ms>] [--no-top]`

Verhalten:

- interaktive Terminal-Ansicht (mit `--features tui`)
- zeigt PSI-Überblick (CPU/MEM/IO) und System-Memory
- zeigt Klassen-Slices aus aktivem Profil mit Live-Werten (wenn verfügbar):
  - `MemoryCurrent`, `MemoryHigh`, `MemoryMax`, `CPUWeight`
- zeigt letzte Daemon/Autopilot-Einträge aus `${state_dir}/daemon-ledger.jsonl` (wenn vorhanden)
- beendet mit `q` oder `Esc`

Non-TTY Fallback:

- wenn `stdout` kein TTY ist, wird automatisch eine einmalige textuelle Summary ausgegeben

---

## `resguard profile ...`
Aktuell implementiert:

- `resguard profile validate <name|path>`

Status:

- `list/show/import/export/new/edit` sind derzeit CLI-Stubs.

---

## `resguard diff`
Aktuell CLI-Stub.

---

## `resguard desktop list`
Verhalten:

- scannt XDG-Desktop-Verzeichnisse:
  - `$XDG_DATA_HOME/applications` (Fallback: `$HOME/.local/share/applications`)
  - jedes `<dir>/applications` aus `$XDG_DATA_DIRS`
- zusätzliche Fallback-Systempfade:
  - `/usr/local/share/applications`
  - `/usr/share/applications`
  - `/var/lib/snapd/desktop/applications` (Ubuntu Snap)

Hinweis:

- Snap-Apps wie Firefox/VS Code erscheinen typischerweise als IDs wie `firefox_firefox.desktop` oder `code_code.desktop`.

---

## `resguard desktop wrap <desktop_id> --class <class>`
Verhalten:

- löst Desktop-ID zuerst exakt auf
- wenn nicht gefunden: versucht sichere Alias-Auflösung für häufige Snap-Namen (z. B. Anfrage `firefox.desktop` -> `firefox_firefox.desktop`) nur bei eindeutiger Zuordnung
- bei mehreren Treffern: Fehler mit Kandidatenliste (kein unsicheres Rateverhalten)
- wenn die Quelle `DBusActivatable=true` hat, setzt der Wrapper explizit `DBusActivatable=false`, damit Launcher `Exec=` des Wrappers verwenden

---

## `resguard desktop doctor`
Verhalten:

- prüft Wrapper-Dateien aus dem Desktop-Mapping auf Existenz und parsebare `Exec=`-Wrapper
- prüft, ob `resguard-<class>.slice` im User-Daemon sichtbar ist
- gibt bei potenziell veralteten Launcher-Caches konkrete nächste Schritte aus:
  - `systemctl --user daemon-reload`
  - optional `update-desktop-database "$HOME/.local/share/applications"`
  - ggf. Logout/Login (oder Reboot) zur Launcher-Aktualisierung
