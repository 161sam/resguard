
# Resguard

**Resguard** ist ein natives Linux-Systemtool zur **Ressourcen-Isolation und System-Stabilisierung**.

Es schützt dein System davor, durch RAM/CPU-intensive Anwendungen (Browser, IDEs, Container etc.) unbedienbar zu werden.

Resguard reserviert Ressourcen für das Betriebssystem und organisiert Anwendungen in **systemd slices**.

Das Ziel:

> Selbst unter extremer Last bleibt dein System **reaktionsfähig**.

---

# Features

- RAM-Reservierung für das Betriebssystem
- CPU-Core Isolation
- systemd slice Management
- Workload-Klassen (browser / ide / heavy)
- Profile-basierte Konfiguration
- Dry-run Modus
- Rollback Support
- System Status Diagnose
- Workloads direkt in slices starten

---

# Warum Resguard?

Typisches Szenario:

- Browser öffnet 50 Tabs
- IDE indexiert ein großes Projekt
- Docker startet mehrere Container

RAM und CPU sind ausgelastet.

➡️ Der Desktop friert ein.  
➡️ Terminal lässt sich nicht mehr öffnen.

Mit Resguard:

- OS hat reservierte Ressourcen
- user workloads sind begrenzt
- System bleibt bedienbar

---

# Installation

## Voraussetzungen

- Linux (systemd + cgroups v2)
- Rust 1.75+

Ubuntu:

```bash
sudo apt install build-essential
````

---

## Build

```bash
git clone https://github.com/yourorg/resguard
cd resguard
cargo build --release
```

Binary:

```
target/release/resguard
```

Installieren:

```bash
sudo cp target/release/resguard /usr/local/bin/
```

---

# Quick Start

### Profil erstellen

```bash
resguard profile new workstation
```

### Profil anwenden

```bash
sudo resguard apply workstation
```

### Status prüfen

```bash
resguard status
```

---

# Workloads starten

Beispiel:

```bash
resguard run --class browsers -- firefox
```

Intern:

```
systemd-run --scope -p Slice=resguard-browsers.slice firefox
```

---

# Dry Run

Zeigt Änderungen ohne sie anzuwenden.

```bash
resguard apply workstation --dry-run
```

---

# Rollback

```bash
sudo resguard rollback
```

---

# Beispielprofile

Im Ordner:

```
docs/examples/
```

Beispiele:

* workstation-16g.yml
* workstation-32g.yml
* dev-docker-heavy.yml

---

# CLI Übersicht

```
resguard profile list
resguard profile show <name>

resguard apply <profile>
resguard diff <profile>
resguard rollback

resguard status

resguard run --class <class> -- <command>
```

---

# Demo

Example workflow:

```bash
resguard profile new workstation
resguard apply workstation
resguard run --class browsers -- firefox
```

Terminal Output:

```
✔ Profile applied
✔ systemd daemon reloaded
✔ slices active
```

---

# Architektur

Resguard basiert auf:

* systemd slices
* cgroups v2
* systemd-oomd

Resguard schreibt nur:

```
/etc/systemd/system/*.slice
/etc/systemd/system/*.slice.d/
```

Alle Änderungen sind:

* versioniert
* rollbackbar
* klar markiert

---

# Sicherheit

Resguard verändert systemd Ressourcenlimits.

Alle Änderungen:

* können per Dry-Run geprüft werden
* werden vor dem Schreiben gesichert
* können per Rollback rückgängig gemacht werden

Weitere Details:

```
docs/safety.md
SECURITY.md
```

---

# Projektstruktur

```
crates/
  resguard-cli
  resguard-core
  resguard-system
  resguard-config
  resguard-state
```

---

# Roadmap

### v0.1

* Profile
* Apply
* Rollback
* run --class

### v0.2

* Rules Engine
* Rescue Mode
* Inspect Tools

### v0.3

* Desktop Integration
* Event Watcher
* Monitoring

---

# License

MIT License
