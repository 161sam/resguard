
# AGENTS.md

Guidelines for AI coding agents (Codex, OpenHands, Claude Code, etc.) working on the **Resguard** repository.

This file defines:

- architectural boundaries
- crate responsibilities
- coding workflow rules
- migration strategy

Agents MUST follow this document when generating code or refactoring.

---

# Project Vision

Resguard is a **Linux resource policy engine for desktop systems** built on:

- systemd slices
- cgroups v2
- PSI (pressure metrics)
- systemd-oomd

The goal is to provide:

- automatic resource control
- desktop application classification
- system responsiveness guarantees

Resguard is **not a process manager** and **not a container runtime**.

It is a **policy layer above systemd**.

---

# Core Architectural Principle

Resguard is organized into **strictly separated layers**.

Agents MUST preserve this separation.

```

CLI / Daemon / Interfaces
↓
Application Services
↓
Policy Engine
↓
Discovery Layer
↓
Runtime / Execution Backend
↓
Linux / systemd / kernel

```

No layer may bypass another.

---

# Crate Architecture

The workspace contains the following crates.

```

crates/
resguard-model/
resguard-policy/
resguard-discovery/
resguard-runtime/
resguard-config/
resguard-services/
resguard-cli/
resguard-daemon/

```

Each crate has **clear responsibilities**.

Agents MUST NOT mix responsibilities across crates.

---

# resguard-model

Purpose: shared domain types.

Contains:

- profile structures
- policy models
- suggestion types
- metrics snapshots
- action plans
- identity types

Examples:

```

Profile
ClassSpec
Suggestion
AppIdentity
PressureSnapshot
MetricsSnapshot
ActionPlan

```

Rules:

- no system calls
- no CLI logic
- no filesystem assumptions
- pure data structures

Dependencies allowed:

```

serde
thiserror
small utility crates

```

---

# resguard-policy

Purpose: decision logic.

Contains:

- classification rules
- confidence scoring
- default profiles
- autopilot decision engine
- threshold logic

Examples:

```

classify(identity) -> ClassMatch

score(identity, signals) -> ConfidenceScore

build_auto_profile(system_snapshot)

decide_pressure_actions(snapshot, state, profile)

```

Rules:

Policy layer MUST NOT:

- call systemctl
- read /proc
- access desktop files
- access filesystem paths
- parse snap paths

Policy only consumes data structures from `resguard-model`.

---

# resguard-discovery

Purpose: detect applications and identities.

Handles:

- XDG desktop discovery
- Snap desktop files
- Flatpak detection (future)
- systemd scope parsing
- Exec command parsing
- desktop-id resolution
- alias resolution

Examples:

```

scan_desktop_entries()

resolve_desktop_id("firefox.desktop")

parse_scope_identity(scope_name)

build_exec_index()

```

Discovery returns **AppIdentity** objects.

Discovery MUST NOT:

- classify apps
- assign resource classes
- apply slices
- modify system configuration

---

# resguard-runtime

Purpose: Linux execution backend.

Handles:

- systemd interactions
- slice creation
- set-property operations
- systemd-run
- PSI reading
- /proc parsing
- memory statistics
- action plan execution
- rollback operations

Examples:

```

read_pressure()

read_system_snapshot()

plan_apply(profile)

execute_plan(plan)

run_in_slice(slice, command)

set_slice_limits(slice)

```

Runtime MUST NOT:

- decide resource classes
- run policy decisions
- parse desktop entries
- run CLI logic

---

# resguard-config

Purpose: configuration and persistence.

Handles:

- profile loading
- YAML parsing
- desktop mapping storage
- daemon configuration
- state persistence

Examples:

```

load_profile()
save_profile()
load_desktop_mapping()
store_state()

```

---

# resguard-services

Purpose: application use-cases.

This crate orchestrates interactions between:

- discovery
- policy
- runtime
- config

Examples:

```

SetupService
SuggestService
DesktopService
DoctorService
MetricsService
PanicService
RescueService
DaemonService

```

Services implement workflows like:

