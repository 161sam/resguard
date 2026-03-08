# E2E Field Runbook (GNOME/KDE)

Use this runbook to produce comparable Ubuntu/Kubuntu field results for v0.3 readiness.

## 1) Choose install method label

Set one of:

- `apt`
- `release-asset`
- `other`

## 2) Execute one run per host/session combo

Recommended command:

```bash
tests/e2e/run_e2e.sh \
  --profile e2e-field \
  --class rescue \
  --setup-profile \
  --install-method apt \
  --suggest-threshold 70 \
  --yes
```

If you only want capture without creating wrappers via suggest:

```bash
tests/e2e/run_e2e.sh --install-method apt --no-suggest-apply --yes
```

## 3) What is captured automatically

- install method label (`install_method`)
- desktop/session metadata (`desktop_environment`, `session_type`)
- snap app behavior:
  - `snap_firefox_list_*`, `snap_firefox_wrap_*`
  - `snap_code_list_*`, `snap_code_wrap_*`
- non-snap behavior:
  - `non_snap_desktop_id`, `non_snap_wrap`
- wrapper verification status (from `verify_desktop_wrap.sh`)
- rescue verification status (from `verify_rescue.sh`)
- suggest confidence/apply capture:
  - `suggest_total`
  - `suggest_confidence_ge_<threshold>`
  - `suggest_firefox_confidence`
  - `suggest_code_confidence`
  - `suggest_apply`, plus `ok/warn/skip/hint` line counts

All fields appear as `CAPTURE key=value` lines in the result markdown under `tests/e2e/results/`.

## 4) Matrix update rule

For each run, copy these items into `tests/e2e/e2e_matrix.md`:

- host/desktop/session
- install method
- result file path
- short notes from key `CAPTURE` lines (especially snap wrapper and suggest confidence/apply outcomes)
