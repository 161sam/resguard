#!/usr/bin/env bash
set -u
set -o pipefail

LOAD_PIDS=()
LOAD_DESC=""

run_ignored_load_tests() {
  local repo_root="$1"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "FAIL cargo not found; cannot run ignored load tests" >&2
    return 1
  fi
  (
    cd "${repo_root}" &&
      cargo test -p resguard --test load -- --ignored --nocapture
  )
}

start_background_load() {
  local duration_s="$1"
  local ncpu
  ncpu="$(nproc 2>/dev/null || echo 2)"
  if [ "${ncpu}" -lt 1 ]; then
    ncpu=1
  fi

  LOAD_PIDS=()
  LOAD_DESC=""

  if command -v stress-ng >/dev/null 2>&1; then
    stress-ng \
      --cpu "${ncpu}" \
      --vm 1 \
      --vm-bytes 35% \
      --timeout "${duration_s}s" \
      >/tmp/resguard-e2e-stress.log 2>&1 &
    LOAD_PIDS+=("$!")
    LOAD_DESC="stress-ng"
    return 0
  fi

  local i
  i=0
  while [ "${i}" -lt "${ncpu}" ]; do
    (
      while :; do
        :
      done
    ) >/dev/null 2>&1 &
    LOAD_PIDS+=("$!")
    i=$((i + 1))
  done
  LOAD_DESC="cpu busy-loop fallback"

  (
    sleep "${duration_s}"
    stop_background_load
  ) >/dev/null 2>&1 &

  return 0
}

stop_background_load() {
  local pid
  for pid in "${LOAD_PIDS[@]:-}"; do
    kill "${pid}" >/dev/null 2>&1 || true
  done
  for pid in "${LOAD_PIDS[@]:-}"; do
    wait "${pid}" >/dev/null 2>&1 || true
  done
}

