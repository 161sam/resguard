#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_FILE="$ROOT_DIR/packaging/deb/control"
RELEASE_ASSETS_DIR="$ROOT_DIR/release-assets"

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

stage_release_assets() {
  local artifact_path="$1"
  local artifact_name
  artifact_name="$(basename "$artifact_path")"
  local staged_artifact="$RELEASE_ASSETS_DIR/$artifact_name"
  local sums_file="$RELEASE_ASSETS_DIR/SHA256SUMS"

  mkdir -p "$RELEASE_ASSETS_DIR"

  if [[ $DRY_RUN -eq 1 ]]; then
    if [[ -f "$artifact_path" ]]; then
      cp -f "$artifact_path" "$staged_artifact"
      echo "[dry-run] staged existing artifact: $staged_artifact"
    else
      printf "dry-run placeholder for %s\n" "$artifact_name" > "$staged_artifact"
      echo "[dry-run] staged placeholder artifact: $staged_artifact"
    fi
  else
    if [[ ! -f "$artifact_path" ]]; then
      echo "error: expected artifact not found: $artifact_path" >&2
      exit 1
    fi
    cp -f "$artifact_path" "$staged_artifact"
  fi

  (cd "$RELEASE_ASSETS_DIR" && sha256sum "$artifact_name" > "$sums_file")
  echo "release-assets staged:"
  echo "  - $staged_artifact"
  echo "  - $sums_file"
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
ARTIFACT_PATH="$ROOT_DIR/$ARTIFACT"
echo "expected artifact: $ARTIFACT_PATH"

echo "staging release assets"
stage_release_assets "$ARTIFACT_PATH"

echo
echo "next tag commands:"
echo "  git add crates/*/Cargo.toml packaging/deb/control"
echo "  git commit -m 'chore(release): cut v$VERSION'"
echo "  git tag -a v$VERSION -m 'resguard v$VERSION'"
echo "  git push origin <branch> --follow-tags"

echo
echo "github release checklist:"
echo "  [ ] changelog section reviewed: CHANGELOG.md"
echo "  [ ] release notes snippet: docs/releases/v$VERSION.md"
echo "  [ ] create tag: git tag -a v$VERSION -m 'resguard v$VERSION'"
echo "  [ ] push tag: git push origin v$VERSION"
echo "  [ ] upload assets:"
echo "      - release-assets/$ARTIFACT"
echo "      - release-assets/SHA256SUMS"
