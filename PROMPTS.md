# Block B — v0.4: Autopilot / Daemon / TUI



---

## Prompt B2 — Runtime Support für adaptive class limit changes

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Add runtime support for safe adaptive slice limit changes needed by autopilot.

Tasks:
1. Extend resguard-runtime to support:
   - temporary class-specific limit changes
   - safe revert support
   - action result reporting
2. Keep scope of actions small and auditable.
3. Prefer explicit typed operations over generic shell commands.
4. Add/update tests for:
   - class limit change planning
   - apply/revert flow
   - no-op handling when values are unchanged

Acceptance:
- runtime can execute autopilot actions safely
- revert path is explicit
- cargo test passes

Suggested commit message:
feat(runtime): add safe adaptive slice limit updates for autopilot actions
```

---

## Prompt B3 — `resguardd` auf echte Autopilot-Services umstellen

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Wire the daemon to the new policy/runtime autopilot flow through services.

Tasks:
1. Update daemon/service integration so the daemon loop does:
   - observe via runtime
   - decide via policy
   - act via runtime
2. Ensure:
   - structured logs remain useful
   - ledger entries remain meaningful
   - signal handling remains safe
3. Keep the daemon conservative by default.
4. Add/update tests for:
   - once mode
   - action trigger path
   - no-action path
   - cooldown behavior

Acceptance:
- daemon uses the proper architecture rather than ad-hoc logic
- cargo test passes
- daemon remains optional and safe by default

Suggested commit message:
refactor(daemon): wire resguardd through services for autopilot execution
```

---

## Prompt B4 — TUI von “nice demo” zu nützlichem Operator-Tool machen

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Turn the TUI into a genuinely useful operator view for v0.4.

Tasks:
1. Review the current feature-gated tui command.
2. Improve it to show:
   - pressure summary
   - class slice limits
   - class slice current usage where available
   - recent autopilot/daemon actions if feasible
3. Keep it lightweight and terminal-friendly.
4. Do not add visual complexity for its own sake.
5. Update docs/README usage examples if behavior becomes more useful.

Acceptance:
- TUI gives immediate visibility into what Resguard is doing
- still works in non-TTY fallback mode
- cargo build/test passes

Suggested commit message:
feat(tui): improve operator visibility into pressure class limits and autopilot state
```

---

## Prompt B5 — v0.4 Release Prep

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Prepare the v0.4.0 release with autopilot, daemon, and TUI improvements.

Tasks:
1. Update:
   - CHANGELOG.md
   - docs/releases.md
   - docs/install.md
   - daemon-related docs
2. Bump versions to 0.4.0 where appropriate.
3. Validate:
   - cargo build
   - cargo build --features tui
   - cargo test --workspace --all-targets
   - ./scripts/release.sh --version 0.4.0 --dry-run
4. Ensure packaging/docs correctly describe:
   - core package
   - optional daemon package
   - TUI feature-gated status if still applicable

Acceptance:
- repo is ready to cut v0.4.0
- release/install docs match reality
- workflows remain healthy

Suggested commit message:
chore(release): prepare v0.4.0 with autopilot daemon and tui
```

---

# Block C — v0.5+: Zero-Config / Developer-Viral Features

## Prompt C1 — Zero-Config Detection als Standard-Flow

```text
You are working in the Resguard repository. Follow AGENTS.md and docs/ARCHITECTURE_V3.md strictly.

Goal:
Design and implement the first zero-config detection flow for common desktop applications.

Purpose:
A user should be able to install resguard, run setup, and immediately benefit from useful default app classification.

Tasks:
1. Extend the discovery + policy + services flow so common apps get practical defaults without explicit manual wrap steps where safely possible.
2. Cover at least:
   - Firefox / Chromium family -> browsers
   - VS Code / Codium / JetBrains -> ide
   - Docker / Podman -> heavy
3. Keep ambiguity handling conservative.
4. Add/update tests for:
   - zero-config detection of common developer desktop apps
   - ambiguous identities remain non-destructive

Acceptance:
- zero-config desktop classification is practical for common developer machines
- cargo test passes

Suggested commit message:
feat(zero-config): add practical default application classification for common developer desktop apps
```

---

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
Introduce a simple but powerful `resguard top` command for class-oriented visibility.

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
