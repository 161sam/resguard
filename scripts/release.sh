#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_FILE="$ROOT_DIR/packaging/deb/control"

usage() {
  cat <<'USAGE'
Usage:
  scripts/release.sh --version <x.y.z> [--dry-run] [--with-daemon]

Options:
  --version <x.y.z>   Target release version (e.g. 0.3.0)
  --dry-run           Print actions without modifying files
  --with-daemon       Build DEB with daemon included (RESGUARD_DEB_WITH_DAEMON=1)
  -h, --help          Show this help
USAGE
}

VERSION=""
DRY_RUN=0
WITH_DAEMON=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --with-daemon)
      WITH_DAEMON=1
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

if [[ -z "$VERSION" ]]; then
  echo "error: --version is required" >&2
  usage >&2
  exit 2
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: invalid version '$VERSION' (expected x.y.z)" >&2
  exit 2
fi

if [[ ! -d "$ROOT_DIR/.git" ]]; then
  echo "error: must run inside git repository" >&2
  exit 1
fi

if [[ $DRY_RUN -eq 0 ]]; then
  if ! git -C "$ROOT_DIR" diff --quiet || ! git -C "$ROOT_DIR" diff --cached --quiet; then
    echo "error: git worktree is not clean (commit/stash changes first)" >&2
    exit 1
  fi
fi

CARGO_TOMLS=(
  "$ROOT_DIR/crates/resguard-cli/Cargo.toml"
  "$ROOT_DIR/crates/resguard-daemon/Cargo.toml"
  "$ROOT_DIR/crates/resguard-core/Cargo.toml"
  "$ROOT_DIR/crates/resguard-system/Cargo.toml"
  "$ROOT_DIR/crates/resguard-config/Cargo.toml"
  "$ROOT_DIR/crates/resguard-state/Cargo.toml"
)

current_version="$(awk -F' = ' '/^version = / {gsub(/"/, "", $2); print $2; exit}' "${CARGO_TOMLS[0]}")"
if [[ -z "$current_version" ]]; then
  echo "error: failed to detect current version" >&2
  exit 1
fi

echo "release version: $current_version -> $VERSION"
echo "dry-run: $DRY_RUN"

action() {
  if [[ $DRY_RUN -eq 1 ]]; then
    echo "[dry-run] $*"
  else
    eval "$@"
  fi
}

for file in "${CARGO_TOMLS[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "error: missing file $file" >&2
    exit 1
  fi
  action "sed -i -E '0,/^version = \"[^\"]+\"/s//version = \"$VERSION\"/' '$file'"
done

action "sed -i -E 's/^Version: .*/Version: $VERSION/' '$CONTROL_FILE'"

echo "running build checks and packaging"
if [[ $WITH_DAEMON -eq 1 ]]; then
  action "RESGUARD_DEB_WITH_DAEMON=1 '$ROOT_DIR/scripts/build-deb.sh'"
else
  action "RESGUARD_DEB_WITH_DAEMON=0 '$ROOT_DIR/scripts/build-deb.sh'"
fi

ARTIFACT="resguard_${VERSION}_amd64.deb"
echo "expected artifact: $ROOT_DIR/$ARTIFACT"

echo
echo "next tag commands:"
echo "  git add crates/*/Cargo.toml packaging/deb/control"
echo "  git commit -m 'chore(release): cut v$VERSION'"
echo "  git tag -a v$VERSION -m 'resguard v$VERSION'"
echo "  git push origin <branch> --follow-tags"
