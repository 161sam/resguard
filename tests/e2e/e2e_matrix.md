# Resguard Field Test Matrix (GNOME/KDE)

Use this matrix for reproducible field validation on Ubuntu/Kubuntu hosts.

## Host Matrix

| Host | Desktop | Session | Install | Kernel | RAM | Script Result | Result File | Notes |
|---|---|---|---|---|---|---|---|---|
| Ubuntu 24.04 | GNOME | Wayland | apt |  |  | partial-pass | `tests/e2e/results/2026-03-08-ubuntu24.04-first-field.md` | apt/setup/apply/rescue/panic/suggest OK; snap Firefox flow patched (alias + DBus wrapper handling), live re-run pending; daemon package not installed by default |
| Ubuntu 24.04 | GNOME | X11 |  |  |  |  |  |  |
| Kubuntu 24.04 | KDE Plasma | Wayland |  |  |  |  |  |  |
| Kubuntu 24.04 | KDE Plasma | X11 |  |  |  |  |  |  |

## Execute

```bash
tests/e2e/run_e2e.sh --profile e2e-field --class rescue --install-method apt
```

Optional profile bootstrap/apply:

```bash
tests/e2e/run_e2e.sh --profile e2e-field --class rescue --setup-profile --install-method apt
```

## What `run_e2e.sh` covers

- system information snapshot (`os-release`, kernel, memory, desktop/session env)
- desktop/user manager checks (`systemctl`, `systemctl --user`)
- desktop wrap validation (`verify_desktop_wrap.sh`)
- rescue path validation (`verify_rescue.sh`)
- snap/non-snap desktop capture (`desktop list` + `desktop wrap` dry-run probes)
- suggest confidence + apply capture (`suggest --dry-run`, `suggest --apply`)
- markdown result capture in `tests/e2e/results/<timestamp>.md`
- structured `CAPTURE key=value` lines for easy comparison

## PASS/FAIL Criteria

- Script summary reports `fail=0`.
- `verify_rescue.sh` and `verify_desktop_wrap.sh` both pass.
- Result markdown file is generated in `tests/e2e/results/`.
- `CAPTURE` lines include install/session/snap/suggest fields for comparison.
