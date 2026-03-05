# Resguard

Resguard ist ein natives Linux-Tool für Ressourcen-Isolation mit systemd slices und cgroups v2.

## Aktueller Stand (v0.1)

Implementiert:

- Profilschema + Validation
- `init`, `apply`, `rollback`, `run`, `status`
- `--root`-Isolation für sichere Testläufe ohne `/etc` zu ändern
- State/Backup/Manifest für Rollback

Teilweise implementiert / Stub:

- `diff`
- `profile list/show/import/export/new/edit`

## Build

```bash
cargo build
```

## Quickstart ohne sudo (`--root` Demo)

Die folgenden Schritte schreiben nur unter `/tmp/rgdemo`.

```bash
# 1) Profil automatisch erzeugen
cargo run -p resguard -- init --name demo --out /etc/resguard/profiles/demo.yml --root /tmp/rgdemo

# 2) Plan ansehen (keine Writes)
cargo run -p resguard -- apply demo --dry-run --root /tmp/rgdemo

# 3) Anwenden in isoliertem Root
cargo run -p resguard -- apply demo --root /tmp/rgdemo

# 4) Ergebnis prüfen
find /tmp/rgdemo/etc/systemd -type f | sort

# 5) Rollback
cargo run -p resguard -- rollback --last --root /tmp/rgdemo
```

Hinweis:

- Bei `--root /tmp/...` wird **kein** `systemctl daemon-reload` ausgeführt.

## Wichtige Pfade

Defaults:

- Config dir: `/etc/resguard`
- State dir: `/var/lib/resguard`

Mit `--root /tmp/rg` werden daraus:

- `/tmp/rg/etc/resguard`
- `/tmp/rg/var/lib/resguard`

State/Backups:

- `${state_dir}/state.json`
- `${state_dir}/backups/<backup_id>/manifest.json`
- `${state_dir}/backups/<backup_id>/...` (gesicherte Dateien)

## `run --class` Voraussetzungen

`run` benötigt entweder:

- ein aktives Profil in `state.json` (nach `apply`), oder
- `--profile <name>`

Zusätzlich muss die Slice existieren (Check via `systemctl cat` bzw. `systemctl --user cat`).
Wenn nicht, bricht `run` mit Hinweis "apply profile first" ab.

## Status

`status` arbeitet best-effort und kann ohne root ausgeführt werden.
Bei teilweise fehlenden Informationen gibt der Command Warnungen aus und endet mit Exitcode `1`.

```bash
cargo run -p resguard -- status
```

## Weitere Doku

- [CLI](docs/cli.md)
- [Design](docs/design.md)
- [Safety](docs/safety.md)
- [Profiles](docs/profiles.md)
