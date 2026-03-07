# Install

Resguard bietet zwei offizielle Installationswege:

1. GitHub Release Assets (`.deb`) für manuelle, versionsgenaue Installation
2. APT Repository (GitHub Pages) für reguläre Paket-Updates via `apt`

## Download .deb from GitHub Release

Manueller Weg, wenn du eine konkrete Version direkt installieren willst.

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.2.1/resguard_0.2.1_amd64.deb"
sudo apt install -y ./resguard_0.2.1_amd64.deb
```

Optional: SHA256 prüfen

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.2.1/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

Optional: Daemon-Variante installieren (zusätzliche `resguardd`-Assets, nicht auto-enabled):

```bash
curl -fsSLO "https://github.com/161sam/resguard/releases/download/v0.2.1/resguard_0.2.1_amd64_daemon.deb"
sudo apt install -y ./resguard_0.2.1_amd64_daemon.deb
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
