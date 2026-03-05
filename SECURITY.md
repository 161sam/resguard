
# Security Policy

## Overview

Resguard modifies systemd slice configuration and therefore influences system resource limits.

Misconfiguration could affect system stability.

Security and safety are core design principles.

---

# Supported Versions

Currently supported:

| Version | Supported |
|-------|--------|
0.1 | yes

---

# Reporting Security Issues

Please report security issues privately.

Email:

security@resguard.dev

Include:

- affected version
- reproduction steps
- logs if possible

Do not open public issues for security vulnerabilities.

---

# Threat Model

Resguard assumes:

- attacker may control user processes
- attacker may attempt to exhaust system resources
- attacker may attempt to bypass slice limits

Goals:

- system must remain responsive
- privileged system services must remain operational
- resource exhaustion must be contained

---

# Attack Surfaces

### Profile Files

Profiles are YAML.

Risk:

- malformed profiles

Mitigation:

- strict validation
- schema checks

---

### Systemd Interaction

Resguard executes:

```

systemctl
systemd-run

```

Risk:

- command injection

Mitigation:

- arguments never interpolated into shell
- commands executed directly via exec

---

### File Writes

Resguard writes to:

```

/etc/systemd/system/

```

Risk:

- overwrite unrelated configuration

Mitigation:

- only files prefixed with resguard
- explicit managed markers

---

# Privilege Model

Commands requiring root:

- apply
- rollback

Commands not requiring root:

- profile commands
- status
- run (if slice exists)

---

# Denial of Service

Worst case:

- misconfigured memory limits

Mitigation:

- dry-run
- validation
- rollback

---

# Responsible Use

Resguard should be used carefully on production systems.

Always test profiles with:

```

resguard apply <profile> --dry-run

```

before applying.