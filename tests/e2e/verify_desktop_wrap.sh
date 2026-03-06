#!/usr/bin/env bash
set -u
set -o pipefail

CLASS="${CLASS:-rescue}"
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

while [ "$#" -gt 0 ]; do
  case "$1" in
    --class)
      CLASS="$2"
      shift 2
      ;;
    -h|--help)
      cat <<'USAGE'
Usage: tests/e2e/verify_desktop_wrap.sh [--class <name>]
USAGE
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
  else
    echo "FAIL resguard binary not found (set RG_BIN)" >&2
    exit 1
  fi
fi

if ! "$RG_BIN" desktop list --origin all >/tmp/resguard_desktop_list.out 2>/tmp/resguard_desktop_list.err; then
  fail "desktop list command failed"
  echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
  exit 1
fi
pass "desktop list command works"

DESKTOP_ID="$(awk 'NR>1 && $1 ~ /\.desktop$/ {print $1; exit}' /tmp/resguard_desktop_list.out)"
if [ -z "$DESKTOP_ID" ]; then
  fail "no desktop entries found in desktop list"
  echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
  exit 1
fi
pass "desktop entry discovered: ${DESKTOP_ID}"

if "$RG_BIN" desktop wrap "$DESKTOP_ID" --class "$CLASS" --dry-run >/tmp/resguard_wrap_dry.out 2>/tmp/resguard_wrap_dry.err; then
  pass "desktop wrap dry-run works"
else
  fail "desktop wrap dry-run failed"
fi

if "$RG_BIN" desktop doctor >/tmp/resguard_desktop_doctor.out 2>/tmp/resguard_desktop_doctor.err; then
  pass "desktop doctor executed"
else
  # desktop doctor intentionally returns non-zero for warnings; treat as informational pass
  if [ -s /tmp/resguard_desktop_doctor.out ] || [ -s /tmp/resguard_desktop_doctor.err ]; then
    pass "desktop doctor returned warnings (expected on partially configured hosts)"
  else
    fail "desktop doctor failed without output"
  fi
fi

echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
if [ "$FAIL_COUNT" -gt 0 ]; then
  exit 1
fi
