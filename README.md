# Resguard

Resguard ist ein natives Linux-Tool für Ressourcen-Isolation mit systemd slices und cgroups v2.

## Aktueller Stand (v0.2)

Implementiert:

- Profilschema + Validation
- `init`, `apply`, `rollback`, `run`, `status`
- `desktop list`, `desktop wrap`, `desktop unwrap`, `desktop doctor`
- `--root`-Isolation für sichere Testläufe ohne `/etc` zu ändern
- State/Backup/Manifest für Rollback

Teilweise implementiert / Stub:

- `diff`
- `profile list/show/import/export/new/edit`

## Build

```bash
cargo build
```

## Installation

### Download .deb from GitHub Release

Manueller Installationsweg pro Version:

```bash
VERSION="0.2.1"
curl -fsSLO "https://github.com/<owner>/<repo>/releases/download/v${VERSION}/resguard_${VERSION}_amd64.deb"
sudo apt install -y "./resguard_${VERSION}_amd64.deb"
```

Optional: Checksum verifizieren

```bash
VERSION="0.2.1"
curl -fsSLO "https://github.com/<owner>/<repo>/releases/download/v${VERSION}/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

### Install via APT repository

APT-Repository auf GitHub Pages einrichten (inkl. automatischer Updates via `apt upgrade`):

```bash
curl -fsSL "https://<owner>.github.io/<repo>/pubkey.gpg" \
  | gpg --dearmor \
  | sudo tee /usr/share/keyrings/resguard-archive-keyring.gpg >/dev/null

echo "deb [arch=amd64 signed-by=/usr/share/keyrings/resguard-archive-keyring.gpg] https://<owner>.github.io/<repo> stable main" \
  | sudo tee /etc/apt/sources.list.d/resguard.list >/dev/null

sudo apt update
sudo apt install -y resguard
```

Unterschiede:

- GitHub Release Asset: manueller Download und manuelles Upgrade pro Version.
- APT Repository: einmal einrichten, danach automatische Update-Pipeline via `apt`.

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

## TUI usage and feature flag

Die TUI ist optional und wird nur mit Feature-Flag gebaut:

```bash
cargo run -p resguard --features tui -- tui
```

Optionen:

- `--interval <ms>`: Refresh-Intervall (default `1000`)
- `--no-top`: nur Summary (ohne Top scopes/slices Tabelle)

Non-interactive Fallback:

- Wenn `stdout` kein TTY ist, gibt `resguard tui` automatisch eine einmalige Summary aus und beendet sich.

## Weitere Doku

- [CLI](docs/cli.md)
- [Design](docs/design.md)
- [Install](docs/install.md)
- [Safety](docs/safety.md)
- [Threat Model](docs/threat-model.md)
- [Profiles](docs/profiles.md)
- [Releases](docs/releases.md)

## Release / Tagging

Release-Ablauf:

```bash
see docs/releases.md
```
