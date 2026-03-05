# Desktop Wrapper Design (v0.2)

Status: Design for v0.2. No full implementation yet.

## Goals

- Launch GUI apps through `resguard run --class ...` without requiring users to type commands.
- Keep behavior reversible (`unwrap`) and transparent.
- Preserve XDG desktop semantics (`Exec` placeholders, icons, names, categories).

## Scope

Planned commands:

- `resguard desktop list`
- `resguard desktop wrap`
- `resguard desktop unwrap`
- `resguard desktop doctor`

Current code status:

- CLI stubs exist.
- No wrapper generation/rewriting logic is active yet.

## XDG Lookup Rules

Wrapper design uses standard desktop entry lookup order.

Source candidates (in priority order for user-facing selection):

1. `$XDG_DATA_HOME/applications` (fallback `~/.local/share/applications`)
2. `/usr/local/share/applications`
3. `/usr/share/applications`
4. each directory in `$XDG_DATA_DIRS` + `/applications`

Rules:

- Prefer user-local entries over system entries for the same Desktop ID.
- Ignore hidden/invalid entries (`NoDisplay=true` can still be listed in advanced mode, but not default).
- Require `.desktop` extension and parseable `[Desktop Entry]` block.

## Wrapper Naming Strategy

### Desktop ID strategy

Given source desktop id `foo.desktop`:

- wrapper desktop id: `foo-resguard.desktop`
- wrapper written to: `~/.local/share/applications/foo-resguard.desktop`

Why:

- avoids mutating system files
- reversible and user-local
- no collision with upstream package-managed desktop file

### Display Name strategy

- preserve original `Name`
- append suffix for transparency, e.g. `Name=Firefox (Resguard)`

### Metadata markers

Wrapper should include explicit managed markers:

- `X-Resguard-Managed=true`
- `X-Resguard-SourceDesktopId=foo.desktop`
- `X-Resguard-Class=browsers`

These markers make `unwrap` and `doctor` deterministic.

## Exec Rewrite Strategy

Original desktop `Exec` must be rewritten to run through resguard while preserving placeholders.

Target template:

```text
Exec=resguard run --class <class> -- <original-binary-and-args-preserving-placeholders>
```

### Placeholder handling

Preserve these placeholders exactly as desktop-spec tokens:

- `%u`, `%U`
- `%f`, `%F`
- `%i`, `%c`, `%k` (if present)

Special handling:

- `%%` stays literal `%`
- unsupported/invalid placeholder tokens should cause wrap validation error (doctor can suggest fix)

Do not shell-join `Exec`.

- Parse desktop `Exec` into argv-like tokens conservatively.
- Reconstruct wrapper `Exec` as a token sequence (desktop format), not `sh -c`.

## Wrap/Unwrap Behavior

## `desktop wrap`

- validate source desktop entry exists and has usable `Exec`
- produce wrapper file in user-local applications dir
- do not overwrite unknown unmanaged files
- if wrapper already managed by resguard:
  - update class/exec idempotently

## `desktop unwrap`

- only remove files marked `X-Resguard-Managed=true`
- verify source marker before delete
- never delete non-managed files

## Doctor Checks (planned)

- duplicate wrappers for same source id
- missing source desktop file
- `resguard` binary not in PATH for session
- class not in active/selected profile
- user slices missing (`systemctl --user cat resguard-<class>.slice`)
- stale wrapper markers (managed file malformed)

## Safety Constraints

- Never edit files under `/usr/share/applications` or `/usr/local/share/applications`.
- Only write/remove in user-local applications directory.
- All writes must be atomic + backup-aware once v0.2 implementation lands.
- Respect existing rollback/state architecture where applicable.

## Open Decisions

- Whether wrapper should replace launcher ordering (`OnlyShowIn`, `Actions`) or preserve as-is.
- Whether to generate one wrapper per class per app or update-in-place by source id.
- How strict token parsing should be for complex Exec lines with quotes/escapes.
