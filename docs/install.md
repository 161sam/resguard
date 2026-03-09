# Install

Resguard bietet zwei offizielle Installationswege:

1. GitHub Release Assets (`.deb`) für manuelle, versionsgenaue Installation
2. APT Repository (GitHub Pages) für reguläre Paket-Updates via `apt`

## Download .deb from GitHub Release

Manueller Weg, wenn du eine konkrete Version direkt installieren willst.

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/resguard_0.4.0_amd64.deb"
sudo apt install -y ./resguard_0.4.0_amd64.deb
```

Optional: SHA256 prüfen

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

Optional: Daemon-Variante installieren (zusätzliche `resguardd`-Assets, nicht auto-enabled):

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/resguard_0.4.0_amd64_daemon.deb"
sudo apt install -y ./resguard_0.4.0_amd64_daemon.deb
```

Upgrade auf neue Versionen erfolgt erneut per Download + Installation.

## Install via APT repository

Empfohlen für Systeme, die Resguard kontinuierlich über `apt` aktualisieren sollen.

```bash
curl -fsSL "https://161sam.github.io/resguard/pubkey.gpg" \
  | gpg --dearmor \
  | sudo tee /usr/share/keyrings/resguard-archive-keyring.gpg >/dev/null

echo "deb [arch=amd64 signed-by=/usr/share/keyrings/resguard-archive-keyring.gpg] https://161sam.github.io/resguard stable main" \
  | sudo tee /etc/apt/sources.list.d/resguard.list >/dev/null

sudo apt update
sudo apt install -y resguard
```

Praktische Einordnung:

- `apt install resguard` ist der Standardweg für das Core-CLI.
- Für `resguardd` (optional) nutze weiterhin das `_daemon.deb` aus den GitHub Release Assets.

Das Repository wird signiert über GitHub Pages ausgeliefert (`Release.gpg` + `InRelease`).

Updates:

```bash
sudo apt update
sudo apt upgrade -y
```

## Unterschiede

- Release Asset:
  - manueller Download einer bestimmten Version
  - geeignet für kontrollierte Einzelinstallationen
- APT Repository:
  - einmaliges Setup
  - danach automatisierte Paket-Updates über den normalen APT-Prozess

Wenn ein neues Tag bereits als GitHub Release verfügbar ist, aber noch nicht im APT Repo erscheint,
ist typischerweise das Signing-Secret im Publishing-Workflow noch nicht gesetzt.
In diesem Fall vorübergehend den Release-Asset-Weg nutzen und Maintainer-Doku in `docs/releases.md` prüfen.

## Optional daemon package (`resguardd`)

Wenn du den optionalen Daemon nutzen willst:

1. Core über APT installieren (`resguard`).
2. Danach das versionsgleiche Daemon-Asset installieren:

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.4.0/resguard_0.4.0_amd64_daemon.deb"
sudo apt install -y ./resguard_0.4.0_amd64_daemon.deb
```

Hinweis:

- Der Daemon bleibt auch nach Installation deaktiviert, bis er explizit aktiviert wird.

## Quick validation (daemon)

Nach Installation des Daemon-Pakets:

```bash
resguard daemon status
sudo resguardd --once
```

Erwartung:

- `resguard daemon status` zeigt den aktuellen Service-Zustand.
- `sudo resguardd --once` läuft einmalig durch (ohne den Service dauerhaft zu starten).

## TUI status (v0.4)

Die TUI ist weiterhin feature-gated und nicht Teil der Standard-Debian-Pakete.

Für TUI-Nutzung aus Source:

```bash
cargo build -p resguard --features tui
cargo run -p resguard --features tui -- tui
```
