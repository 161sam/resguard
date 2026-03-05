# Resguard Safety Model (v0.1)

Resguard verändert systemd-Konfiguration. Daher ist Safety zentral.

## Sicherheitsprinzipien

1. Dry-run vor Writes
2. Backups vor jedem File-Write
3. State + Manifest für nachvollziehbare Rollbacks
4. Best-effort automatische Rücknahme bei Apply-Fehlern

---

## Dry Run

```bash
resguard apply workstation --dry-run
```

Zeigt den Plan (`ensure_dir`, `write_file`, `exec`) und schreibt nichts.

---

## State + Backup Layout

Standardpfad (anpassbar via `--state-dir`):

- `state.json`: `/var/lib/resguard/state.json`
- Backups: `/var/lib/resguard/backups/<backup_id>/...`
- Manifest pro Apply: `/var/lib/resguard/backups/<backup_id>/manifest.json`

`<backup_id>` ist aktuell ein Millisekunden-Timestamp.

Mit `--root /tmp/rg` werden Pfade isoliert unter `/tmp/rg/...` geschrieben.

---

## Was wird gespeichert?

`state.json` enthält:

- `activeProfile`
- `backupId`
- `managedPaths`
- `createdPaths`

`manifest.json` enthält denselben Snapshot pro Backup-ID.

---

## Rollback-Verhalten (exakt)

Rollback stellt wieder her:

- alle Dateien, die vor Apply bereits existierten und gesichert wurden

Rollback entfernt:

- alle Dateien, die im Apply neu erstellt wurden (`createdPaths`)

Zusätzlich:

- `systemctl daemon-reload` nur bei `--root /`
- `state.json` wird nach erfolgreichem Rollback auf default/leer gesetzt

---

## Apply-Fehler

Wenn ein Apply-Schritt fehlschlägt:

- Resguard versucht automatisch Rollback für die aktuelle Transaktion
- Exit `4`: Apply fehlgeschlagen, Rollback-Versuch durchgeführt
- Exit `5`: Rollback selbst fehlgeschlagen

---

## Rechte / Root

Bei `--root /` gelten Root-Anforderungen für systemweite Änderungen.

Bei Test-Roots (`--root /tmp/...`) können Writes ohne Root geprüft werden, ohne echtes systemd zu verändern.
