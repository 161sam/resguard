# Block B — v0.4: Autopilot / Daemon / TUI


• Implemented the first real autopilot decision engine in resguard-policy with conservative, deterministic behavior.

  ### What I added

  - New module: autopilot.rs
      - AutopilotState { last_action_tick }
      - AutopilotAction:
          - ReduceBrowsersLimits { memory_high_percent, memory_max_percent }
          - ReduceHeavyCpuWeight { cpu_weight }
          - PreserveRescueClass
      - AutopilotDecision { actions, next_state, in_cooldown }
      - decide_autopilot_actions(metrics, state, profile, now_tick)
  - Extended thresholds in thresholds.rs
      - memory_pressure_high_threshold() -> 20.0
      - cpu_pressure_high_threshold() -> 30.0
      - autopilot_cooldown_ticks() -> 3
  - Exported autopilot API in lib.rs
      - decide_autopilot_actions
      - AutopilotAction
      - AutopilotDecision
      - AutopilotState

  ### Decision model behavior

  - Input: MetricsSnapshot (pressure), AutopilotState, Profile, now_tick.
  - Output: deterministic action list + updated state.
  - Conservative actions only:
      - high memory pressure + browsers class exists -> reduce browsers limits
      - high CPU pressure + heavy class exists -> reduce heavy CPU weight
      - if any action and rescue exists -> add PreserveRescueClass
  - Cooldown: no actions during cooldown window to avoid thrashing.

  ### Tests added

  In autopilot.rs:

  - high memory pressure
  - high CPU pressure
  - no action below threshold
  - cooldown anti-thrashing behavior

  ### Validation

  - cargo fmt --all passed
  - cargo test -p resguard-policy passed
  - cargo test (workspace) passed

  Suggested commit message:
  feat(policy): add initial autopilot decision engine for adaptive pressure handling


---

• Implemented runtime-side adaptive limit support with explicit planning, apply, and revert paths.

  ### What was added

  - New module: adaptive.rs
      - Typed request/input:
          - ClassLimitRequest
          - ClassLimitCurrent
      - Typed planning/output:
          - AdaptiveChangePlan with changes and skipped_noop
          - PlannedClassLimitChange
      - Explicit apply/revert reporting:
          - AdaptiveApplyResult (applied, skipped_noop, warnings, revert_plan)
          - AdaptiveRevertPlan / AdaptiveRevertStep
          - AdaptiveRevertResult
      - Public functions:
          - read_class_limit_current
          - plan_class_limit_changes (+ injectable *_with variant for tests)
          - apply_class_limit_changes (+ injectable *_with)
          - revert_class_limit_changes (+ injectable *_with)
  - Extended systemd adapter in systemd.rs
      - Added typed setter:
          - systemctl_set_slice_limits(user, slice, memory_high, memory_max, cpu_weight)
      - Kept compatibility:
          - existing systemctl_set_slice_memory_limits now delegates to new typed setter
      - Added validation test for empty property set.
  - Export wiring in lib.rs
      - Exported new adaptive APIs/types and systemctl_set_slice_limits.

  ### Behavior guarantees

  - Scope is small and auditable: only explicit slice properties (MemoryHigh, MemoryMax, CPUWeight).
  - No-op handling is explicit at planning layer.
  - Revert path is explicit via returned AdaptiveRevertPlan.
  - Deterministic and testable through injected reader/apply closures.

  ### Tests added/updated

  In adaptive.rs:

  - class limit change planning
  - no-op when unchanged
  - apply/revert flow
  - no-op reporting preserved
  - multi-request planning behavior

  ### Validation

  - cargo fmt --all passed
  - cargo test -p resguard-runtime passed
  - cargo test (workspace) passed

  Suggested commit message:
  feat(runtime): add safe adaptive slice limit updates for autopilot actions

---

## Prompt B3 — `resguardd` auf echte Autopilot-Services umstellen


