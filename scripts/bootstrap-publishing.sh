#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

detect_repo_from_origin() {
  local origin_url
  origin_url="$(git -C "$ROOT_DIR" remote get-url origin 2>/dev/null || true)"
  if [[ "$origin_url" =~ github\.com[:/]([^/]+/[^/.]+)(\.git)?$ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return 0
  fi
  return 1
}

usage() {
  cat <<'USAGE'
Usage:
  scripts/bootstrap-publishing.sh [options]

Options:
  --repo <owner/repo>        Target GitHub repository (default: detect from git remote)
  --output-dir <path>        Directory for generated key files (default: /tmp/resguard-publish-XXXXXX)
  --private-key-file <path>  Use existing armored private key file (skip key generation)
  --public-key-file <path>   Use existing public key (armored or binary) when private key is provided
  --key-name <text>          GPG key real name (default: Resguard APT Repository Signing)
  --key-email <email>        GPG key email (default: release@resguard.local)
  --expire <period>          GPG expiry (default: 3y)
  --skip-gh                  Only generate key files, skip GitHub API/secret setup
  -h, --help                 Show this help

Behavior:
  - Generates a dedicated repository signing key (non-interactive).
  - Exports:
      * RESGUARD_APT_GPG_PRIVATE_KEY.asc
      * RESGUARD_APT_GPG_PUBLIC_KEY.asc
      * pubkey.gpg
  - If gh auth is valid (unless --skip-gh):
      * sets secret RESGUARD_APT_GPG_PRIVATE_KEY
      * attempts to configure GitHub Pages for workflow deployment
USAGE
}

REPO=""
OUTPUT_DIR=""
KEY_NAME="Resguard APT Repository Signing"
KEY_EMAIL="release@resguard.local"
KEY_EXPIRE="3y"
SKIP_GH=0
PRIVATE_KEY_FILE=""
PUBLIC_KEY_FILE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      REPO="${2:-}"
      shift 2
      ;;
    --output-dir)
      OUTPUT_DIR="${2:-}"
      shift 2
      ;;
    --key-name)
      KEY_NAME="${2:-}"
      shift 2
      ;;
    --private-key-file)
      PRIVATE_KEY_FILE="${2:-}"
      shift 2
      ;;
    --public-key-file)
      PUBLIC_KEY_FILE="${2:-}"
      shift 2
      ;;
    --key-email)
      KEY_EMAIL="${2:-}"
      shift 2
      ;;
    --expire)
      KEY_EXPIRE="${2:-}"
      shift 2
      ;;
    --skip-gh)
      SKIP_GH=1
      shift
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

if [[ -z "$REPO" ]]; then
  REPO="$(detect_repo_from_origin || true)"
fi
if [[ -z "$REPO" ]]; then
  echo "error: could not detect --repo (expected owner/repo)" >&2
  exit 2
fi

if ! command -v gpg >/dev/null 2>&1; then
  echo "error: gpg is required" >&2
  exit 1
fi

if [[ -z "$OUTPUT_DIR" ]]; then
  OUTPUT_DIR="$(mktemp -d /tmp/resguard-publish-XXXXXX)"
else
  mkdir -p "$OUTPUT_DIR"
fi

PRIVATE_ASC="$OUTPUT_DIR/RESGUARD_APT_GPG_PRIVATE_KEY.asc"
PUBLIC_ASC="$OUTPUT_DIR/RESGUARD_APT_GPG_PUBLIC_KEY.asc"
PUBLIC_GPG="$OUTPUT_DIR/pubkey.gpg"

if [[ -n "$PRIVATE_KEY_FILE" ]]; then
  if [[ ! -f "$PRIVATE_KEY_FILE" ]]; then
    echo "error: --private-key-file not found: $PRIVATE_KEY_FILE" >&2
    exit 2
  fi
  cp -f "$PRIVATE_KEY_FILE" "$PRIVATE_ASC"
  if [[ -n "$PUBLIC_KEY_FILE" ]]; then
    if [[ ! -f "$PUBLIC_KEY_FILE" ]]; then
      echo "error: --public-key-file not found: $PUBLIC_KEY_FILE" >&2
      exit 2
    fi
    cp -f "$PUBLIC_KEY_FILE" "$PUBLIC_ASC"
  fi
else
  GNUPGHOME="$(mktemp -d /tmp/resguard-gnupg-XXXXXX)"
  chmod 700 "$GNUPGHOME"
  trap 'rm -rf "$GNUPGHOME"' EXIT

  KEY_BATCH="$OUTPUT_DIR/keygen.batch"
  cat >"$KEY_BATCH" <<EOF
