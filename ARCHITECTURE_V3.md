# ARCHITECTURE_V3.md

## Zweck

Dieses Dokument beschreibt die Zielarchitektur von **Resguard v3**. Ziel ist eine klare Trennung zwischen:

* **Model**
* **Policy**
* **Discovery**
* **Runtime**
* **Services**
* **Interfaces**

Resguard soll langfristig nicht als wachsendes CLI mit verstreuter Speziallogik weiterentwickelt werden, sondern als **kleine Linux Resource Policy Platform** mit klar abgegrenzten Verantwortlichkeiten.

---

## Leitidee

Resguard ist ein **Policy Layer über systemd/cgroups v2/PSI**.

Es ist **nicht**:

* kein Container-Runtime
* kein vollständiger Prozessmanager
* kein allgemeiner Scheduler
* kein Desktop-Environment-spezifisches UI-Projekt

Es ist:

* ein **Linux Desktop/Workstation Resource Guard**
* ein **Policy- und Orchestrierungs-Layer**
* ein Tool zur Sicherstellung von **Responsiveness unter Last**

---

## Architekturprinzipien

### 1. Strikte Schichtentrennung

Jede Schicht hat eine klar definierte Verantwortung.

### 2. Observe → Decide → Act

Resguard soll intern möglichst immer diesem Muster folgen:

* **Observe**: Systemzustand lesen
* **Decide**: Richtige Policy-Entscheidung treffen
* **Act**: Technische Aktion ausführen

### 3. Keine Business-Logik im CLI

CLI und Daemon sind Interfaces, nicht die eigentliche Fachlogik.

### 4. Linux-spezifische Systemzugriffe zentral bündeln

`systemctl`, `/proc`, PSI, cgroups und Slice-Manipulationen gehören in eine Runtime-/Execution-Schicht.

### 5. Discovery ist nicht Policy

Das Erkennen einer App und das Entscheiden ihrer Klasse sind getrennte Probleme.

---

## High-Level Zielarchitektur

```text
+------------------------------------------------------+
|                    Interfaces                         |
|  CLI | Daemon | TUI | future Desktop Integration      |
+------------------------------+-----------------------+
                               |
                               v
+------------------------------------------------------+
|                Application Services                   |
| Setup | Suggest | Desktop | Doctor | Panic | Rescue   |
+------------------------------+-----------------------+
               |                    |                   |
               v                    v                   v
+------------------------+  +------------------------+  +------------------------+
|      Policy Engine     |  |   Discovery Layer      |  |   Runtime / Execution  |
| classification         |  | xdg/snap/flatpak       |  | systemd/cgroups/PSI    |
| confidence             |  | desktop IDs            |  | planner/executor       |
| autoprofile            |  | scope parsing          |  | rollback               |
| autopilot decisions    |  | exec parsing           |  | system snapshots       |
+------------------------+  +------------------------+  +------------------------+
               \                   |                   /
                \                  |                  /
                 \                 |                 /
                  v                v                v
+------------------------------------------------------+
|                     Model Layer                       |
| Profile | ClassSpec | AppIdentity | Suggestion | ... |
+------------------------------------------------------+
```

---

## Ziel-Workspace

```text
resguard/
├─ crates/
│  ├─ resguard-model/
│  ├─ resguard-policy/
│  ├─ resguard-discovery/
│  ├─ resguard-runtime/
│  ├─ resguard-config/
│  ├─ resguard-services/
│  ├─ resguard-cli/
│  └─ resguard-daemon/
├─ docs/
├─ tests/
├─ packaging/
└─ scripts/
```

---

## Crate-Übersicht

### `resguard-model`

**Verantwortung:** Gemeinsame Domain-Typen und neutrale Datenstrukturen.

**Beispiele:**

* `Profile`
* `ClassSpec`
* `MemoryPolicy`
* `CpuPolicy`
* `OomdPolicy`
* `Suggestion`
* `SuggestionReason`
* `AppIdentity`
* `DesktopEntryRef`
* `PressureSnapshot`
* `MetricsSnapshot`
* `ActionPlan`
* `ApplyResult`
* `DoctorReport`

**Wichtig:**

* keine Systemzugriffe
* keine CLI-Logik
* keine Dateisystem-Annahmen
* möglichst pure Datenmodelle

---

### `resguard-policy`

**Verantwortung:** Fachliche Entscheidungen.

**Beispiele:**

* App-Klassifikation
* Confidence Scoring
* Auto-Profil-Generierung
* Pressure-/Autopilot-Entscheidungen
* Policy Defaults
* Klassenregeln

**Beispiel-APIs:**

```text
classify(identity, rules) -> ClassMatch
score(identity, signals) -> ConfidenceScore
build_auto_profile(system_snapshot) -> Profile
decide_pressure_actions(snapshot, state, profile) -> Vec<Action>
```

