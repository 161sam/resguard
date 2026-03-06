#!/usr/bin/env bash
set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
RESULTS_DIR="${SCRIPT_DIR}/results"

PROFILE="${PROFILE:-e2e-field}"
CLASS="${CLASS:-rescue}"
SETUP_PROFILE=0
AUTO_YES="${AUTO_YES:-0}"

PASS_COUNT=0
FAIL_COUNT=0

usage() {
  cat <<'USAGE'
Usage: tests/e2e/run_e2e.sh [options]

Options:
  --profile <name>      Profile name for checks (default: e2e-field)
  --class <name>        Class for rescue/desktop-wrap checks (default: rescue)
  --setup-profile       Optional: run init/apply for the profile
  --yes                 Non-interactive mode (skip confirmation prompt)
  -h, --help            Show help
USAGE
}

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

run_checked() {
  local label="$1"
  shift
  if "$@"; then
    pass "$label"
  else
    fail "$label"
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --profile)
      PROFILE="$2"
      shift 2
      ;;
    --class)
      CLASS="$2"
      shift 2
      ;;
    --setup-profile)
      SETUP_PROFILE=1
      shift
      ;;
    --yes)
      AUTO_YES=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
done

RG_BIN="${RG_BIN:-}"
if [ -z "$RG_BIN" ]; then
  if command -v resguard >/dev/null 2>&1; then
    RG_BIN="$(command -v resguard)"
  elif [ -x "${REPO_ROOT}/target/debug/resguard" ]; then
    RG_BIN="${REPO_ROOT}/target/debug/resguard"
  elif [ -x "${REPO_ROOT}/target/release/resguard" ]; then
    RG_BIN="${REPO_ROOT}/target/release/resguard"
  fi
fi

if [ -z "$RG_BIN" ] || [ ! -x "$RG_BIN" ]; then
  echo "FAIL resguard binary not found (set RG_BIN or build/install first)" >&2
  exit 1
fi

mkdir -p "$RESULTS_DIR"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
RESULT_FILE="${RESULTS_DIR}/${TS}.md"

{
  echo "# Resguard E2E Field Result"
  echo
  echo "- timestamp_utc: ${TS}"
  echo "- host: $(hostname 2>/dev/null || echo unknown)"
  echo "- profile: ${PROFILE}"
  echo "- class: ${CLASS}"
  echo "- resguard_bin: ${RG_BIN}"
  echo
  echo "## Log"
  echo
  echo '```text'
} > "$RESULT_FILE"

exec > >(tee -a "$RESULT_FILE") 2>&1

info "results file: ${RESULT_FILE}"
info "repo root: ${REPO_ROOT}"

if [ "${AUTO_YES}" -ne 1 ]; then
  echo "This will run host checks and may run profile apply if --setup-profile is set."
  read -r -p "Continue? [y/N] " yn
  case "$yn" in
    y|Y|yes|YES) ;;
    *)
      echo "Aborted"
      echo '```' >> "$RESULT_FILE"
      exit 1
      ;;
  esac
fi

info "collecting system info"
run_checked "Linux host" test "$(uname -s)" = "Linux"
run_checked "systemctl available" command -v systemctl >/dev/null 2>&1
run_checked "systemd-run available" command -v systemd-run >/dev/null 2>&1

echo "OS:"
if [ -f /etc/os-release ]; then
  sed -n '1,8p' /etc/os-release || true
fi

echo "Desktop/session:"
echo "XDG_CURRENT_DESKTOP=${XDG_CURRENT_DESKTOP:-unknown}"
echo "XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unknown}"
echo "XDG_SESSION_DESKTOP=${XDG_SESSION_DESKTOP:-unknown}"
echo "Kernel=$(uname -r)"
grep -E '^MemTotal:' /proc/meminfo || true

echo
echo "manager checks:"
run_checked "system manager reachable" systemctl --no-pager --version >/dev/null 2>&1
run_checked "system manager state query" systemctl is-system-running >/dev/null 2>&1
if systemctl --user is-system-running >/dev/null 2>&1; then
  pass "user manager state query"
else
  fail "user manager state query"
fi

if [ "$SETUP_PROFILE" -eq 1 ]; then
  info "optional profile setup enabled"
  if [ "$(id -u)" -eq 0 ]; then
    run_checked "init profile ${PROFILE}" "$RG_BIN" init --name "$PROFILE" --out "/etc/resguard/profiles/${PROFILE}.yml"
    run_checked "apply profile ${PROFILE}" "$RG_BIN" apply "$PROFILE" --user-daemon-reload
  else
    run_checked "init profile ${PROFILE} (sudo)" sudo "$RG_BIN" init --name "$PROFILE" --out "/etc/resguard/profiles/${PROFILE}.yml"
    run_checked "apply profile ${PROFILE} (sudo)" sudo "$RG_BIN" apply "$PROFILE" --user-daemon-reload
  fi
else
  info "skipping profile setup (use --setup-profile to enable)"
fi

if RG_BIN="$RG_BIN" CLASS="$CLASS" "$SCRIPT_DIR/verify_desktop_wrap.sh" --class "$CLASS"; then
  pass "desktop wrap verification"
else
  fail "desktop wrap verification"
fi

if RG_BIN="$RG_BIN" PROFILE="$PROFILE" CLASS="$CLASS" "$SCRIPT_DIR/verify_rescue.sh" --profile "$PROFILE" --class "$CLASS"; then
  pass "rescue verification"
else
  fail "rescue verification"
fi

SUGGEST_ARGS=(suggest --dry-run)
if [ -f "/etc/resguard/profiles/${PROFILE}.yml" ]; then
  SUGGEST_ARGS+=(--profile "$PROFILE")
fi
if "$RG_BIN" "${SUGGEST_ARGS[@]}" >/tmp/resguard_suggest_e2e.out 2>/tmp/resguard_suggest_e2e.err; then
  pass "suggest dry-run"
else
  fail "suggest dry-run"
fi

echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
echo '```'
echo
echo "## Summary"
echo
echo "- pass: ${PASS_COUNT}"
echo "- fail: ${FAIL_COUNT}"

if [ "$FAIL_COUNT" -gt 0 ]; then
  exit 1
fi