```

setup
suggest
desktop wrap
apply
panic
rescue

```

Rules:

Services may call:

```

policy
runtime
discovery
config

```

Services must not contain:

- CLI code
- systemctl invocations
- direct filesystem logic

Those belong to runtime/config.

---

# resguard-cli

Purpose: command line interface.

Handles:

- clap definitions
- argument parsing
- service invocation
- output rendering

Examples:

```

resguard setup
resguard suggest
resguard desktop wrap
resguard doctor
resguard metrics
resguard panic
resguard rescue

```

Rules:

CLI MUST NOT implement business logic.

CLI calls services.

---

# resguard-daemon

Purpose: background autopilot process.

Responsibilities:

- event loop
- pressure monitoring
- autopilot execution
- signal handling
- daemon logging

Daemon MUST call `DaemonService`.

Daemon MUST NOT reimplement policy logic.

---

# Dependency Rules

Allowed dependency direction:

```

model
↑
policy
↑
discovery
↑
runtime
↑
services
↑
cli / daemon

```

Forbidden:

```

policy -> runtime
policy -> discovery

runtime -> cli
runtime -> services

discovery -> runtime

```

Agents MUST respect this architecture.

---

# Code Style Rules

Agents MUST follow these coding rules.

## Small PRs

Preferred PR size:

```

50–400 lines

```

Avoid large refactors in one commit.

---

## Deterministic Logic

Avoid hidden behavior.

Prefer explicit flows:

```

observe -> decide -> act

```

---

## Avoid Global State

Use explicit dependency passing.

---

## Error Handling

Use structured errors.

Preferred crates:

```

thiserror
anyhow (for CLI only)

```

---

# Testing Strategy

Each layer must have its own tests.

### policy tests

```

classification
confidence scoring
autopilot decisions

```

### discovery tests

```

snap parsing
desktop alias resolution
exec token parsing

```

### runtime tests

```

slice planning
systemd command generation
rollback planning

```

### service tests

```

suggest workflow
desktop wrap workflow
setup workflow

```

CLI tests should only validate argument parsing.

---

# Migration Strategy

The project is currently transitioning toward this architecture.

Migration phases:

### Phase 1

Introduce `resguard-model`.

Move shared types there.

---

### Phase 2

Introduce `resguard-discovery`.

Move:

- XDG scanning
- Snap detection
- desktop-id resolution
- scope parsing

out of CLI.

---

### Phase 3

Introduce `resguard-policy`.

Move:

- classification
- confidence scoring
- autoprofile logic

out of CLI.

---

### Phase 4

Introduce `resguard-services`.

Move workflows:

- setup
- suggest
- desktop wrap

into services.

---

### Phase 5

Stabilize `resguard-runtime`.

Centralize systemd interaction.

---

### Phase 6

Refactor daemon to use services.

---

# Agent Workflow

When implementing features, agents MUST:

1. Identify the correct layer.
2. Add logic to that crate.
3. Expose minimal API upward.
4. Write tests in that crate.
5. Update CLI only to call services.

---

# Feature Development Guidance

Typical implementation path:

### Example: Improve Suggest

1. discovery → improve identity detection
2. policy → update classification rules
3. services → update SuggestService
4. CLI → adjust output

Never implement all logic in CLI.

---

# Autopilot Design

Future autopilot logic must follow:

```

observe (runtime)
→ decide (policy)
→ execute (runtime)

```

Autopilot MUST NOT embed policy rules directly.

---

# Security Principles

Resguard modifies system resource limits.

Agents MUST ensure:

- safe path handling
- no arbitrary file writes
- no shell injection
- explicit unit file paths
- correct systemd reload behavior

---

# Non-Goals

Resguard will NOT become:

- a container runtime
- a full process manager
- a scheduler
- a GUI-heavy desktop tool

Focus remains:

```

desktop resource policy

```

---

# Summary

Resguard architecture is based on:

```

Model
Policy
Discovery
Runtime
Services
Interfaces

```

Agents MUST maintain strict separation between these layers.

All new features must respect this structure.
