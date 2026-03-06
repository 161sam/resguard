
# Resguard Profiles

Profiles definieren, wie Systemressourcen zwischen OS und Anwendungen aufgeteilt werden.

Ein Profil beschreibt:

- RAM-Reservierung für das System
- RAM-Limits für User-Workloads
- CPU-Reservierungen
- optionale Workload-Klassen (browsers, ide, heavy, rescue)
- OOM-Verhalten
- Prozessregeln (optional)

Profiles werden standardmäßig gespeichert in:

```

/etc/resguard/profiles/

````

---

# Profile Struktur

Minimaler Aufbau:

```yaml
apiVersion: resguard.io/v1
kind: Profile

metadata:
  name: workstation

spec:
  memory:
    system:
      memoryLow: "2G"
    user:
      memoryHigh: "12G"
      memoryMax: "14G"
````

---

# Memory Policy

## system.memoryLow

RAM-Reserve für OS-Prozesse.

Dieser Wert wird in:

```
system.slice
```

gesetzt.

Beispiel:

```yaml
system:
  memoryLow: "2G"
```

Effekt:

```
system.slice
MemoryLow=2G
```

Der Kernel versucht diesen Speicher nicht zu reclaimen.

---

## user.memoryHigh

Soft-Limit für User-Workloads.

Wenn überschritten:

* aggressiver Memory Reclaim
* systemd-oomd kann eingreifen

---

## user.memoryMax

Hard-Limit für User-Workloads.

Wenn überschritten:

* OOM-Kill innerhalb der Slice

---

# CPU Policy

CPU-Reservierung ist optional.

```yaml
cpu:
  enabled: true
  reserveCoreForSystem: true
  systemAllowedCpus: "0"
  userAllowedCpus: "1-7"
```

Beispiel (8-Core CPU):

| Slice        | CPUs |
| ------------ | ---- |
| system.slice | 0    |
| user.slice   | 1-7  |

Damit bleibt immer mindestens ein CPU-Core frei für das System.

---

# Workload Classes

Workloads können in separate slices isoliert werden.

Beispiel:

```yaml
slices:
  classes:
    browsers:
      sliceName: "resguard-browsers.slice"
      memoryMax: "6G"
      cpuWeight: 80

    ide:
      sliceName: "resguard-ide.slice"
      memoryMax: "4G"

    heavy:
      sliceName: "resguard-heavy.slice"
      memoryMax: "5G"

    rescue:
      sliceName: "resguard-rescue.slice"
      cpuWeight: 100
      memoryMax: "1G"
```

Diese werden als systemd units erzeugt.

---

# Prozesse in Klassen starten

```bash
resguard run --class browsers -- firefox
```

Intern:

```
systemd-run --scope -p Slice=resguard-browsers.slice firefox
```

---

# Rules (optional)

Rules erlauben automatische Klassifikation.

Beispiel:

```yaml
rules:
  - id: r1
    class: browsers
    match:
      desktopId: "firefox.desktop"

  - id: r2
    class: ide
    match:
      regex: "(code|codium|pycharm)"

  - id: r3
    class: heavy
    match:
      unit: "docker.service"
```

Hinweis:

In v0.1 sind Rules **nur deklarativ**.

Automatische Klassifikation wird in späteren Versionen implementiert.

---

# Beispielprofile

---

# workstation-16g.yml

Für typische Desktop-Workstations.

```yaml
apiVersion: resguard.io/v1
kind: Profile

metadata:
  name: workstation-16g

spec:
  memory:
    system:
      memoryLow: "2G"

    user:
      memoryHigh: "12G"
      memoryMax: "14G"

  cpu:
    enabled: true
    reserveCoreForSystem: true
    systemAllowedCpus: "0"
    userAllowedCpus: "1-7"

  slices:
    classes:

      browsers:
        sliceName: "resguard-browsers.slice"
        memoryMax: "6G"

      ide:
        sliceName: "resguard-ide.slice"
        memoryMax: "4G"

      heavy:
        sliceName: "resguard-heavy.slice"
        memoryMax: "5G"

      rescue:
        sliceName: "resguard-rescue.slice"
        memoryMax: "1G"
```

---

# workstation-32g.yml

```yaml
spec:
  memory:
    system:
      memoryLow: "4G"

    user:
      memoryHigh: "26G"
      memoryMax: "30G"
```

---

# dev-docker-heavy.yml

Für Entwickler mit vielen Containern.

```yaml
spec:
  slices:

    classes:
      docker:
        sliceName: "resguard-docker.slice"
        memoryMax: "12G"

      browsers:
        sliceName: "resguard-browsers.slice"
        memoryMax: "6G"
```

---

# Best Practices

### RAM Reserve

Empfehlungen:

| RAM  | Reserve |
| ---- | ------- |
| 8GB  | 1-1.5GB |
| 16GB | 2GB     |
| 32GB | 3-4GB   |
| 64GB | 6GB     |

---

### Browser Isolation

Browser sind oft größte RAM-Verbraucher.

Empfehlung:

```
browser.slice MemoryMax
```

---

### Docker / VM

Container können sehr aggressiv sein.

Empfehlung:

```
docker.slice MemoryMax
```

---

### CPU Isolation

Nur aktivieren wenn:

* viele Kerne vorhanden
* CPU starvation Problem besteht