**Darf nicht:**

* `systemctl` aufrufen
* `/proc` lesen
* Desktop-Dateien scannen
* Snap-Pfade parsen
* Wrapper-Dateien schreiben

---

### `resguard-discovery`

**Verantwortung:** Alles, was Anwendungen, Desktop-Einträge und Prozess-Identitäten auffindet.

**Beispiele:**

* XDG Scan
* Snap Discovery
* Flatpak Discovery (später)
* Desktop-ID-Auflösung
* Alias-Auflösung
* Scope Parsing
* Exec Parsing
* Identity Extraction

**Beispiel-APIs:**

```text
scan_desktop_entries() -> Vec<DesktopEntryRef>
resolve_desktop_id(id) -> ResolutionResult
parse_scope_identity(scope_name, exec_start) -> AppIdentity
build_exec_index(entries) -> ExecIndex
```

**Wichtig:**

Discovery erkennt nur **was etwas ist**, nicht **was damit passieren soll**.

---

### `resguard-runtime`

**Verantwortung:** Linux-/systemd-/PSI-/cgroups-Ausführung.

**Beispiele:**

* `systemctl show`
* `systemctl cat`
* `systemd-run`
* PSI lesen
* `/proc/meminfo`
* Slice-Dateien rendern/schreiben
* Apply/Diff/Rollback
* Panic apply/revert
* System Snapshot erzeugen

**Beispiel-APIs:**

```text
read_pressure() -> PressureSnapshot
read_system_snapshot() -> SystemSnapshot
plan_apply(profile) -> ActionPlan
execute_plan(plan) -> ApplyResult
run_in_slice(slice, cmd, mode) -> ExitStatus
set_slice_limits(slice, limits) -> Result<()>
```

**Darf nicht:**

* App-Klassen entscheiden
* Confidence berechnen
* Desktop-Dateien interpretieren
* CLI-Output generieren

---

### `resguard-config`

**Verantwortung:** Laden, Speichern und Auflösen von Konfigurationen und State.

**Beispiele:**

* Profile-Store
* Desktop-Mapping-Store
* Daemon-Config
* State-Datei

**Beispiel-APIs:**

```text
load_profile()
save_profile()
load_desktop_mapping()
save_desktop_mapping()
load_state()
save_state()
```

---

### `resguard-services`

**Verantwortung:** Orchestrierung fachlicher Use-Cases.

Dies ist die Schicht zwischen Interfaces und Fach-/Systemlogik.

**Beispiele:**

* `SetupService`
* `SuggestService`
* `DesktopService`
* `DoctorService`
* `MetricsService`
* `PanicService`
* `RescueService`
* `DaemonService`
* `ApplyService`

**Beispiel-Flows:**

* `setup`
* `suggest`
* `desktop wrap`
* `panic`
* `rescue`
* `doctor`

**Wichtig:**

Services dürfen Policy/Runtime/Discovery/Config orchestrieren, aber keine Shell-/systemd-Details selbst implementieren.

---

### `resguard-cli`

**Verantwortung:** Kommandozeileninterface.

**Enthält:**

* clap Definitionen
* Argument Parsing
* Service-Aufrufe
* Output Rendering

**Darf nicht:**

* Discovery-Logik enthalten
* Confidence/Policy selbst berechnen
* `systemctl` direkt aufrufen
* Slice-Definitionen direkt rendern

---

### `resguard-daemon`

**Verantwortung:** Langlebiger Watchdog-/Autopilot-Prozess.

**Enthält:**

* Loop
* Signal Handling
* Logging
* Taktung
* Aufruf des `DaemonService`

**Wichtig:**

Der Daemon soll **keine eigene fachliche Sonderlogik** entwickeln, sondern dieselben Services/Policies wie das CLI nutzen.

---

## Verantwortungsdiagramm

```text
+-------------------+------------------------------------------------------+
| Layer             | Verantwortung                                        |
+-------------------+------------------------------------------------------+
| model             | Typen, DTOs, neutrale Daten                          |
| policy            | Entscheidungen, Regeln, Scoring                      |
| discovery         | Identifikation von Apps, Desktop-IDs, Scopes         |
| runtime           | Linux-/systemd-/PSI-Ausführung                       |
| config            | Laden/Speichern von State und Konfiguration          |
| services          | Orchestrierung von Use-Cases                         |
| cli               | Argumente, Kommandos, Ausgabe                        |
| daemon            | Event Loop, periodische Ausführung, Logging          |
+-------------------+------------------------------------------------------+
```

---

## Abhängigkeitsrichtung

Die Abhängigkeitsrichtung muss **einseitig** sein.

