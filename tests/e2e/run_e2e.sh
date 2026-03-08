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
INSTALL_METHOD="${INSTALL_METHOD:-unknown}"
SUGGEST_THRESHOLD="${SUGGEST_THRESHOLD:-70}"
RUN_SUGGEST_APPLY="${RUN_SUGGEST_APPLY:-1}"

PASS_COUNT=0
FAIL_COUNT=0

usage() {
  cat <<'USAGE'
Usage: tests/e2e/run_e2e.sh [options]

Options:
  --profile <name>      Profile name for checks (default: e2e-field)
  --class <name>        Class for rescue/desktop-wrap checks (default: rescue)
  --setup-profile       Optional: run init/apply for the profile
  --install-method <m>  Install method label for capture (apt|release-asset|other)
  --suggest-threshold <n>  Confidence threshold for suggest checks (default: 70)
  --no-suggest-apply    Skip suggest --apply step (capture-only dry-run)
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

capture_kv() {
  echo "CAPTURE $1=$2"
}

desktop_case_capture() {
  local label="$1"
  local filter="$2"
  local alias_id="${3:-}"
  local list_out="/tmp/resguard_desktop_${label}.out"
  local list_err="/tmp/resguard_desktop_${label}.err"
  local wrap_out="/tmp/resguard_wrap_${label}.out"
  local wrap_err="/tmp/resguard_wrap_${label}.err"

  if "$RG_BIN" desktop list --origin all --filter "$filter" >"$list_out" 2>"$list_err"; then
    pass "desktop list filter=${filter}"
  else
    fail "desktop list filter=${filter}"
    capture_kv "${label}_list_status" "fail"
    return 1
  fi

  local discovered_id
  discovered_id="$(awk 'NR>1 && $1 ~ /\.desktop$/ {print $1; exit}' "$list_out")"
  local discovered_count
  discovered_count="$(awk 'NR>1 && $1 ~ /\.desktop$/ {c++} END {print c+0}' "$list_out")"
  capture_kv "${label}_list_status" "ok"
  capture_kv "${label}_list_count" "${discovered_count}"
  capture_kv "${label}_first_desktop_id" "${discovered_id:-none}"

  if [ -n "$alias_id" ]; then
    if "$RG_BIN" desktop wrap "$alias_id" --class "$CLASS" --dry-run >"$wrap_out" 2>"$wrap_err"; then
      pass "desktop wrap alias ${alias_id} (dry-run)"
      capture_kv "${label}_wrap_alias" "ok:${alias_id}"
    else
      fail "desktop wrap alias ${alias_id} (dry-run)"
      capture_kv "${label}_wrap_alias" "fail:${alias_id}"
    fi
  fi

  if [ -n "$discovered_id" ]; then
    if "$RG_BIN" desktop wrap "$discovered_id" --class "$CLASS" --dry-run >"$wrap_out" 2>"$wrap_err"; then
      pass "desktop wrap discovered ${label} entry (dry-run)"
      capture_kv "${label}_wrap_discovered" "ok:${discovered_id}"
    else
      fail "desktop wrap discovered ${label} entry (dry-run)"
      capture_kv "${label}_wrap_discovered" "fail:${discovered_id}"
    fi
  else
    capture_kv "${label}_wrap_discovered" "none"
  fi
}

suggest_capture() {
  local out="/tmp/resguard_suggest_e2e.out"
  local err="/tmp/resguard_suggest_e2e.err"
  local apply_out="/tmp/resguard_suggest_apply_e2e.out"
  local apply_err="/tmp/resguard_suggest_apply_e2e.err"

  local args=(suggest --dry-run --confidence-threshold "$SUGGEST_THRESHOLD")
  if [ -f "/etc/resguard/profiles/${PROFILE}.yml" ]; then
    args+=(--profile "$PROFILE")
  fi
  if "$RG_BIN" "${args[@]}" >"$out" 2>"$err"; then
    pass "suggest dry-run"
    capture_kv "suggest_dry_run" "ok"
  else
    fail "suggest dry-run"
    capture_kv "suggest_dry_run" "fail"
  fi

  local line_re='^[^[:space:]]+\.scope[[:space:]]+[^[:space:]]+[[:space:]]+[0-9]+[[:space:]]'
  local total high firefox code
  total="$(awk -v re="$line_re" '$0 ~ re {c++} END{print c+0}' "$out")"
  high="$(awk -v re="$line_re" -v t="$SUGGEST_THRESHOLD" '$0 ~ re && $3+0 >= t {c++} END{print c+0}' "$out")"
  firefox="$(awk -v re="$line_re" '$0 ~ re && tolower($1) ~ /firefox/ {print $3; exit}' "$out")"
  code="$(awk -v re="$line_re" '$0 ~ re && tolower($1) ~ /code/ {print $3; exit}' "$out")"
  capture_kv "suggest_total" "$total"
  capture_kv "suggest_confidence_ge_${SUGGEST_THRESHOLD}" "$high"
  capture_kv "suggest_firefox_confidence" "${firefox:-none}"
  capture_kv "suggest_code_confidence" "${code:-none}"

  if [ "$RUN_SUGGEST_APPLY" -eq 1 ]; then
    local apply_args=(suggest --apply --confidence-threshold "$SUGGEST_THRESHOLD")
    if [ -f "/etc/resguard/profiles/${PROFILE}.yml" ]; then
      apply_args+=(--profile "$PROFILE")
    fi
    if "$RG_BIN" "${apply_args[@]}" >"$apply_out" 2>"$apply_err"; then
      pass "suggest apply"
      capture_kv "suggest_apply" "ok"
    else
      fail "suggest apply"
      capture_kv "suggest_apply" "fail"
    fi
    capture_kv "suggest_apply_ok_lines" "$(awk -F'\t' '$1=="ok"{c++} END{print c+0}' "$apply_out")"
    capture_kv "suggest_apply_warn_lines" "$(awk -F'\t' '$1=="warn"{c++} END{print c+0}' "$apply_out")"
    capture_kv "suggest_apply_skip_lines" "$(awk -F'\t' '$1=="skip"{c++} END{print c+0}' "$apply_out")"
    capture_kv "suggest_apply_hint_lines" "$(awk -F'\t' '$1=="hint"{c++} END{print c+0}' "$apply_out")"
  else
    info "skipping suggest apply (--no-suggest-apply)"
    capture_kv "suggest_apply" "skipped"
  fi
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
    --install-method)
      INSTALL_METHOD="$2"
      shift 2
      ;;
    --suggest-threshold)
      SUGGEST_THRESHOLD="$2"
      shift 2
      ;;
    --no-suggest-apply)
      RUN_SUGGEST_APPLY=0
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
  echo "- install_method: ${INSTALL_METHOD}"
  echo "- suggest_threshold: ${SUGGEST_THRESHOLD}"
  echo "- suggest_apply_enabled: ${RUN_SUGGEST_APPLY}"
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
capture_kv "desktop_environment" "${XDG_CURRENT_DESKTOP:-unknown}"
capture_kv "session_type" "${XDG_SESSION_TYPE:-unknown}"

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

