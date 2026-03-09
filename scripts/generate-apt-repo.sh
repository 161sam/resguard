#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

REPO_DIR="$ROOT_DIR/apt"
INPUT_DIR="$ROOT_DIR/release-assets"
DIST="stable"
COMPONENT="main"
ARCH="amd64"
ORIGIN="Resguard"
LABEL="Resguard APT Repository"
DESCRIPTION="Resguard GitHub Pages APT repository"
SIGN_KEY=""
GPG_HOMEDIR=""
GPG_PASSPHRASE_ENV=""
PUBKEY_PATH="$REPO_DIR/pubkey.gpg"

usage() {
  cat <<'USAGE'
Usage:
  scripts/generate-apt-repo.sh [options]

Options:
  --repo-dir <path>             Repository output directory (default: ./apt)
  --input-dir <path>            Directory containing .deb files (default: ./release-assets)
  --distribution <name>         Distribution name (default: stable)
  --component <name>            Component name (default: main)
  --arch <arch>                 Architecture (default: amd64)
  --origin <text>               Release Origin field
  --label <text>                Release Label field
  --description <text>          Release Description field
  --sign-key <gpg-key-id>       GPG key id/fingerprint for Release signing
  --gpg-homedir <path>          GPG home directory
  --gpg-passphrase-env <name>   Env var containing GPG passphrase (optional)
  --export-pubkey <path>        Export public key to this path (default: apt/pubkey.gpg)
  -h, --help                    Show this help

Examples:
  scripts/generate-apt-repo.sh
  scripts/generate-apt-repo.sh --sign-key ABCDEF1234567890
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-dir)
      REPO_DIR="$2"
      shift 2
      ;;
    --input-dir)
      INPUT_DIR="$2"
      shift 2
      ;;
    --distribution)
      DIST="$2"
      shift 2
      ;;
    --component)
      COMPONENT="$2"
      shift 2
      ;;
    --arch)
      ARCH="$2"
      shift 2
      ;;
    --origin)
      ORIGIN="$2"
      shift 2
      ;;
    --label)
      LABEL="$2"
      shift 2
      ;;
    --description)
      DESCRIPTION="$2"
      shift 2
      ;;
    --sign-key)
      SIGN_KEY="$2"
      shift 2
      ;;
    --gpg-homedir)
      GPG_HOMEDIR="$2"
      shift 2
      ;;
    --gpg-passphrase-env)
      GPG_PASSPHRASE_ENV="$2"
      shift 2
      ;;
    --export-pubkey)
      PUBKEY_PATH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! command -v dpkg-scanpackages >/dev/null 2>&1 && ! command -v apt-ftparchive >/dev/null 2>&1; then
  echo "error: need dpkg-scanpackages or apt-ftparchive to generate Packages index" >&2
  exit 1
fi

if ! command -v apt-ftparchive >/dev/null 2>&1; then
  echo "error: apt-ftparchive is required to generate dists/$DIST/Release" >&2
  exit 1
fi

if [[ -n "$SIGN_KEY" ]] && ! command -v gpg >/dev/null 2>&1; then
  echo "error: gpg is required for signing" >&2
  exit 1
fi

POOL_DIR="$REPO_DIR/pool/$COMPONENT/r/resguard"
BINARY_DIR="$REPO_DIR/dists/$DIST/$COMPONENT/binary-$ARCH"
DIST_DIR="$REPO_DIR/dists/$DIST"

mkdir -p "$POOL_DIR" "$BINARY_DIR"
find "$POOL_DIR" -maxdepth 1 -type f -name '*.deb' -delete