```text
resguard-model
   ^
   |
resguard-policy      resguard-discovery      resguard-runtime      resguard-config
         \                 |                    /                   /
          \                |                   /                   /
           \               |                  /                   /
                    resguard-services
                         ^
                         |
               +---------+----------+
               |                    |
         resguard-cli         resguard-daemon
```

### Verbotene Richtungen

* `policy -> runtime`
* `policy -> discovery`
* `runtime -> cli`
* `runtime -> services`
* `discovery -> runtime`
* `cli -> runtime` direkt, wenn Service existiert

---

## Zentrale Datenflüsse

### 1. `resguard setup`

```text
CLI
 -> SetupService
   -> Runtime: Systemzustand lesen
   -> Policy: Auto-Profil generieren
   -> Config: Profil speichern
   -> Runtime: Plan erzeugen und anwenden
 -> Output
```

### 2. `resguard suggest --apply`

```text
CLI
 -> SuggestService
   -> Runtime: aktive Scopes lesen
   -> Discovery: AppIdentity aus Scope/Exec/Desktop ableiten
   -> Policy: Klasse + Confidence bestimmen
   -> DesktopService / Config: optional Wrap / Mapping
 -> Output
```

### 3. `resguard panic`

```text
CLI
 -> PanicService
   -> Runtime: aktuelle Limits lesen
   -> Runtime: reduzierte Limits setzen
   -> Runtime: revert planen/ausführen
 -> Output
```

### 4. `resguardd`

```text
Daemon loop
 -> DaemonService
   -> Runtime: PSI/Systemzustand lesen
   -> Policy: Aktion bestimmen
   -> Runtime: Aktion ausführen
   -> Config/State: Ledger/State aktualisieren
 -> structured logging
```

---

## Observe → Decide → Act Diagramm

```text
+------------------+     +------------------+     +------------------+
| Observe          | --> | Decide           | --> | Act              |
| Runtime          |     | Policy           |     | Runtime          |
| /proc, PSI,      |     | scoring, limits, |     | systemd-run,     |
| systemctl show   |     | pressure policy  |     | set-property,    |
| scopes           |     | class mapping    |     | slice updates    |
+------------------+     +------------------+     +------------------+
```

Das ist das grundlegende Muster für fast alle wichtigen Use-Cases.

---

## Reale Problemfälle und ihre Schichtzuordnung

### Snap Firefox Discovery

**Gehört in:**

* `resguard-discovery::snap`
* `resguard-discovery::alias`
* `resguard-discovery::desktop`

**Nicht in:**

* `commands/desktop.rs`
* `main.rs`

---

### Confidence Scoring für `suggest`

**Gehört in:**

* `resguard-policy::confidence`
* `resguard-policy::classification`

**Nicht in:**

* CLI-Kommandologik

---

### Apply / Rollback / Planner

**Gehört in:**

* `resguard-runtime::planner`
* `resguard-runtime::executor`
* `resguard-runtime::rollback`

---

### Desktop Wrap / Unwrap Workflow

**Orchestrierung gehört in:**

* `resguard-services::desktop_service`

**Discovery gehört in:**

* `resguard-discovery`

**Datei-/Mapping-Store gehört in:**

* `resguard-config`

---

### Rescue / Panic

**Use-Case gehört in:**

* `resguard-services::rescue_service`
* `resguard-services::panic_service`

**Technische Ausführung gehört in:**

* `resguard-runtime`

---

## Geplante Module pro Crate

### `resguard-model`

```text
profile.rs
class.rs
policy.rs
identity.rs
metrics.rs
doctor.rs
plan.rs
```

### `resguard-policy`

```text
classification.rs
confidence.rs
autoprofile.rs
autopilot.rs
rules.rs
thresholds.rs
defaults.rs
```

### `resguard-discovery`

```text
xdg.rs
snap.rs
flatpak.rs        # future
scope.rs
exec.rs
desktop.rs
alias.rs
mapping.rs
```

### `resguard-runtime`

```text
systemd.rs
cgroup.rs
pressure.rs
meminfo.rs
planner.rs
executor.rs
rollback.rs
snapshot.rs
files.rs
```

### `resguard-config`

```text
profiles.rs
desktop_mapping.rs
daemon_config.rs
state_store.rs
store.rs
```

### `resguard-services`

```text
setup_service.rs
suggest_service.rs
desktop_service.rs
doctor_service.rs
metrics_service.rs
panic_service.rs
rescue_service.rs
apply_service.rs
daemon_service.rs
```

### `resguard-cli`

```text
main.rs
cli.rs
output.rs
commands/
  apply.rs
  daemon.rs
  desktop.rs
  doctor.rs
  metrics.rs
  panic.rs
  profile.rs
  rescue.rs
  rollback.rs
  run.rs
  setup.rs
  status.rs
  suggest.rs
  version.rs
```

### `resguard-daemon`

