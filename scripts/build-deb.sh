#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTROL_FILE="$ROOT_DIR/packaging/deb/control"
POSTINST_FILE="$ROOT_DIR/packaging/deb/postinst"
PRERM_FILE="$ROOT_DIR/packaging/deb/prerm"
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

PKG_NAME="$(awk -F': ' '/^Package:/ {print $2}' "$CONTROL_FILE")"
VERSION="$(awk -F': ' '/^Version:/ {print $2}' "$CONTROL_FILE")"
ARCH="$(awk -F': ' '/^Architecture:/ {print $2}' "$CONTROL_FILE")"

if [[ -z "$PKG_NAME" || -z "$VERSION" || -z "$ARCH" ]]; then
  echo "error: invalid control file metadata" >&2
  exit 1
fi

ARTIFACT_NAME="${PKG_NAME}_${VERSION}_${ARCH}.deb"
STAGE_DIR="$ROOT_DIR/packaging/deb/.build/${PKG_NAME}_${VERSION}_${ARCH}"
INCLUDE_DAEMON="${RESGUARD_DEB_WITH_DAEMON:-1}"

rm -rf "$STAGE_DIR"
install -d -m 0755 "$STAGE_DIR/DEBIAN"
install -d -m 0755 "$STAGE_DIR/usr/bin"
install -d -m 0755 "$STAGE_DIR/etc/resguard/profiles"
install -d -m 0755 "$STAGE_DIR/var/lib/resguard"
install -d -m 0755 "$STAGE_DIR/usr/share/resguard/systemd"
install -d -m 0755 "$STAGE_DIR/usr/share/doc/resguard"
install -d -m 0755 "$STAGE_DIR/usr/share/man/man1"

cargo build --release -p resguard --manifest-path "$ROOT_DIR/Cargo.toml"

install -m 0755 "$ROOT_DIR/target/release/resguard" "$STAGE_DIR/usr/bin/resguard"
install -m 0644 "$CONTROL_FILE" "$STAGE_DIR/DEBIAN/control"
install -m 0755 "$POSTINST_FILE" "$STAGE_DIR/DEBIAN/postinst"
install -m 0755 "$PRERM_FILE" "$STAGE_DIR/DEBIAN/prerm"
install -m 0644 "$ROOT_DIR/README.md" "$STAGE_DIR/usr/share/doc/resguard/README.md"
install -m 0644 "$ROOT_DIR/CHANGELOG.md" "$STAGE_DIR/usr/share/doc/resguard/CHANGELOG.md"
install -m 0644 "$ROOT_DIR/docs/man/resguard.1" "$STAGE_DIR/usr/share/man/man1/resguard.1"

if [[ "$INCLUDE_DAEMON" == "1" ]]; then
  cargo build --release -p resguard-daemon --manifest-path "$ROOT_DIR/Cargo.toml"
  install -m 0755 "$ROOT_DIR/target/release/resguardd" "$STAGE_DIR/usr/bin/resguardd"
  install -m 0644 "$RESGUARDD_CONFIG_TEMPLATE" "$STAGE_DIR/usr/share/resguard/resguardd.yml"
  install -m 0644 "$RESGUARDD_SERVICE_TEMPLATE" "$STAGE_DIR/usr/share/resguard/systemd/resguardd.service"
fi

rm -f "$ROOT_DIR/$ARTIFACT_NAME"
if dpkg-deb --help 2>/dev/null | grep -q -- "--root-owner-group"; then
  dpkg-deb --root-owner-group --build "$STAGE_DIR" "$ROOT_DIR/$ARTIFACT_NAME"
else
  dpkg-deb --build "$STAGE_DIR" "$ROOT_DIR/$ARTIFACT_NAME"
fi

echo "built: $ROOT_DIR/$ARTIFACT_NAME"