mapfile -t DEBS < <(
  find "$INPUT_DIR" -maxdepth 1 -type f \( -name "resguard_*_${ARCH}.deb" -o -name "resguard-daemon_*_${ARCH}.deb" \) | sort
)
if [[ ${#DEBS[@]} -eq 0 ]]; then
  echo "error: no .deb artifacts found in $INPUT_DIR (expected resguard_*_${ARCH}.deb and/or resguard-daemon_*_${ARCH}.deb)" >&2
  exit 1
fi

for deb in "${DEBS[@]}"; do
  cp -f "$deb" "$POOL_DIR/"
done

pushd "$REPO_DIR" >/dev/null
PACKAGES_REL="dists/$DIST/$COMPONENT/binary-$ARCH/Packages"
if command -v dpkg-scanpackages >/dev/null 2>&1; then
  dpkg-scanpackages --arch "$ARCH" "pool/$COMPONENT/r/resguard" > "$PACKAGES_REL"
else
  apt-ftparchive packages "pool/$COMPONENT/r/resguard" > "$PACKAGES_REL"
fi
gzip -n -9 -c "$PACKAGES_REL" > "${PACKAGES_REL}.gz"

cat > ".apt-ftparchive.conf" <<EOF
APT::FTPArchive::Release::Origin "${ORIGIN}";
APT::FTPArchive::Release::Label "${LABEL}";
APT::FTPArchive::Release::Suite "${DIST}";
APT::FTPArchive::Release::Codename "${DIST}";
APT::FTPArchive::Release::Architectures "${ARCH}";
APT::FTPArchive::Release::Components "${COMPONENT}";
APT::FTPArchive::Release::Description "${DESCRIPTION}";
EOF

apt-ftparchive -c ".apt-ftparchive.conf" release "dists/$DIST" > "dists/$DIST/Release"
rm -f ".apt-ftparchive.conf"
popd >/dev/null

if [[ -n "$SIGN_KEY" ]]; then
  GPG_CMD=(gpg --batch --yes)
  if [[ -n "$GPG_HOMEDIR" ]]; then
    GPG_CMD+=(--homedir "$GPG_HOMEDIR")
  fi
  if [[ -n "$GPG_PASSPHRASE_ENV" ]]; then
    if [[ -z "${!GPG_PASSPHRASE_ENV:-}" ]]; then
      echo "error: passphrase env var '$GPG_PASSPHRASE_ENV' is empty" >&2
      exit 1
    fi
    GPG_CMD+=(--pinentry-mode loopback --passphrase "${!GPG_PASSPHRASE_ENV}")
  fi

  "${GPG_CMD[@]}" \
    --local-user "$SIGN_KEY" \
    --output "$DIST_DIR/Release.gpg" \
    --detach-sign "$DIST_DIR/Release"
  "${GPG_CMD[@]}" \
    --local-user "$SIGN_KEY" \
    --output "$DIST_DIR/InRelease" \
    --clearsign "$DIST_DIR/Release"

  mkdir -p "$(dirname "$PUBKEY_PATH")"
  "${GPG_CMD[@]}" --output "$PUBKEY_PATH" --export "$SIGN_KEY"
else
  rm -f "$DIST_DIR/Release.gpg" "$DIST_DIR/InRelease"
fi

if [[ ! -s "$BINARY_DIR/Packages" ]]; then
  echo "error: generated Packages index is empty" >&2
  exit 1
fi
gzip -t "$BINARY_DIR/Packages.gz"
if ! grep -q "main/binary-$ARCH/Packages" "$DIST_DIR/Release"; then
  echo "error: Release file does not reference Packages index for $ARCH" >&2
  exit 1
fi
if [[ -n "$SIGN_KEY" ]]; then
  GPG_VERIFY=(gpg --batch --yes)
  if [[ -n "$GPG_HOMEDIR" ]]; then
    GPG_VERIFY+=(--homedir "$GPG_HOMEDIR")
  fi
  "${GPG_VERIFY[@]}" --verify "$DIST_DIR/Release.gpg" "$DIST_DIR/Release" >/dev/null 2>&1
  "${GPG_VERIFY[@]}" --verify "$DIST_DIR/InRelease" >/dev/null 2>&1
fi

echo "apt repository generated at: $REPO_DIR"
echo "distribution: $DIST"
echo "component: $COMPONENT"
echo "arch: $ARCH"
echo "packages copied:"
printf '  - %s\n' "${DEBS[@]##*/}"
if [[ -n "$SIGN_KEY" ]]; then
  echo "signed metadata:"
  echo "  - $DIST_DIR/Release.gpg"
  echo "  - $DIST_DIR/InRelease"
  echo "exported key:"
  echo "  - $PUBKEY_PATH"
else
  echo "signing skipped (use --sign-key to create Release.gpg and InRelease)"
fi
