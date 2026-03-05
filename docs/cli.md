
# Resguard CLI Spec (v0.1)

## Allgemein
Binary: `resguard`

### Globale Flags
- `--format <table|json|yaml>` (default: `table`)
- `--verbose` (mehr Logs)
- `--quiet` (nur Fehler)
- `--no-color`
- `--root <path>` (Test/Dev: prefix für File-IO; default `/`)
- `--state-dir <path>` (default: `/var/lib/resguard`)
- `--config-dir <path>` (default: `/etc/resguard`)

Hinweis: `--root` ist für Tests/CI wichtig, um Apply/Diff ohne echtes `/etc/systemd` zu testen.

### Exit-Codes (vereinheitlicht)
- `0` OK
- `1` generischer Fehler
- `2` Validation failed (Profile/Args)
- `3` Permission denied / not root (für commands die root brauchen)
- `4` Apply failed (rollback attempted)
- `5` Rollback failed
- `6` External command failed (`systemctl`/`systemd-run`), inkl. Exit Status

---

## Commands

## `resguard init`
Auto-detect Hardware und generiert ein Profil.

Syntax:
- `resguard init [--name <n>] [--out <path>] [--apply] [--dry-run]`

Verhalten:
- ohne root:
  - schreibt standardmäßig nach `./<name>.yml` (oder `--out`)
- mit root:
  - schreibt standardmäßig nach `<config-dir>/profiles/<name>.yml`
- `--apply`:
  - führt anschließend `resguard apply <name>` aus (root erforderlich)
- `--dry-run`:
  - zeigt die generierte Profile YAML (keine Writes)

Exit:
- `0` OK
- `2` invalid args
- `3` `--apply` ohne root

Beispiele:
```bash
resguard init --dry-run
resguard init --name auto-workstation
sudo resguard init --apply
````

---

## `resguard profile`

### `resguard profile list`

Listet Profile aus `<config-dir>/profiles`.

### `resguard profile show <name>`

Gibt das Profil YAML aus.

### `resguard profile new <name> [--from <base>]`

Erstellt neues Profil. Ohne `--from` wird ein minimaler Skeleton erzeugt.

### `resguard profile edit <name>`

Öffnet `$EDITOR` (fallback: `nano`), danach validate und save.

### `resguard profile validate <name|path>`

Validiert Profil (Schema + Sanity). Gibt Fehler strukturiert aus.

### `resguard profile import <file.yml>`

Kopiert Profil ins profile store (Name aus metadata).

### `resguard profile export <name> --out <file.yml>`

Schreibt Profil in Datei.

---

## `resguard diff <profile>`

Vergleicht geplante Zielkonfiguration gegen aktuellen Zustand.

* zeigt betroffene Dateien
* zeigt unified diffs (oder in `--format json`: structured diff)

Exit codes:

* `0` OK (auch wenn Änderungen vorhanden; im Output signalisiert)
* `2` invalid profile

Optional flag:

* `--show-content` (default on für table, off für json)

---

## `resguard apply <profile>`

Wendet Profil an.

Flags:

* `--dry-run` (keine Writes, keine systemctl calls; zeigt Plan)
* `--no-oomd` (skip ManagedOOM* props)
* `--no-cpu` (skip AllowedCPUs)
* `--no-classes` (skip generating class slices)
* `--force` (apply auch wenn aktive profile anders; default: erlaubt, aber warnt)
* `--user-daemon-reload` (best-effort: versucht `systemctl --user daemon-reload` für den Login-User)

Verhalten:

* root erforderlich (außer `--dry-run`).
* schreibt Drop-ins für:

  * system manager: `system.slice`, `user.slice`
* erzeugt Klassen-Slices in:

  * system manager: `/etc/systemd/system/resguard-*.slice`
  * user manager: `/etc/systemd/user/resguard-*.slice`
* `systemctl daemon-reload` immer
* `systemctl restart systemd-oomd` nur falls oomd settings geändert und oomd enabled im Profil
* user reload:

  * wenn `--user-daemon-reload`: best-effort
  * sonst: Hinweis im Output, wie man es selbst ausführt

Exit:

* `0` OK
* `2` Validation failed
* `3` not root
* `4` apply failed (rollback attempted)

---

## `resguard rollback [--last | --to <backup-id>]`

Rollback zur letzten oder spezifischen Backup-ID.

* root erforderlich
* stellt alle betroffenen Dateien wieder her
* `systemctl daemon-reload`

Exit:

* `0` OK
* `3` not root
* `5` rollback failed

---

## `resguard status`

Zeigt:

* aktives Profil (state.json)
* Slice-Properties:

  * system manager: `user.slice`, `system.slice`, `resguard-*.slice`
  * user manager: `resguard-*.slice` (best-effort)
* `systemd-oomd` aktiv?
* optional PSI: CPU/Mem pressure summary
* Hinweise (z. B. „MemoryMax nicht gesetzt“, „oomd disabled“)

Exit:

* `0` OK
* `1` Status teilweise nicht lesbar (best-effort output)
* `6` `systemctl show` hart fehlschlägt

---

## `resguard run --class <class> -- <cmd...>`

Startet ein Kommando zuverlässig in einer Klasse (Slice), via transient scope.

Syntax:

* `resguard run --class <class> [--profile <name>] [--slice <slice>] [--wait] -- <cmd...>`

Regeln:

* `<cmd...>` ist Pflicht nach `--`.
* Klasse kommt aus aktivem Profil (state.json) oder via `--profile`.
* Wenn `--slice` gesetzt ist, überschreibt es die Slice-Auflösung.

Mode:

* wenn `geteuid()==0` → system mode:

  * `systemd-run --scope -p Slice=<slice> -- <cmd...>`
* sonst → user mode:

  * `systemd-run --user --scope -p Slice=<slice> -- <cmd...>`

Slice existence:

* user mode: `systemctl --user cat <slice>` (best-effort)
* system mode: `systemctl cat <slice>`
  Wenn fehlt → Error: „apply profile first“.

Exit:

* `0` Start OK (oder Command OK bei `--wait`)
* `2` invalid class/profile/args
* `6` systemd-run failed

Beispiele:

```bash
resguard run --class browsers -- firefox
resguard run --class heavy -- docker compose up
sudo resguard run --class heavy -- /usr/local/bin/some-root-workload
```

---

## Output-Formate

### `--format table` (default)

* menschenlesbar
* diffs als unified diff blocks

### `--format json`

* für scripting
* enthält:

  * command, timestamp
  * plan/actions (paths, operations)
  * state (active profile, backup id)
  * errors structured

### `--format yaml`

* wie json, nur YAML

---

## Minimalanforderungen (v0.1)

* systemd vorhanden
* cgroups v2
* `systemctl` und `systemd-run` im PATH


