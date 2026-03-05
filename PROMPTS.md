## Codex Prompt B — Profile schema v1 + YAML load/save + validation helpers

**Ziel:** `resguard-core` enthält Profile structs; `resguard-config` kann Profile aus Store laden/speichern; `validate` prüft sanity.

**Aufgaben**

1. Implementiere Profile structs passend zu `docs/design.md` (nur v0.1 Felder nötig: memory/cpu/oomd/classes).
2. Implementiere `parse_size("12G") -> u64 bytes` + `parse_cpuset("1-7,9") -> Vec<u32>` (sanity).
3. Implementiere `validate_profile(&Profile) -> Vec<ValidationError>`.

**Code-Snippet: size parsing (minimal, robust)**

```rust
pub fn parse_size_to_bytes(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() { return Err("empty size".into()); }
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let (n_str, u_str) = if unit.chars().all(|c| c.is_ascii_alphabetic()) {
        (num.trim(), unit.trim())
    } else {
        (s, "")
    };
    let n: u64 = n_str.parse().map_err(|_| format!("invalid number: {n_str}"))?;
    let mult: u64 = match u_str.to_ascii_uppercase().as_str() {
        "" => 1,
        "K" => 1024,
        "M" => 1024_u64.pow(2),
        "G" => 1024_u64.pow(3),
        "T" => 1024_u64.pow(4),
        _ => return Err(format!("invalid unit: {u_str}")),
    };
    n.checked_mul(mult).ok_or_else(|| "size overflow".into())
}
```

**Code-Snippet: validation sanity**

```rust
pub fn validate_memory(high: Option<&str>, max: Option<&str>) -> Result<(), String> {
    if let (Some(h), Some(m)) = (high, max) {
        let hb = parse_size_to_bytes(h)?;
        let mb = parse_size_to_bytes(m)?;
        if mb < hb {
            return Err(format!("MemoryMax ({m}) must be >= MemoryHigh ({h})"));
        }
    }
    Ok(())
}
```

**Acceptance**

* `resguard profile validate <file>` liefert klare Fehler
* Unit tests für parse_size + sanity

---

## Codex Prompt C — `resguard init` (auto profile detect + write)

**Ziel:** `resguard init` liest Hardware (RAM/CPU) und erzeugt Profile YAML. Root schreibt ins Store, non-root in CWD. Optional `--apply`.

**Aufgaben**

1. `resguard-system`: `read_mem_total_bytes()` aus `/proc/meminfo` (MemTotal)
2. CPU count über `std::thread::available_parallelism()`
3. Implementiere Heuristiken (16G→2G reserve etc.)
4. YAML generieren und speichern:

   * root: `${config_dir}/profiles/<name>.yml`
   * non-root: `./<name>.yml` (oder `--out`)
5. `--dry-run`: nur YAML ausgeben
6. `--apply`: ruft intern apply auf (kein shell)

**Code-Snippet: /proc/meminfo parsing**

```rust
pub fn mem_total_bytes() -> anyhow::Result<u64> {
    let content = std::fs::read_to_string("/proc/meminfo")?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            // e.g. "       16327156 kB"
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let kb: u64 = parts.get(0).ok_or_else(|| anyhow::anyhow!("MemTotal parse"))?.parse()?;
            return Ok(kb * 1024);
        }
    }
    Err(anyhow::anyhow!("MemTotal not found"))
}
```

**Code-Snippet: reserve heuristic**

```rust
pub fn default_reserve_bytes(total: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    match total / gb {
        0..=9  => 1 * gb,
        10..=19 => 2 * gb,
        20..=39 => 4 * gb,
        _ => 6 * gb,
    }
}
```

**Acceptance**

* `resguard init --dry-run` prints valid YAML
* `sudo resguard init --apply` creates profile in `/etc/resguard/profiles/` and applies it

---

## Codex Prompt D — Planner + Apply (system drop-ins + class slices system+user) + `--root`

**Ziel:** `apply` schreibt genau die resguard-managed files unter `--root`, erstellt dirs, schreibt content, macht system daemon-reload, optional best-effort user daemon-reload.

**Aufgaben**

1. Implementiere Plan-Actions:

   * EnsureDir, WriteFile, Exec (systemctl/systemd-run)
2. Apply schreibt:

   * `${root}/etc/systemd/system/system.slice.d/50-resguard.conf`
   * `${root}/etc/systemd/system/user.slice.d/50-resguard.conf`
   * `${root}/etc/systemd/system/resguard-<class>.slice`
   * `${root}/etc/systemd/user/resguard-<class>.slice`
3. Bei `--dry-run`: keine Writes, nur plan anzeigen
4. Nach Writes:

   * `systemctl daemon-reload` **nur wenn root=="/"** (bei Tests skip)
5. `--user-daemon-reload`: best-effort `sudo -u $SUDO_USER systemctl --user daemon-reload` (nur wenn möglich)

**Code-Snippet: write file with parent dir**

```rust
pub fn write_file(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}
```

**Code-Snippet: systemd drop-in render**

```rust
pub fn render_user_slice_dropin(memory_high: Option<&str>, memory_max: Option<&str>, allowed_cpus: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str("# Managed by resguard. DO NOT EDIT.\n[Slice]\n");
    if let Some(v) = memory_high { s.push_str(&format!("MemoryHigh={v}\n")); }
    if let Some(v) = memory_max { s.push_str(&format!("MemoryMax={v}\n")); }
    if let Some(v) = allowed_cpus { s.push_str(&format!("AllowedCPUs={v}\n")); }
    s
}
```

