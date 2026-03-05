# Resguard Safety Model (v0.2)

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

---

## Panic Mode (`resguard panic`)

`panic` setzt temporär aggressivere Limits auf `user.slice`:

- `MemoryHigh` und `MemoryMax` werden abgesenkt
- bei gesetzter `--duration` versucht der Command danach die vorherigen Werte wiederherzustellen

Sicherheitsaspekte:

- nur mit Root im System-Root (`--root /`)
- bei fehlgeschlagenem Revert wird Fehler zurückgegeben (kein stilles Ignorieren)
- ohne `--duration` ist Revert manuell erforderlich (expliziter Hinweis wird ausgegeben)

---

## Daemon Watchdog Safety (`resguardd`)

`resguardd` ist optional und standardmäßig nicht automatisch aktiviert.

### Guardrails gegen Thrash

- hold/cooldown-gating vor erneuter Aktion
- konfigurierbares Poll-Intervall (mit Mindestwert)
- `--once` Modus für einzelnen Entscheidungszyklus ohne Dauerbetrieb

### Revert-Verhalten

Bei Aktion `set-property`:

1. vorherige `user.slice` Properties werden gespeichert (`MemoryHigh`, `MemoryMax`)
2. temporäre Limits werden gesetzt
3. nach Ablauf wird Revert ausgeführt
4. bei SIGINT/SIGTERM während aktiver Aktion: früher Revert-Versuch vor Exit

### Auditing

Daemon schreibt ein Action-Ledger unter:

- `${state_dir}/daemon-ledger.jsonl` (default `/var/lib/resguard/daemon-ledger.jsonl`)

Pro Record:

- timestamp
- PSI-Werte
- Aktion/Entscheidung
- `revert_ok` Status (wenn relevant)
