

## Prompt C2 — `resguard run` als Durchbruch-Feature ausbauen

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Turn `resguard run` into the flagship developer feature of the project.

Vision:
A user should be able to run any command in a safe resource class with minimal friction.

Tasks:
1. Audit current run implementation.
2. Improve UX for:
   - explicit class mode: resguard run --class heavy <cmd...>
   - profile-backed defaults
   - better diagnostics when slices are missing
3. Add optional auto-detect mode if safe and practical:
   - e.g. resguard run firefox
   - only if classification confidence is strong enough
4. Keep shell safety and argument handling strict.
5. Add/update tests for:
   - explicit class run
   - profile-based resolution
   - missing slice guidance
   - optional auto-detect path if implemented

Acceptance:
- resguard run is an obviously useful developer-facing feature
- docs/examples are compelling and accurate
- cargo test passes

Suggested commit message:
feat(run): make resguard run the flagship resource-isolated command launcher
```

---

## Prompt C3 — `resguard top` / operator visibility

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Introduce a simple but powerful and colored `resguard top` command for class-oriented visibility.

Purpose:
Users should immediately understand what resources each class/app is using.

Tasks:
1. Design a text-first `resguard top` command.
2. Show at minimum:
   - class/slice name
   - current memory usage
   - configured limits
   - notable active scopes if available
3. Reuse runtime/services/model layers appropriately.
4. Keep the output compact and useful.
5. Add docs and tests where practical.

Acceptance:
- command is useful on a real developer workstation
- architecture boundaries remain respected
- cargo test passes

Suggested commit message:
feat(observability): add resguard top for class-oriented runtime visibility
```

---

# Meine empfohlene Reihenfolge

Wenn du **möglichst sinnvoll und ohne Chaos** weitermachen willst, dann so:

## Zuerst

* **A1**
* **A2**
* **A3**
* **A4**

## Danach

* **B1**
* **B2**
* **B3**
* **B4**
* **B5**

## Danach

* **C2**
* **C1**
* **C3**
