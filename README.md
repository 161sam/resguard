# Resguard

Resguard ist ein natives Linux-Tool für Ressourcen-Isolation mit systemd slices und cgroups v2.

## Aktueller Stand (v0.4)

Implementiert:

- Profilschema + Validation
- `init`, `apply`, `rollback`, `run`, `status`
- `suggest` mit confidence-threshold und optionalem `--apply`
- `desktop list`, `desktop wrap`, `desktop unwrap`, `desktop doctor`
- optionales `resguardd` mit services-basierter Autopilot-Ausführung (`observe -> decide -> act`)
- feature-gated TUI Operator-View (`--features tui`)
- `--root`-Isolation für sichere Testläufe ohne `/etc` zu ändern
- State/Backup/Manifest für Rollback

Teilweise implementiert / Stub:

- `diff`
- `profile list/show/import/export/new/edit`

## Build

```bash
cargo build
./target/debug/resguard --version
./target/debug/resguard version
```

## Installation

### Download .deb from GitHub Release

Manueller Installationsweg pro Version (aktuell `v0.4.0`):

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/resguard_0.4.0_amd64.deb"
sudo apt install -y ./resguard_0.4.0_amd64.deb
```

Optional: Checksum verifizieren

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

Optional: separates Daemon-Paket installieren (enthält `resguardd` + systemd-Service-Template).
Der Daemon bleibt nach Installation deaktiviert, bis er explizit aktiviert wird.

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/resguard-daemon_0.4.0_amd64.deb"
sudo apt install -y ./resguard-daemon_0.4.0_amd64.deb
```

### Install via APT repository

Einmaliges Setup gegen das GitHub-Pages-APT-Repo (`stable main`):
Das Repo wird aus `apt/` per Workflow `.github/workflows/apt-pages.yml` veröffentlicht.

```bash
curl -fsSL "https://161sam.github.io/resguard/pubkey.gpg" \
  | gpg --dearmor \
  | sudo tee /usr/share/keyrings/resguard-archive-keyring.gpg >/dev/null

echo "deb [arch=amd64 signed-by=/usr/share/keyrings/resguard-archive-keyring.gpg] https://161sam.github.io/resguard stable main" \
  | sudo tee /etc/apt/sources.list.d/resguard.list >/dev/null

sudo apt update
sudo apt install -y resguard
```

Upgrades danach:

```bash
sudo apt update
sudo apt upgrade -y
```

Unterschiede:

- GitHub Release Asset: manueller Download und manuelles Upgrade pro Version.
- APT Repository: einmal einrichten, danach Upgrades über den normalen `apt`-Prozess.
- `apt install resguard` deckt den Core-Weg ab.
- Optionaler Daemon ist ein separates Paket: `resguard-daemon` (Release-Asset oder APT).

Hinweis zur Veröffentlichung:

- Die APT-Metadaten werden signiert veröffentlicht.
- Falls das Signing-Secret noch nicht eingerichtet ist, wird nur der GitHub-Release-Upload ausgeführt.
- Details: [docs/releases.md](docs/releases.md)

Daemon-Validierung nach Installation des optionalen `resguard-daemon`-Pakets:

```bash
apt policy resguard
apt policy resguard-daemon
resguardd --help
systemctl status resguardd --no-pager
resguard daemon status
sudo resguardd --once
systemctl cat resguardd.service
```

## Post-install Quickstart

```bash
resguard doctor
sudo resguard setup
resguard suggest
```

Optional Desktop-Wrap (Beispiel):

```bash
resguard desktop list --filter firefox
# auf Ubuntu/Snap oft: firefox_firefox.desktop
resguard desktop wrap firefox.desktop --class browsers
sudo resguard apply auto --user-daemon-reload
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

## TUI usage and feature flag

Die TUI ist optional und wird nur mit Feature-Flag gebaut:

```bash
cargo run -p resguard --features tui -- tui
```

Optionen:

- `--interval <ms>`: Refresh-Intervall (default `1000`)
- `--no-top`: nur kompakte Summary (ohne Klassen-/Aktionstabellen)

Ansicht (Default):

- PSI Summary (CPU/MEM/IO)
- Memory Überblick (Total/Available + Nutzungsbalken)
- Klassen-Slices mit aktuellen Limits/Nutzung (`MemoryCurrent/High/Max`, `CPUWeight`)
- Letzte Daemon/Autopilot-Aktionen aus `${state_dir}/daemon-ledger.jsonl` (wenn vorhanden)

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

Release-Ablauf und Tagging: [docs/releases.md](docs/releases.md)

Kurzfassung für Maintainer:

```bash
./scripts/release.sh --version <x.y.z>
git tag -a v<x.y.z> -m "resguard v<x.y.z>"
git push origin v<x.y.z>
```
