# Resguard Field Test Matrix (GNOME/KDE)

Use this matrix for reproducible field validation on Ubuntu/Kubuntu hosts.

## Host Matrix

| Host | Desktop | Session | Kernel | RAM | Script Result | Result File | Notes |
|---|---|---|---|---|---|---|---|
| Ubuntu 24.04 | GNOME | Wayland |  |  |  |  |  |
| Ubuntu 24.04 | GNOME | X11 |  |  |  |  |  |
| Kubuntu 24.04 | KDE Plasma | Wayland |  |  |  |  |  |
| Kubuntu 24.04 | KDE Plasma | X11 |  |  |  |  |  |

## Execute

```bash
tests/e2e/run_e2e.sh --profile e2e-field --class rescue
```

Optional profile bootstrap/apply:

```bash
tests/e2e/run_e2e.sh --profile e2e-field --class rescue --setup-profile
```

## What `run_e2e.sh` covers

- system information snapshot (`os-release`, kernel, memory, desktop/session env)
- desktop/user manager checks (`systemctl`, `systemctl --user`)
- desktop wrap validation (`verify_desktop_wrap.sh`)
- rescue path validation (`verify_rescue.sh`)
- suggest dry-run execution (`resguard suggest --dry-run`)
- markdown result capture in `tests/e2e/results/<timestamp>.md`

## PASS/FAIL Criteria

- Script summary reports `fail=0`.
- `verify_rescue.sh` and `verify_desktop_wrap.sh` both pass.
- Result markdown file is generated in `tests/e2e/results/`.
