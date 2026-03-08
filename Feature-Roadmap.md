Hier ist eine **realistische Feature-Roadmap für Resguard**, die alle Features aus unseren Gesprächen integriert und gleichzeitig **technisch sinnvoll aufeinander aufbaut**.
Wichtig: Die Reihenfolge orientiert sich an drei Faktoren:

1. **Stabilität des Core-Systems**
2. **UX / Adoption**
3. **Langfristige Vision (Autopilot / Adaptive Resource Control)**

Ich habe sie in **Versionen + Ziele + konkrete Features** gegliedert.

---

# Resguard Feature Roadmap

## Vision

Resguard wird die **Standard Resource Policy Engine für Linux Desktop & Developer Workstations**, die automatisch sicherstellt:

* Systeme bleiben **immer responsiv**
* Apps bekommen **faire Ressourcen**
* Nutzer müssen **nichts konfigurieren**

---

# v0.3 — Desktop Integration & Real-World Stability

**Ziel:** Resguard funktioniert zuverlässig auf echten Linux Desktops.

Status: ihr seid gerade **mitten in dieser Phase**.

## Core Improvements

* Snap Desktop Discovery (✔ bereits implementiert)
* Desktop ID alias resolution
  `firefox.desktop → firefox_firefox.desktop`
* `DBusActivatable=false` enforcement bei Wrappern
* Desktop Wrap Flow stabilisieren

## Suggest Engine

* Confidence Scoring verbessern
* Snap App Identity Detection
* Threshold realistisch setzen

Beispiel:

```
pattern: 40
identity: 30
desktop-id: 30
```

## CLI Improvements

* `--version`
* `resguard version`
* bessere `status` Darstellung

## Packaging

* APT Repository
* optional daemon package

## Field Testing

E2E Matrix:

| OS     | DE    | Install |
| ------ | ----- | ------- |
| Ubuntu | GNOME | apt     |
| Ubuntu | KDE   | apt     |
| Debian | GNOME | apt     |

Tools:

```
tests/e2e/run_e2e.sh
tests/e2e/e2e_matrix.md
tests/e2e/RUNBOOK.md
```

---

# v0.4 — Autopilot & Smart Resource Control

**Ziel:** Resguard wird zu einem **automatischen Desktop-Stabilizer**.

Das ist wahrscheinlich das **Durchbruch-Feature**.

## Autopilot Mode

CLI:

```
resguard autopilot
```

oder:

```
resguard setup --autopilot
```

### Autopilot Engine

Resguard beobachtet:

```
PSI metrics
memory pressure
cpu pressure
swap activity
```

Kernel Quellen:

```
/proc/pressure/memory
/proc/pressure/cpu
/proc/pressure/io
```

### Dynamische Aktionen

Beispiel:

| Pressure    | Aktion                   |
| ----------- | ------------------------ |
| Memory hoch | Browser limit            |
| CPU hoch    | IDE CPUWeight reduzieren |
| IO hoch     | heavy class drosseln     |

---

## Self-Tuning Policies

Resguard passt Limits automatisch an.

Beispiel:

```
browsers memory_max = dynamic
```

Policy:

```
min: 2GB
max: 6GB
adjust: PSI
```

---

## Daemon Mode

```
resguardd
```

Funktionen:

* kontinuierliches Monitoring
* Autopilot
* Policy Anpassung

---

# v0.5 — Zero-Config Desktop Intelligence

**Ziel:** Resguard funktioniert komplett ohne Konfiguration.

User installiert nur:

```
sudo apt install resguard
resguard setup
```

Fertig.

---

## Automatic App Classification

Resguard erkennt Apps automatisch.

Detection über:

* Desktop IDs
* Exec paths
* Snap scopes
* Flatpak metadata

Mapping:

| App              | Class    |
| ---------------- | -------- |
| Firefox / Chrome | browsers |
| VSCode           | ide      |
| Docker           | heavy    |
| LibreOffice      | normal   |

---

## Suggest Autoconfig

```
resguard suggest --auto
```

oder automatisch beim Setup.

---

## Desktop Integration

GNOME / KDE Launcher:

Apps starten automatisch mit Slice.

---

# v0.6 — Observability & UX

**Ziel:** Nutzer verstehen, was Resguard tut.

Das erhöht Adoption massiv.

---

## resguard top

Analog zu `htop`.

Beispiel:

```
APP        CLASS       RAM      LIMIT
Firefox    browsers    4.2G     6G
Code       ide         2.1G     4G
Docker     heavy       1.8G     3G
```

---

## resguard monitor

Live Übersicht:

```
CPU pressure
Memory pressure
IO pressure
```

---

## Slice Visualizer

```
resguard tree
```

Beispiel:

```
system.slice
user.slice
  resguard-browsers.slice
  resguard-ide.slice
```

---

# v0.7 — Laptop & Energy Awareness

Resguard reagiert auf Energiezustand.

---

## Battery-Aware Policies

Wenn Akku niedrig:

```
heavy class throttled
browser memory reduced
IDE cpu reduced
```

---

## Thermal Awareness

Integration mit:

```
/sys/class/thermal
```

---

# v0.8 — Distribution Integration

**Ziel:** Resguard wird Teil von Linux Distributionen.

---

## Distribution Presets

Profile für:

| Distribution | Preset           |
| ------------ | ---------------- |
| Ubuntu       | desktop-balanced |
| Fedora       | workstation      |
| Arch         | developer        |

---

## Default Installation

Langfristig möglich:

```
Ubuntu preinstall
```

oder

```
Fedora Workstation feature
```

---

# v1.0 — Linux Desktop Resource Standard

Endziel:

Resguard wird:

```
policy engine for desktop resources
```

über systemd slices.

---

## Standard Features

* Autopilot
* Zero config detection
* Desktop integration
* PSI adaptive control
* developer workstation optimization

---

# Technische Architektur (Langfristig)

```
kernel
 ├─ PSI metrics
 ├─ cgroups v2
 └─ OOM control

systemd
 └─ slices

resguard
 ├─ CLI
 ├─ daemon
 ├─ policy engine
 └─ autopilot

desktop
 ├─ launcher wrappers
 └─ app classification
```

---

# Kurzfassung der wichtigsten Features

| Feature                  | Version |
| ------------------------ | ------- |
| Desktop integration      | v0.3    |
| Snap support             | v0.3    |
| Autopilot                | v0.4    |
| Self tuning policies     | v0.4    |
| Zero config detection    | v0.5    |
| App classification       | v0.5    |
| resguard top             | v0.6    |
| Monitoring               | v0.6    |
| Battery awareness        | v0.7    |
| Distribution integration | v0.8    |

---

# Wichtigste Durchbruchfeatures

Wenn **diese drei funktionieren**, kann Resguard wirklich groß werden:

1️⃣ **Autopilot Mode**
→ verhindert Desktop Freezes

2️⃣ **Zero-Config Detection**
→ keine Konfiguration nötig

3️⃣ **resguard top**
→ sofort sichtbarer Nutzen

---

Wenn du möchtest, kann ich dir auch noch zeigen:

* **welche Architekturänderung Resguard langfristig massiv vereinfachen würde**
* und **welches einzelne Feature Resguard wirklich viral machen könnte (Developer Community).**
