#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_CONTROL_FILE="$ROOT_DIR/packaging/deb/core/control"
DAEMON_CONTROL_FILE="$ROOT_DIR/packaging/deb/daemon/control"
RESGUARDD_CONFIG_TEMPLATE="$ROOT_DIR/packaging/etc/resguard/resguardd.yml"
RESGUARDD_SERVICE_TEMPLATE="$ROOT_DIR/packaging/systemd/resguardd.service"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "error: dpkg-deb is required" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required" >&2
  exit 1
fi

read_control_field() {
  local file="$1"
  local key="$2"
  awk -F': ' -v k="$key" '$1 == k {print $2; exit}' "$file"
}

PKG_KIND="${RESGUARD_DEB_PACKAGE:-}"
if [[ -z "$PKG_KIND" ]]; then
  case "${RESGUARD_DEB_WITH_DAEMON:-}" in
    0) PKG_KIND="core" ;;
    1) PKG_KIND="daemon" ;;
    "") PKG_KIND="core" ;;
    *)
      echo "error: invalid RESGUARD_DEB_WITH_DAEMON='${RESGUARD_DEB_WITH_DAEMON}' (expected 0 or 1)" >&2
      exit 2
      ;;
  esac
fi

if [[ "$PKG_KIND" != "core" && "$PKG_KIND" != "daemon" ]]; then
  echo "error: invalid RESGUARD_DEB_PACKAGE='$PKG_KIND' (expected core or daemon)" >&2
  exit 2
fi

CLI_CARGO_TOML="$ROOT_DIR/crates/resguard-cli/Cargo.toml"
DAEMON_CARGO_TOML="$ROOT_DIR/crates/resguard-daemon/Cargo.toml"
CLI_VERSION="$(awk -F' = ' '/^version = / {gsub(/"/, "", $2); print $2; exit}' "$CLI_CARGO_TOML")"
DAEMON_VERSION="$(awk -F' = ' '/^version = / {gsub(/"/, "", $2); print $2; exit}' "$DAEMON_CARGO_TOML")"
CORE_VERSION="$(read_control_field "$CORE_CONTROL_FILE" "Version")"
DAEMON_VERSION_CONTROL="$(read_control_field "$DAEMON_CONTROL_FILE" "Version")"

if [[ -z "$CLI_VERSION" || -z "$DAEMON_VERSION" || -z "$CORE_VERSION" || -z "$DAEMON_VERSION_CONTROL" ]]; then
  echo "error: failed to read versions from Cargo/control files" >&2
  exit 1
fi

if [[ "$CLI_VERSION" != "$DAEMON_VERSION" || "$CLI_VERSION" != "$CORE_VERSION" || "$CLI_VERSION" != "$DAEMON_VERSION_CONTROL" ]]; then
  echo "error: version mismatch detected" >&2
  echo "resguard-cli version: $CLI_VERSION" >&2
  echo "resguard-daemon version: $DAEMON_VERSION" >&2
  echo "core control version: $CORE_VERSION" >&2
  echo "daemon control version: $DAEMON_VERSION_CONTROL" >&2
  echo "fix: align crate and control versions before building" >&2
  exit 1
fi

if [[ "$PKG_KIND" == "core" ]]; then
  CONTROL_FILE="$CORE_CONTROL_FILE"
  POSTINST_FILE="$ROOT_DIR/packaging/deb/core/postinst"
  PRERM_FILE="$ROOT_DIR/packaging/deb/core/prerm"
else
  CONTROL_FILE="$DAEMON_CONTROL_FILE"
  POSTINST_FILE="$ROOT_DIR/packaging/deb/daemon/postinst"
  PRERM_FILE="$ROOT_DIR/packaging/deb/daemon/prerm"
fi

PKG_NAME="$(read_control_field "$CONTROL_FILE" "Package")"
VERSION="$(read_control_field "$CONTROL_FILE" "Version")"
ARCH="$(read_control_field "$CONTROL_FILE" "Architecture")"

if [[ -z "$PKG_NAME" || -z "$VERSION" || -z "$ARCH" ]]; then
  echo "error: invalid control file metadata in $CONTROL_FILE" >&2
  exit 1
fi

