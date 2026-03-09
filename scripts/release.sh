#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_FILE="$ROOT_DIR/packaging/deb/control"
RELEASE_ASSETS_DIR="$ROOT_DIR/release-assets"

usage() {
  cat <<'USAGE'
Usage:
  scripts/release.sh --version <x.y.z> [--dry-run]

Options:
  --version <x.y.z>   Target release version (e.g. 0.2.2)
  --dry-run           Print actions without modifying files
  --with-daemon       Deprecated (ignored): both release artifacts are always built
  -h, --help          Show this help
USAGE
}

VERSION=""
DRY_RUN=0
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
      echo "warn: --with-daemon is deprecated and ignored (both artifacts are always built)"
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
  "$ROOT_DIR/crates/resguard-model/Cargo.toml"
  "$ROOT_DIR/crates/resguard-policy/Cargo.toml"
  "$ROOT_DIR/crates/resguard-discovery/Cargo.toml"
  "$ROOT_DIR/crates/resguard-runtime/Cargo.toml"
  "$ROOT_DIR/crates/resguard-services/Cargo.toml"
  "$ROOT_DIR/crates/resguard-core/Cargo.toml"
  "$ROOT_DIR/crates/resguard-config/Cargo.toml"
  "$ROOT_DIR/crates/resguard-state/Cargo.toml"
  "$ROOT_DIR/crates/resguard-system/Cargo.toml"
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
    /bin/bash -lc "$*"
  fi
}

stage_release_assets() {
  local cli_artifact_path="$1"
  local daemon_artifact_path="$2"
  local cli_artifact_name
  local daemon_artifact_name
  cli_artifact_name="$(basename "$cli_artifact_path")"
  daemon_artifact_name="$(basename "$daemon_artifact_path")"
  local staged_cli_artifact="$RELEASE_ASSETS_DIR/$cli_artifact_name"
  local staged_daemon_artifact="$RELEASE_ASSETS_DIR/$daemon_artifact_name"
  local sums_file="$RELEASE_ASSETS_DIR/SHA256SUMS"

  mkdir -p "$RELEASE_ASSETS_DIR"

  if [[ $DRY_RUN -eq 1 ]]; then
    if [[ -f "$cli_artifact_path" ]]; then
      cp -f "$cli_artifact_path" "$staged_cli_artifact"
      echo "[dry-run] staged existing artifact: $staged_cli_artifact"
    else
      printf "dry-run placeholder for %s\n" "$cli_artifact_name" > "$staged_cli_artifact"
      echo "[dry-run] staged placeholder artifact: $staged_cli_artifact"
    fi
    if [[ -f "$daemon_artifact_path" ]]; then
      cp -f "$daemon_artifact_path" "$staged_daemon_artifact"
      echo "[dry-run] staged existing artifact: $staged_daemon_artifact"
    else
      printf "dry-run placeholder for %s\n" "$daemon_artifact_name" > "$staged_daemon_artifact"
      echo "[dry-run] staged placeholder artifact: $staged_daemon_artifact"
    fi
  else
    if [[ ! -f "$cli_artifact_path" ]]; then
      echo "error: expected artifact not found: $cli_artifact_path" >&2
      exit 1
    fi
    if [[ ! -f "$daemon_artifact_path" ]]; then
      echo "error: expected artifact not found: $daemon_artifact_path" >&2
      exit 1
    fi
    cp -f "$cli_artifact_path" "$staged_cli_artifact"
    cp -f "$daemon_artifact_path" "$staged_daemon_artifact"
  fi

  (
    cd "$RELEASE_ASSETS_DIR"
    sha256sum \
      "$cli_artifact_name" \
      "$daemon_artifact_name" > "$sums_file"
  )
  echo "release-assets staged:"
  echo "  - $staged_cli_artifact"
  echo "  - $staged_daemon_artifact"
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
action "RESGUARD_DEB_WITH_DAEMON=0 '$ROOT_DIR/scripts/build-deb.sh'"
action "RESGUARD_DEB_WITH_DAEMON=1 '$ROOT_DIR/scripts/build-deb.sh'"
action "mv -f '$ROOT_DIR/resguard_${VERSION}_amd64.deb' '$ROOT_DIR/resguard_${VERSION}_amd64_daemon.deb'"
action "RESGUARD_DEB_WITH_DAEMON=0 '$ROOT_DIR/scripts/build-deb.sh'"

CLI_ARTIFACT="resguard_${VERSION}_amd64.deb"
DAEMON_ARTIFACT="resguard_${VERSION}_amd64_daemon.deb"
CLI_ARTIFACT_PATH="$ROOT_DIR/$CLI_ARTIFACT"
DAEMON_ARTIFACT_PATH="$ROOT_DIR/$DAEMON_ARTIFACT"
echo "expected artifacts:"
echo "  - $CLI_ARTIFACT_PATH"
echo "  - $DAEMON_ARTIFACT_PATH"

echo "staging release assets"
stage_release_assets "$CLI_ARTIFACT_PATH" "$DAEMON_ARTIFACT_PATH"

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
echo "  [ ] automatic upload by workflow .github/workflows/release-upload.yml"
echo "      expected uploaded assets:"
echo "      - $CLI_ARTIFACT"
echo "      - $DAEMON_ARTIFACT"
echo "      - SHA256SUMS"
