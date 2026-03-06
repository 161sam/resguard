# Resguard Field Test Matrix (Ubuntu Desktop)

Use this matrix for reproducible desktop field tests with `tests/e2e/run_e2e.sh`.

## Environment Matrix

| Desktop | Session | Kernel (`uname -r`) | RAM (`MemTotal`) | Swap | Result | Notes |
|---|---|---|---|---|---|---|
| GNOME | Wayland |  |  |  |  |  |
| GNOME | X11 |  |  |  |  |  |
| KDE Plasma | Wayland |  |  |  |  |  |
| KDE Plasma | X11 |  |  |  |  |  |

## Repro Steps

1. Run `tests/e2e/run_e2e.sh --profile e2e-field --class heavy`.
2. Record environment:
   - Desktop/session: `echo "$XDG_CURRENT_DESKTOP / $XDG_SESSION_TYPE"`
   - Kernel: `uname -r`
   - RAM: `grep MemTotal /proc/meminfo`
3. Copy PASS/FAIL summary from script output into the matrix.
4. If FAIL, attach:
   - failing check name
   - exact stderr/stdout snippet
   - whether `stress-ng` was used

## Success Criteria

- Command responsiveness: `resguard run ... --wait -- true` completes in <= 2000 ms.
- `htop` starts via `resguard run` (checked with `htop --help` execution in class/slice).
- Force kill path works (`kill -9` returns expected killed status path).

