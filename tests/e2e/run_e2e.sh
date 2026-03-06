#!/usr/bin/env bash
set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# shellcheck source=tests/e2e/load_helpers.sh
source "${SCRIPT_DIR}/load_helpers.sh"

PROFILE="${PROFILE:-e2e-field}"
CLASS="${CLASS:-heavy}"
LOAD_SECONDS="${LOAD_SECONDS:-20}"
SKIP_IGNORED_LOAD_TESTS=0
AUTO_YES="${AUTO_YES:-0}"
OPEN_TERMINAL="${OPEN_TERMINAL:-0}"

PASS_COUNT=0
FAIL_COUNT=0

pass() {
  PASS_COUNT=$((PASS_COUNT + 1))
  echo "PASS $*"
}

fail() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  echo "FAIL $*"
}

info() {
  echo "INFO $*"
}

cleanup() {
  stop_background_load
}
trap cleanup EXIT

while [ "$#" -gt 0 ]; do
  case "$1" in
    --help|-h)
      cat <<'EOF'
Usage: tests/e2e/run_e2e.sh [options]

Options:
  --profile <name>                Profile name (default: e2e-field)
  --class <name>                  Workload class for rescue checks (default: heavy)
  --load-seconds <n>              Background load duration in seconds (default: 20)
  --skip-ignored-load-tests       Skip cargo ignored load tests
  --open-terminal                 Ask verify step to open htop terminal if possible
  --yes                           Non-interactive mode (no confirmation prompt)
EOF
      exit 0
      ;;
    --profile)
      PROFILE="$2"
      shift 2
      ;;
    --class)
      CLASS="$2"
      shift 2
      ;;
    --load-seconds)
      LOAD_SECONDS="$2"
      shift 2
      ;;
    --skip-ignored-load-tests)
      SKIP_IGNORED_LOAD_TESTS=1
      shift
      ;;
    --yes)
      AUTO_YES=1
      shift
      ;;
    --open-terminal)
      OPEN_TERMINAL=1
      shift
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

if [ "$(uname -s)" = "Linux" ]; then
  pass "linux host detected"
else
  fail "non-linux host detected"
fi

if command -v systemctl >/dev/null 2>&1; then
  pass "systemctl found"
else
  fail "systemctl missing"
fi

if command -v systemd-run >/dev/null 2>&1; then
  pass "systemd-run found"
else
  fail "systemd-run missing"
fi

if [ "${FAIL_COUNT}" -gt 0 ]; then
  echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
  exit 1
fi

RG_BIN=""
if command -v resguard >/dev/null 2>&1; then
  RG_BIN="$(command -v resguard)"
elif [ -x "${REPO_ROOT}/target/debug/resguard" ]; then
  RG_BIN="${REPO_ROOT}/target/debug/resguard"
elif command -v cargo >/dev/null 2>&1; then
  info "building resguard binary"
  if (cd "${REPO_ROOT}" && cargo build -q -p resguard); then
    RG_BIN="${REPO_ROOT}/target/debug/resguard"
  else
    fail "cargo build failed"
  fi
else
  fail "resguard binary not found and cargo unavailable"
fi

if [ -z "${RG_BIN}" ] || [ ! -x "${RG_BIN}" ]; then
  fail "resguard binary unavailable"
fi

if [ "${FAIL_COUNT}" -gt 0 ]; then
  echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
  exit 1
fi

info "using resguard: ${RG_BIN}"
info "profile=${PROFILE} class=${CLASS} load_seconds=${LOAD_SECONDS}"
info "matrix template: tests/e2e/e2e_matrix.md"

if [ "${AUTO_YES}" -ne 1 ]; then
  echo "This script applies profile '${PROFILE}' on the host and starts synthetic load."
  read -r -p "Continue? [y/N] " yn
  case "${yn}" in
    y|Y|yes|YES)
      ;;
    *)
      echo "Aborted."
      exit 1
      ;;
  esac
fi

if [ ! -f "/etc/resguard/profiles/${PROFILE}.yml" ]; then
  info "profile missing, creating /etc/resguard/profiles/${PROFILE}.yml"
  if [ "$(id -u)" -eq 0 ]; then
    if "${RG_BIN}" init --name "${PROFILE}" --out "/etc/resguard/profiles/${PROFILE}.yml"; then
      pass "created profile ${PROFILE}"
    else
      fail "failed to create profile ${PROFILE}"
    fi
  else
    if sudo "${RG_BIN}" init --name "${PROFILE}" --out "/etc/resguard/profiles/${PROFILE}.yml"; then
      pass "created profile ${PROFILE}"
    else
      fail "failed to create profile ${PROFILE} (sudo init)"
    fi
  fi
else
  pass "profile exists: /etc/resguard/profiles/${PROFILE}.yml"
fi

if [ "$(id -u)" -eq 0 ]; then
  if "${RG_BIN}" apply "${PROFILE}" --user-daemon-reload; then
    pass "applied profile ${PROFILE}"
  else
    fail "profile apply failed"
  fi
else
  if sudo "${RG_BIN}" apply "${PROFILE}" --user-daemon-reload; then
    pass "applied profile ${PROFILE}"
  else
    fail "profile apply failed (sudo apply)"
  fi
fi

if [ "${SKIP_IGNORED_LOAD_TESTS}" -eq 1 ]; then
  info "skipping ignored Rust load tests"
else
  if run_ignored_load_tests "${REPO_ROOT}"; then
    pass "ignored Rust load tests passed"
  else
    fail "ignored Rust load tests failed"
  fi
fi

if start_background_load "${LOAD_SECONDS}"; then
  pass "started background load (${LOAD_DESC})"
else
  fail "failed to start background load"
fi

if RG_BIN="${RG_BIN}" PROFILE="${PROFILE}" CLASS="${CLASS}" OPEN_TERMINAL="${OPEN_TERMINAL}" \
  "${SCRIPT_DIR}/verify_rescue.sh" --profile "${PROFILE}" --class "${CLASS}"; then
  pass "rescue verification passed"
else
  fail "rescue verification failed"
fi

stop_background_load
pass "background load stopped"

echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
if [ "${FAIL_COUNT}" -gt 0 ]; then
  exit 1
fi