ARTIFACT_NAME="${PKG_NAME}_${VERSION}_${ARCH}.deb"
STAGE_DIR="$ROOT_DIR/packaging/deb/.build/${PKG_NAME}_${VERSION}_${ARCH}"

rm -rf "$STAGE_DIR"
install -d -m 0755 "$STAGE_DIR/DEBIAN"
install -m 0644 "$CONTROL_FILE" "$STAGE_DIR/DEBIAN/control"
install -m 0755 "$POSTINST_FILE" "$STAGE_DIR/DEBIAN/postinst"
install -m 0755 "$PRERM_FILE" "$STAGE_DIR/DEBIAN/prerm"

if [[ "$PKG_KIND" == "core" ]]; then
  install -d -m 0755 "$STAGE_DIR/usr/bin"
  install -d -m 0755 "$STAGE_DIR/etc/resguard/profiles"
  install -d -m 0755 "$STAGE_DIR/var/lib/resguard"
  install -d -m 0755 "$STAGE_DIR/usr/share/bash-completion/completions"
  install -d -m 0755 "$STAGE_DIR/usr/share/zsh/vendor-completions"
  install -d -m 0755 "$STAGE_DIR/usr/share/fish/vendor_completions.d"
  install -d -m 0755 "$STAGE_DIR/usr/share/doc/resguard"
  install -d -m 0755 "$STAGE_DIR/usr/share/man/man1"

  cargo build --release -p resguard --manifest-path "$ROOT_DIR/Cargo.toml"

  install -m 0755 "$ROOT_DIR/target/release/resguard" "$STAGE_DIR/usr/bin/resguard"
  install -m 0644 "$ROOT_DIR/README.md" "$STAGE_DIR/usr/share/doc/resguard/README.md"
  install -m 0644 "$ROOT_DIR/CHANGELOG.md" "$STAGE_DIR/usr/share/doc/resguard/CHANGELOG.md"
  install -m 0644 "$ROOT_DIR/docs/man/resguard.1" "$STAGE_DIR/usr/share/man/man1/resguard.1"
  "$ROOT_DIR/target/release/resguard" completion bash > "$STAGE_DIR/usr/share/bash-completion/completions/resguard"
  "$ROOT_DIR/target/release/resguard" completion zsh > "$STAGE_DIR/usr/share/zsh/vendor-completions/_resguard"
  "$ROOT_DIR/target/release/resguard" completion fish > "$STAGE_DIR/usr/share/fish/vendor_completions.d/resguard.fish"
  chmod 0644 "$STAGE_DIR/usr/share/bash-completion/completions/resguard"
  chmod 0644 "$STAGE_DIR/usr/share/zsh/vendor-completions/_resguard"
  chmod 0644 "$STAGE_DIR/usr/share/fish/vendor_completions.d/resguard.fish"
else
  install -d -m 0755 "$STAGE_DIR/usr/bin"
  install -d -m 0755 "$STAGE_DIR/usr/share/resguard-daemon/systemd"
  install -d -m 0755 "$STAGE_DIR/usr/share/doc/resguard-daemon"

  cargo build --release -p resguard-daemon --manifest-path "$ROOT_DIR/Cargo.toml"

  install -m 0755 "$ROOT_DIR/target/release/resguardd" "$STAGE_DIR/usr/bin/resguardd"
  install -m 0644 "$RESGUARDD_CONFIG_TEMPLATE" "$STAGE_DIR/usr/share/resguard-daemon/resguardd.yml"
  install -m 0644 "$RESGUARDD_SERVICE_TEMPLATE" "$STAGE_DIR/usr/share/resguard-daemon/systemd/resguardd.service"
  install -m 0644 "$ROOT_DIR/README.md" "$STAGE_DIR/usr/share/doc/resguard-daemon/README.md"
fi

rm -f "$ROOT_DIR/$ARTIFACT_NAME"
if dpkg-deb --help 2>/dev/null | grep -q -- "--root-owner-group"; then
  dpkg-deb --root-owner-group --build "$STAGE_DIR" "$ROOT_DIR/$ARTIFACT_NAME"
else
  dpkg-deb --build "$STAGE_DIR" "$ROOT_DIR/$ARTIFACT_NAME"
fi

echo "built: $ROOT_DIR/$ARTIFACT_NAME"