echo
info "snap/non-snap wrapper capture"
desktop_case_capture "snap_firefox" "firefox" "firefox.desktop"
desktop_case_capture "snap_code" "code" "code.desktop"

if "$RG_BIN" desktop list --origin all >/tmp/resguard_desktop_all.out 2>/tmp/resguard_desktop_all.err; then
  NON_SNAP_ID="$(awk 'NR>1 && $1 ~ /\.desktop$/ && $3 !~ /snap/i {print $1; exit}' /tmp/resguard_desktop_all.out)"
  capture_kv "non_snap_desktop_id" "${NON_SNAP_ID:-none}"
  if [ -n "${NON_SNAP_ID:-}" ]; then
    if "$RG_BIN" desktop wrap "$NON_SNAP_ID" --class "$CLASS" --dry-run >/tmp/resguard_wrap_non_snap.out 2>/tmp/resguard_wrap_non_snap.err; then
      pass "desktop wrap non-snap discovered entry (dry-run)"
      capture_kv "non_snap_wrap" "ok:${NON_SNAP_ID}"
    else
      fail "desktop wrap non-snap discovered entry (dry-run)"
      capture_kv "non_snap_wrap" "fail:${NON_SNAP_ID}"
    fi
  else
    capture_kv "non_snap_wrap" "none"
  fi
else
  capture_kv "non_snap_wrap" "list-failed"
fi

if RG_BIN="$RG_BIN" PROFILE="$PROFILE" CLASS="$CLASS" "$SCRIPT_DIR/verify_rescue.sh" --profile "$PROFILE" --class "$CLASS"; then
  pass "rescue verification"
else
  fail "rescue verification"
fi

echo
info "suggest capture"
suggest_capture

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