%no-protection
Key-Type: RSA
Key-Length: 4096
Name-Real: $KEY_NAME
Name-Email: $KEY_EMAIL
Expire-Date: $KEY_EXPIRE
%commit
EOF

  if ! gpg --batch --homedir "$GNUPGHOME" --pinentry-mode loopback --generate-key "$KEY_BATCH"; then
    echo "error: gpg key generation failed in this environment." >&2
    echo "hint: rerun on a workstation with functional gpg-agent, or provide existing keys via:" >&2
    echo "  --private-key-file <path> --public-key-file <path>" >&2
    exit 1
  fi

  KEY_ID="$(gpg --batch --homedir "$GNUPGHOME" --list-secret-keys --with-colons | awk -F: '/^sec:/ {print $5; exit}')"
  FINGERPRINT="$(gpg --batch --homedir "$GNUPGHOME" --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')"

  if [[ -z "$KEY_ID" || -z "$FINGERPRINT" ]]; then
    echo "error: failed to resolve generated key id/fingerprint" >&2
    exit 1
  fi

  gpg --batch --homedir "$GNUPGHOME" --armor --export-secret-keys "$KEY_ID" > "$PRIVATE_ASC"
  gpg --batch --homedir "$GNUPGHOME" --armor --export "$KEY_ID" > "$PUBLIC_ASC"
  gpg --batch --homedir "$GNUPGHOME" --export "$KEY_ID" > "$PUBLIC_GPG"
fi

KEY_ID="(imported)"
FINGERPRINT="(imported)"
if [[ -n "${GNUPGHOME:-}" ]]; then
  KEY_ID="$(gpg --batch --homedir "$GNUPGHOME" --list-secret-keys --with-colons | awk -F: '/^sec:/ {print $5; exit}')"
  FINGERPRINT="$(gpg --batch --homedir "$GNUPGHOME" --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')"
fi

if [[ ! -f "$PUBLIC_ASC" ]]; then
  TMP_IMPORT_HOME="$(mktemp -d /tmp/resguard-pub-XXXXXX)"
  chmod 700 "$TMP_IMPORT_HOME"
  trap 'rm -rf "${GNUPGHOME:-}" "$TMP_IMPORT_HOME"' EXIT
  gpg --batch --homedir "$TMP_IMPORT_HOME" --import "$PRIVATE_ASC" >/dev/null 2>&1 || true
  if gpg --batch --homedir "$TMP_IMPORT_HOME" --list-secret-keys --with-colons >/dev/null 2>&1; then
    IMPORT_KEY_ID="$(gpg --batch --homedir "$TMP_IMPORT_HOME" --list-secret-keys --with-colons | awk -F: '/^sec:/ {print $5; exit}')"
    if [[ -n "$IMPORT_KEY_ID" ]]; then
      gpg --batch --homedir "$TMP_IMPORT_HOME" --armor --export "$IMPORT_KEY_ID" > "$PUBLIC_ASC"
      gpg --batch --homedir "$TMP_IMPORT_HOME" --export "$IMPORT_KEY_ID" > "$PUBLIC_GPG"
    fi
  fi
fi

echo "generated signing key:"
echo "  repo: $REPO"
echo "  key_id: $KEY_ID"
echo "  fingerprint: $FINGERPRINT"
echo "  output_dir: $OUTPUT_DIR"
echo "  private_key: $PRIVATE_ASC"
echo "  public_key_armored: $PUBLIC_ASC"
echo "  pubkey_gpg: $PUBLIC_GPG"

if [[ $SKIP_GH -eq 1 ]]; then
  echo
  echo "gh setup skipped (--skip-gh). Run manually:"
  echo "  gh auth login -h github.com"
  echo "  gh secret set RESGUARD_APT_GPG_PRIVATE_KEY --repo $REPO < \"$PRIVATE_ASC\""
  echo "  gh api --method POST repos/$REPO/pages -f build_type=workflow || true"
  exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
  echo
  echo "gh CLI not found. Remaining one-time setup:"
  echo "  gh auth login -h github.com"
  echo "  gh secret set RESGUARD_APT_GPG_PRIVATE_KEY --repo $REPO < \"$PRIVATE_ASC\""
  echo "  gh api --method POST repos/$REPO/pages -f build_type=workflow || true"
  exit 0
fi

if ! gh auth status >/dev/null 2>&1; then
  echo
  echo "gh auth is not available in this environment."
  echo "remaining one-time setup:"
  echo "  gh auth login -h github.com"
  echo "  gh secret set RESGUARD_APT_GPG_PRIVATE_KEY --repo $REPO < \"$PRIVATE_ASC\""
  echo "  gh api --method POST repos/$REPO/pages -f build_type=workflow || true"
  exit 0
fi

gh repo view "$REPO" --json name >/dev/null
gh secret set RESGUARD_APT_GPG_PRIVATE_KEY --repo "$REPO" < "$PRIVATE_ASC"

if gh api "repos/$REPO/pages" >/dev/null 2>&1; then
  gh api --method PUT "repos/$REPO/pages" -f build_type=workflow >/dev/null 2>&1 || true
else
  gh api --method POST "repos/$REPO/pages" -f build_type=workflow >/dev/null 2>&1 || true
fi

echo
echo "github setup complete:"
echo "  - secret set: RESGUARD_APT_GPG_PRIVATE_KEY"
echo "  - pages configuration: best-effort set to workflow mode"
echo "next:"
echo "  - commit and push release/tag workflow changes"
echo "  - tag release: git tag -a vX.Y.Z -m 'resguard vX.Y.Z' && git push origin vX.Y.Z"
