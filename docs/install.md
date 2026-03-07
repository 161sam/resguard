# Install

Resguard bietet zwei offizielle Installationswege:

1. GitHub Release Assets (`.deb`) für manuelle, versionsgenaue Installation
2. APT Repository (GitHub Pages) für reguläre Paket-Updates via `apt`

## Download .deb from GitHub Release

Manueller Weg, wenn du eine konkrete Version direkt installieren willst.

```bash
VERSION="0.2.1"
curl -fsSLO "https://github.com/<owner>/<repo>/releases/download/v${VERSION}/resguard_${VERSION}_amd64.deb"
sudo apt install -y "./resguard_${VERSION}_amd64.deb"
```

Optional: SHA256 prüfen

```bash
VERSION="0.2.1"
curl -fsSLO "https://github.com/<owner>/<repo>/releases/download/v${VERSION}/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

Upgrade auf neue Versionen erfolgt erneut per Download + Installation.

## Install via APT repository

Empfohlen für Systeme, die Resguard kontinuierlich über `apt` aktualisieren sollen.

```bash
curl -fsSL "https://<owner>.github.io/<repo>/pubkey.gpg" \
  | gpg --dearmor \
  | sudo tee /usr/share/keyrings/resguard-archive-keyring.gpg >/dev/null

echo "deb [arch=amd64 signed-by=/usr/share/keyrings/resguard-archive-keyring.gpg] https://<owner>.github.io/<repo> stable main" \
  | sudo tee /etc/apt/sources.list.d/resguard.list >/dev/null

sudo apt update
sudo apt install -y resguard
```

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
