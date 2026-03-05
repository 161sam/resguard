
# AGENTS.md

Guidelines for human and AI agents contributing to the **Resguard** project.

Resguard is a **native Linux system tool** written in Rust that manages
system resource isolation using **systemd slices and cgroups v2**.

The project prioritizes:

- system stability
- predictable behavior
- safe system modification
- reproducible builds
- minimal dependencies

Agents working on this repository must follow the guidelines below.

---

# Project Purpose

Resguard ensures that Linux systems remain responsive even under extreme
CPU and memory pressure.

It accomplishes this by:

- reserving resources for the operating system
- limiting user workloads
- isolating workloads in dedicated systemd slices
- allowing controlled process launching via `resguard run`

Resguard is designed to function **agentless by default**, relying on systemd
as the permanent enforcement layer.

---

# Architecture Overview

Resguard is implemented as a Rust workspace.

```

resguard/
crates/
resguard-cli
resguard-core
resguard-system
resguard-config
resguard-state

```

### Crate responsibilities

| crate | responsibility |
|------|---------------|
resguard-cli | CLI interface and command dispatch
resguard-core | profile schema, validation, planner, diff engine
resguard-system | systemd interaction and Linux adapters
resguard-config | profile storage and loading
resguard-state | state tracking, backups, rollback

---

# Key Concepts

## Agentless Enforcement

Resguard does **not run continuously**.

Instead:

1. `resguard apply` writes systemd configuration
2. systemd enforces the limits permanently

This ensures:

- reliability
- low overhead
- minimal attack surface

---

## Workload Classes

Applications may run in slices such as:

```

resguard-browsers.slice
resguard-ide.slice
resguard-heavy.slice

```

Processes are placed into slices using:

```

resguard run --class <class>

```

This uses transient scopes via `systemd-run`.

---

# System Integration

Resguard writes systemd configuration files to:

```

/etc/systemd/system/
/etc/systemd/user/

```

Examples:

```

/etc/systemd/system/system.slice.d/50-resguard.conf
/etc/systemd/system/user.slice.d/50-resguard.conf
/etc/systemd/system/resguard-browsers.slice
/etc/systemd/user/resguard-browsers.slice

```

These files are always marked:

```

# Managed by resguard. DO NOT EDIT.

```

Agents must **never modify unrelated system files**.

---

# Documentation Map

Agents must consult documentation before implementing features.

| Document | Description |
|--------|-------------|
docs/design.md | system architecture and design decisions
docs/cli.md | CLI specification
docs/profiles.md | profile schema and examples
docs/safety.md | rollback, safety model
docs/issues-roadmap.md | roadmap and milestones

---

# Development Principles

## Safety First

System stability is more important than feature completeness.

All changes must:

- support dry-run
- support rollback
- avoid destructive writes

---

## Idempotent Operations

Running the same command twice must not change the system state.

Example:

```

resguard apply profile
resguard apply profile

```

must produce identical results.

---

## Explicit System Changes

Never perform hidden system modifications.

Every change must be:

- visible in `diff`
- reversible via `rollback`

---

# Rust Coding Guidelines

## General Rules

- Use **stable Rust only**
- Prefer **explicit types**
- Avoid unnecessary abstractions
- Favor readability over cleverness

---

## Error Handling

Use structured error handling.

Preferred pattern:

```

anyhow::Result<T>

```

For libraries:

```

thiserror

```

Never panic for recoverable errors.

---

## Logging

Logging must be minimal and structured.

Use verbosity flags:

```

--verbose
--quiet

```

Do not print secrets or environment dumps.

---

## CLI Design

CLI must follow the spec in:

```

docs/cli.md

```

Use:

```

clap

```

for argument parsing.

---

## File Writing

All file writes must follow this pattern:

1. validate input
2. create backup
3. write file
4. reload systemd
5. update state

Never write directly without backup.

---

# System Command Execution

External commands include:

```

systemctl
systemd-run

```

Agents must execute commands using **direct exec**, never shell strings.

Example (Rust):

```

Command::new("systemctl")
.arg("daemon-reload")
.status()?;

```

Never use:

```

sh -c
bash -c

```

---

# Security Requirements

Resguard interacts with privileged system components.

Security rules:

- no command injection
- no path traversal
- validate all profile inputs
- never overwrite unknown files

See:

```

SECURITY.md

```

---

# Testing Requirements

All new functionality must include tests.

Required test types:

### Unit Tests

- profile parsing
- validation logic
- planner output

### Snapshot Tests

Planner diff results.

### Integration Tests

Using fake root directory:

```

resguard --root /tmp/test

```

Ensure:

- correct file generation
- rollback restores state

---

# Code Style

Follow Rust conventions:

```

cargo fmt
cargo clippy

```

Must pass before committing.

---

# Commit Guidelines

Commits must follow conventional commit style.

Examples:

```

feat(cli): add init command
fix(systemd): correct slice generation
refactor(core): simplify planner logic
docs: update profile documentation

```

---

# Dependency Policy

Dependencies must remain minimal.

Allowed categories:

- CLI parsing
- serialization
- error handling

Avoid large frameworks.

---

# Future Architecture

Resguard will evolve in phases.

### v0.1
Agentless system manager integration.

### v0.2
Desktop wrapper generation.

### v0.3
Optional `resguardd` event daemon.

Agents must **not implement v0.2/v0.3 features prematurely**.

Follow roadmap:

```

docs/issues-roadmap.md

```

---

# AI Agent Rules

Agents must:

1. read project documentation first
2. respect architecture boundaries
3. never introduce unsafe system modifications
4. maintain backward compatibility
5. prefer small incremental changes

Agents must not:

- introduce background daemons without design approval
- modify system paths outside Resguard scope
- break rollback guarantees

---

# Contribution Workflow

Typical development flow:

```

cargo build
cargo test
cargo fmt
cargo clippy

```

Before submitting changes.

---

# Project Philosophy

Resguard follows the Unix philosophy:

- small
- predictable
- transparent

System behavior must always be understandable and reversible.
