
# Resguard Safety Model

Resguard verändert systemd slices.

Fehlkonfiguration könnte theoretisch:

- Performance reduzieren
- Prozesse unerwartet limitieren

Deshalb ist Safety ein zentraler Bestandteil.

---

# Sicherheitsprinzipien

1. **Dry-Run First**
2. **Atomic Writes**
3. **Backups vor jeder Änderung**
4. **Rollback jederzeit möglich**
5. **Alle Dateien klar als Managed markiert**

---

# Dry Run

```bash
resguard apply workstation --dry-run
````

Zeigt:

* betroffene Dateien
* Änderungen
* diff output

Ohne Änderungen am System.

---

# Backups

Vor jeder Änderung werden Dateien gesichert.

Pfad:

```
/var/lib/resguard/backups/<timestamp>/
```

Beispiel:

```
/var/lib/resguard/backups/2026-03-05T14-00/
```

---

# Rollback

Rollback stellt vorherigen Zustand wieder her.

```bash
resguard rollback
```

Oder:

```bash
resguard rollback --to 2026-03-05T14-00
```

Rollback stellt wieder her:

* systemd drop-ins
* slice units
* state file

Danach:

```
systemctl daemon-reload
```

---

# Recovery ohne resguard

Falls resguard selbst beschädigt ist.

Manuell:

```
rm /etc/systemd/system/user.slice.d/50-resguard.conf
rm /etc/systemd/system/system.slice.d/50-resguard.conf
rm /etc/systemd/system/resguard-*.slice

systemctl daemon-reload
```

---

# Safe Defaults

Profiles werden validiert.

Checks:

* MemoryMax >= MemoryHigh
* CPU Sets gültig
* Slice Namen korrekt
* keine ungültigen Units

---

# Failure Modes

### Systemctl fehlgeschlagen

resguard bricht ab und versucht rollback.

---

### Teilweise Apply

resguard erkennt unvollständige Writes via state file.

Rollback möglich.

---

### OOMD Konflikte

resguard setzt nur:

```
ManagedOOMMemoryPressure
```

Es überschreibt keine fremden OOM-Konfigurationen.

---

# Logging

Verbose Mode:

```
resguard --verbose apply workstation
```

zeigt:

* systemctl commands
* file writes
* diffs

Keine sensiblen Daten werden geloggt.
