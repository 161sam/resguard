#!/usr/bin/env bash
set -u
set -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

PROFILE="${PROFILE:-e2e-field}"
CLASS="${CLASS:-heavy}"
SLICE="${SLICE:-}"
MAX_LATENCY_MS="${MAX_LATENCY_MS:-2000}"
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

while [ "$#" -gt 0 ]; do
  case "$1" in
    --help|-h)
      cat <<'EOF'
Usage: tests/e2e/verify_rescue.sh [options]

Options:
  --profile <name>                Profile name (default: e2e-field)
  --class <name>                  Class used for resguard run (default: heavy)
  --slice <unit.slice>            Explicit slice override (optional)
  --max-latency-ms <n>            Max allowed quick-command latency (default: 2000)
  --open-terminal                 Try opening terminal+htop in the selected class
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
    --slice)
      SLICE="$2"
      shift 2
      ;;
    --max-latency-ms)
      MAX_LATENCY_MS="$2"
      shift 2
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

RG_BIN="${RG_BIN:-}"
if [ -z "${RG_BIN}" ]; then
  if command -v resguard >/dev/null 2>&1; then
    RG_BIN="$(command -v resguard)"
  elif [ -x "${REPO_ROOT}/target/debug/resguard" ]; then
    RG_BIN="${REPO_ROOT}/target/debug/resguard"
  else
    echo "FAIL resguard binary not found (set RG_BIN or build/install resguard)" >&2
    exit 1
  fi
fi

run_cmd=("${RG_BIN}" run --class "${CLASS}" --profile "${PROFILE}")
if [ -n "${SLICE}" ]; then
  run_cmd+=(--slice "${SLICE}")
fi
run_cmd+=(--wait --)

expected_slice="${SLICE:-resguard-${CLASS}.slice}"
info "expected slice: ${expected_slice}"

start_ms="$(date +%s%3N)"
if "${run_cmd[@]}" /usr/bin/env true >/dev/null 2>&1; then
  end_ms="$(date +%s%3N)"
  elapsed_ms="$((end_ms - start_ms))"
  if [ "${elapsed_ms}" -le "${MAX_LATENCY_MS}" ]; then
    pass "within ${MAX_LATENCY_MS}ms command prompt appears (${elapsed_ms}ms)"
  else
    fail "command start latency ${elapsed_ms}ms exceeds ${MAX_LATENCY_MS}ms"
  fi
else
  fail "quick command failed via resguard run"
fi

cgroup_out="$("${run_cmd[@]}" /bin/cat /proc/self/cgroup 2>/dev/null || true)"
if printf "%s\n" "${cgroup_out}" | grep -Fq "/${expected_slice}/"; then
  pass "resguard run started in expected slice (${expected_slice})"
else
  fail "slice verification failed; expected /${expected_slice}/ in /proc/self/cgroup"
fi

if command -v htop >/dev/null 2>&1; then
  if "${run_cmd[@]}" htop --help >/dev/null 2>&1; then
    pass "htop starts via resguard run"
  else
    fail "htop did not start via resguard run"
  fi
elif command -v top >/dev/null 2>&1; then
  if "${run_cmd[@]}" top -bn1 >/dev/null 2>&1; then
    pass "top fallback starts via resguard run"
  else
    fail "top fallback failed via resguard run"
  fi
else
  fail "neither htop nor top found on host"
fi

if "${run_cmd[@]}" /bin/sh -c "sleep 30 & p=\$!; kill -9 \"\$p\"; wait \"\$p\"; [ \"\$?\" -eq 137 ]" >/dev/null 2>&1; then
  pass "kill -9 works in rescue path"
else
  fail "kill -9 verification failed"
fi

if [ "${OPEN_TERMINAL}" -eq 1 ]; then
  if command -v htop >/dev/null 2>&1; then
    if command -v gnome-terminal >/dev/null 2>&1; then
      "${RG_BIN}" run --class "${CLASS}" --profile "${PROFILE}" -- gnome-terminal -- htop >/dev/null 2>&1 &
      pass "opened gnome-terminal with htop via resguard run"
    elif command -v konsole >/dev/null 2>&1; then
      "${RG_BIN}" run --class "${CLASS}" --profile "${PROFILE}" -- konsole -e htop >/dev/null 2>&1 &
      pass "opened konsole with htop via resguard run"
    elif command -v xterm >/dev/null 2>&1; then
      "${RG_BIN}" run --class "${CLASS}" --profile "${PROFILE}" -- xterm -e htop >/dev/null 2>&1 &
      pass "opened xterm with htop via resguard run"
    else
      fail "no supported terminal emulator found for --open-terminal"
    fi
  else
    fail "--open-terminal requested but htop is not installed"
  fi
fi

echo "SUMMARY pass=${PASS_COUNT} fail=${FAIL_COUNT}"
if [ "${FAIL_COUNT}" -gt 0 ]; then
  exit 1
fi