**Acceptance**

* `resguard apply <profile> --dry-run` shows which files would be written
* `resguard apply <profile> --root /tmp/rgtest` writes only under /tmp/rgtest and does NOT call systemctl

---

## Codex Prompt E — State + Backups + Rollback (transactional best-effort)

**Ziel:** Jede Apply-Operation macht Backups, schreibt state.json, Rollback stellt wieder her.

**Aufgaben**

1. Implementiere Backup dir: `${state_dir}/backups/<timestamp>/...`
2. Vor jedem Write:

   * falls target exists: copy into backup (same relative path)
   * falls nicht exists: mark as “created”
3. state.json speichert:

   * active profile
   * backup id (timestamp)
   * managed paths list
   * created paths list
4. Apply Fehler → automatisch rollback attempt
5. Rollback:

   * restore backed-up files
   * delete created files
   * systemctl daemon-reload (nur root=="/")

**Code-Snippet: backup path mapping**

```rust
pub fn backup_path(backup_root: &std::path::Path, target: &std::path::Path, root: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let rel = target.strip_prefix(root).unwrap_or(target);
    Ok(backup_root.join(rel))
}
```

**Acceptance**

* Integration test: apply → modify generated file → rollback restores original
* apply failure triggers rollback attempt

---

## Codex Prompt F — `run --class` echte Ausführung (user vs system mode) + `--wait`

**Ziel:** `run` nutzt `systemd-run` korrekt, ohne shell, und wählt automatisch `--user` wenn nicht root.

**Aufgaben**

1. Slice Resolution:

   * aus active profile (state.json) oder `--profile`
   * `--slice` override
2. Mode:

   * euid==0 → system
   * else → user
3. Existence check:

   * user: `systemctl --user cat <slice>` (best-effort; wenn fail → trotzdem run? v0.1: fail hard mit Hinweis “apply first”)
4. Exec:

   * user: `systemd-run --user --scope -p Slice=<slice> -- <cmd...>`
   * system: `systemd-run --scope -p Slice=<slice> -- <cmd...>`
5. `--wait`:

   * add `--wait` to systemd-run and forward exit code

**Code-Snippet: exec systemd-run**

```rust
use std::process::Command;

pub fn systemd_run(user: bool, slice: &str, wait: bool, cmd: &[String]) -> anyhow::Result<i32> {
    let mut c = Command::new("systemd-run");
    if user { c.arg("--user"); }
    c.arg("--scope");
    if wait { c.arg("--wait"); }
    c.arg("-p").arg(format!("Slice={slice}"));
    c.arg("--");
    for a in cmd { c.arg(a); }
    let status = c.status()?;
    Ok(status.code().unwrap_or(1))
}
```

**Acceptance**

* `resguard run --class browsers -- echo hi` starts successfully
* `resguard run --class browsers --wait -- false` returns non-zero

---

## Codex Prompt G — `status` minimal nützlich (system + best-effort user)

**Ziel:** `status` zeigt active profile, system slice props, class slices props, oomd active, PSI summary.

**Aufgaben**

1. state.json lesen: active profile + class slices list
2. `systemctl show user.slice system.slice` parse key props:

   * MemoryHigh, MemoryMax, MemoryLow, AllowedCPUs
3. best-effort user:

   * `systemctl --user show resguard-browsers.slice` (wenn fail: warn)
4. PSI summary:

   * parse `/proc/pressure/memory` and `/proc/pressure/cpu` (nur 1-min avg)
5. Ausgabe als table-like text ok

**Code-Snippet: PSI parse (tiny)**

```rust
pub fn read_pressure_1min(path: &str) -> anyhow::Result<Option<f64>> {
    let s = std::fs::read_to_string(path)?;
    // line: "some avg10=0.00 avg60=0.00 avg300=0.00 total=0"
    for line in s.lines() {
        if line.starts_with("some ") {
            for tok in line.split_whitespace() {
                if let Some(v) = tok.strip_prefix("avg60=") {
                    return Ok(Some(v.parse()?));
                }
            }
        }
    }
    Ok(None)
}
```

**Acceptance**

* `resguard status` works without root
* If `systemctl show` fails, prints best-effort diagnostics and exits 1 (per spec)

---

## Codex Prompt H — CI + Quality gates (fmt, clippy, test) + minimal changelog

**Ziel:** Stabiler Dev-Loop.

**Aufgaben**

1. Add GitHub Actions: build/test/fmt/clippy
2. Ensure `cargo fmt` clean
3. Add `CHANGELOG.md` initial entry v0.1.0 (unreleased)

**Acceptance**

* CI green on push

---

# Bonus: “Agent Loop” Meta-Prompt (für Codex Autopilot)

Wenn du willst, dass Codex in kleinen PRs arbeitet:

```text
You are working in the resguard repository. Follow AGENTS.md strictly.
Implement milestone v0.1 in small, deterministic commits. Each commit must:
- compile (cargo build)
- include tests where applicable
- update docs if behavior changes

Work in this order:
1) Profile schema + validation (core/config)
2) init command (detect + generate)
3) apply/dry-run with --root support + system and user slice generation
4) state+backup+rollback
5) run --class (user/system mode) + --wait
6) status minimal

Never use shell invocation. Use std::process::Command with args.
Never write outside paths under --root. All writes must be backup-protected.
Prefer explicit errors and clear user-facing messages.
```

---