• Implemented and validated the daemon autopilot wiring through services.

  What I changed

  1. Rewired resguardd to use service orchestration (observe -> decide -> act) via daemon_autopilot_tick(...) from services, replacing ad-hoc watchdog/action
     logic in main.rs.
  2. Kept daemon behavior conservative by default:
      - default poll interval is 5000ms
      - no profile => safe no-op decision
      - once mode exits non-zero only on trigger.
  3. Preserved and improved operator visibility:
      - structured JSON logs for daemon_start, tick, once_decision, tick_failed, signal
      - ledger entries now record tick/decision/actions/applied/no-op/cooldown/profile/warnings.
  4. Added daemon tests covering requested paths in main.rs:
      - once mode
      - action trigger path
      - no-action path
      - cooldown behavior.
  5. Added daemon dependency on services in Cargo.toml.
  6. Cleaned service imports/formatting in daemon_service.rs.
  7. Fixed two clippy blockers in runtime so CI lint passes:
      - adaptive.rs
      - systemd.rs

  Root cause and fix summary

  - Root cause: daemon still contained legacy local watchdog policy/execution logic, bypassing the new architecture layers.
  - Fix: daemon now delegates autopilot decisions/actions to resguard-services, which uses resguard-policy + resguard-runtime internally.

  Validation run

  1. cargo fmt --all passed.
  2. cargo clippy --workspace --all-targets --all-features -- -D warnings passed.
  3. cargo test -p resguard-daemon passed.
  4. cargo test -p resguard-services passed.
  5. cargo test --workspace --all-targets passed.

  Suggested commit message: refactor(daemon): wire resguardd through services for autopilot execution

---

## Prompt B4 — TUI von “nice demo” zu nützlichem Operator-Tool machen


• Implemented a lightweight but much more useful TUI operator view via services, while keeping non-TTY fallback.

  What changed

  1. Added new TUI service snapshot layer in tui_service.rs:

  - pressure summary (CPU/MEM/IO)
  - memory totals
  - class slice live limits/usage (MemoryCurrent/High/Max, CPUWeight) based on active profile
  - recent daemon/autopilot actions from ${state_dir}/daemon-ledger.jsonl
  - unit tests for ledger parsing and class slice naming

  2. Exported service module in lib.rs.
  3. Refactored feature-gated CLI TUI command in tui.rs:

  - now consumes resguard_services::tui_service::collect_tui_snapshot(...)
  - interactive panes:
      - PSI
      - system memory + gauge
      - class slice table
      - recent autopilot/daemon actions table
  - non-TTY one-shot summary remains supported and now includes class/action visibility

  4. Updated TUI wiring in main.rs to pass config_dir and state_dir.
  5. Fixed legacy shim call for feature-gated builds in legacy.rs.
  6. Updated docs:

  - README.md TUI section now describes improved visibility and --no-top behavior
  - docs/cli.md now includes a resguard tui section with behavior and fallback notes

  Validation

  - cargo fmt --all passed
  - cargo build --workspace passed
  - cargo build -p resguard --features tui passed
  - cargo test --workspace --all-targets passed
  - cargo test -p resguard-services passed

  Suggested commit message: feat(tui): improve operator visibility into pressure class limits and autopilot state

---

## Prompt B5 — v0.4 Release Prep

• Prepared v0.4.0 release updates across versions, docs, and validation.

  Updated

  - Version bumps to 0.4.0 in all crate manifests and packaging control:
      - crates/resguard-cli/Cargo.toml
      - crates/resguard-daemon/Cargo.toml
      - crates/resguard-model/Cargo.toml
      - crates/resguard-policy/Cargo.toml
      - crates/resguard-discovery/Cargo.toml
      - crates/resguard-runtime/Cargo.toml
      - crates/resguard-config/Cargo.toml
      - crates/resguard-services/Cargo.toml
      - crates/resguard-state/Cargo.toml
      - crates/resguard-core/Cargo.toml
      - crates/resguard-system/Cargo.toml
      - packaging/deb/control
  - Changelog/release notes:
      - Added 0.4.0 entry with autopilot + daemon + TUI scope in CHANGELOG.md
      - Added docs/releases/v0.4.0.md
  - Release/install/docs updates:
      - Updated release flow/package matrix and examples in docs/releases.md
      - Updated install examples to v0.4.0 and added TUI packaging note in docs/install.md
      - Updated daemon safety/autopilot behavior in docs/safety.md
      - Updated README release/install references and v0.4 capabilities in README.md
      - Updated manpage version banner in docs/man/resguard.1
  - Release script improved to bump all relevant crate manifests:
      - scripts/release.sh

  Validation run

  - cargo build passed.
  - cargo build --features tui passed.
  - cargo test --workspace --all-targets passed.
  - ./scripts/release.sh --version 0.4.0 --dry-run passed.

  Notes

  - Docs now clearly state:
      - core package: resguard_<version>_amd64.deb
      - optional daemon package: resguard_<version>_amd64_daemon.deb
      - TUI remains feature-gated/source-build oriented.
  - release-assets/ was generated by dry-run (placeholders) and is currently untracked in the worktree.

  Suggested commit message: chore(release): prepare v0.4.0 with autopilot daemon and tui

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