```text
main.rs
loop.rs
runner.rs
signal.rs
logging.rs
```

---

## Zielzustand für Tests

### Policy Tests

* Klassifikation
* Confidence Scoring
* Autoprofile-Heuristiken
* Pressure-Entscheidungen

### Discovery Tests

* XDG scan
* Snap alias resolution
* Exec Parsing
* Scope Parsing
* Desktop-ID-Auflösung

### Runtime Tests

* Plan-Erzeugung
* `systemctl`-Command-Generierung
* Apply/Diff/Rollback
* Panic-Revert

### Service Tests

* Setup Workflow
* Suggest Workflow
* Desktop Wrap/Unwrap Workflow
* Doctor/Status/Metrics Use-Cases

### CLI Tests

* Parsing
* Ausgabeformate
* `--version`
* Error Messaging

---

## Migrationsreihenfolge

Wichtig: **inkrementell**, kein Big-Bang-Refactor.

### Phase 1 — `resguard-model`

**Ziel:** Gemeinsame Typen zentralisieren.

**Move first:**

* Profile
* Suggestion Types
* Identity Types
* Metrics Reports
* Doctor Reports
* Action Plans

### Phase 2 — `resguard-discovery`

**Ziel:** Real-World-Desktop-/Snap-Komplexität isolieren.

**Move:**

* XDG scan
* Snap scan
* Alias resolution
* Scope Parsing
* Exec Parsing
* Desktop entry parsing

### Phase 3 — `resguard-policy`

**Ziel:** Suggest + AutoProfile + spätere Autopilot-Logik entkoppeln.

**Move:**

* Confidence Scoring
* Classification
* Auto profile defaults
* Threshold logic

### Phase 4 — `resguard-services`

**Ziel:** Use-Cases zentralisieren.

**Move first:**

* SuggestService
* DesktopService
* SetupService

### Phase 5 — `resguard-runtime`

**Ziel:** Linux-/systemd-Logik konsolidieren.

**Move/align:**

* planner
* executor
* rollback
* panic/revert
* slice rendering
* run integration

### Phase 6 — `resguard-daemon` an Services anschließen

**Ziel:** Daemon wird dünn und wiederverwendet dieselbe Logik.

---

## Empfohlener nächster Architekturmeilenstein

### Milestone A: Discovery + Policy Extraction

Dieser Meilenstein bringt den größten Nutzen bei geringem Risiko.

**Enthält:**

* neue Crate `resguard-discovery`
* neue Crate `resguard-policy`
* `suggest` und `desktop` nutzen diese Crates

**Warum zuerst?**

Weil dort aktuell die meiste reale Komplexität sitzt:

* Snap
* XDG
* Desktop IDs
* Alias Resolution
* Confidence Scoring
* echte Ubuntu-/KDE-Probleme

---

## Warum diese Architektur Resguard vereinfacht

### Heute drohender Schmerz

Wenn neue Features weiter direkt in CLI-/Command-Code wachsen, dann vermischt sich:

* Discovery
* Klassifikation
* Confidence
* Runtime
* UX

Das führt langfristig zu:

* unübersichtlichen Kommandos
* schwierigen Refactors
* doppelter Logik
* schwachen Tests
* riskanten Releases

### Mit V3-Architektur

Jede Frage hat einen klaren Ort:

* **Was ist das?** → Discovery
* **Was soll damit passieren?** → Policy
* **Wie wird das ausgeführt?** → Runtime
* **Wie orchestriert der Use-Case?** → Services
* **Wie sieht der Benutzer das?** → CLI / Daemon / TUI

---

## Langfristige Perspektive

Diese Architektur schafft die Grundlage für:

* **v0.3** Desktop-Härtung
* **v0.4** Autopilot / adaptive pressure handling
* **v0.5** Zero-config detection
* **v0.6** Observability / `resguard top`
* **v0.7+** Battery-/Thermal-aware Policies

Resguard entwickelt sich damit von einem „praktischen CLI-Werkzeug“ zu einer **kleinen Linux Resource Policy Platform**.

---

## Kurzfazit

Resguard V3 basiert auf der Trennung von:

```text
Model
Policy
Discovery
Runtime
Services
Interfaces
```

Diese Trennung ist der Schlüssel, damit Resguard:

* real wartbar bleibt
* Snap/XDG/DE-Komplexität beherrscht
* Autopilot sauber bauen kann
* CLI und Daemon dieselbe Logik nutzen
* langfristig als Standard-Linux-Tool wachsen kann

---

## Nächster Schritt

Die empfohlene nächste Refactor-Stufe ist:

### `resguard-discovery` + `resguard-policy` extrahieren

Danach:

### `resguard-services` als orchestrierende Schicht einziehen

Erst dann sollte der größere Autopilot-/Daemon-Ausbau folgen.
