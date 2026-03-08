Prompt A — Snap/Firefox Feldtest-Erkenntnisse in konkrete Fixes übersetzen

• Implemented a minimal hardening fix for Firefox Snap wrap flow and validated it with tests.

  ### What I changed

  - Firefox Snap alias resolution made practical and deterministic:
      - firefox.desktop can resolve to firefox_firefox.desktop via snap alias path.
      - Duplicate alias hits across scan dirs are deduplicated by desktop ID (prefers user-origin entry).
      - File: main.rs
  - Wrapper launch behavior hardened for desktop environments:
      - Wrapped desktop entries now force DBusActivatable=false when source has that key, so launcher uses wrapper Exec=....
      - File: main.rs
      - Wrap command messaging updated accordingly.
      - File: desktop.rs
  - Tests added/updated for real-case behavior:
      - Firefox snap alias case (firefox.desktop -> firefox_firefox.desktop)
      - Wrapper enforces DBusActivatable=false
      - Existing snap ambiguity and mapping tests still pass
      - File: main.rs
  - User-facing docs updated for behavior change:
      - Added note that wrapper forces DBusActivatable=false for such sources.
      - File: cli.md
  - Field-test docs updated from vague gap to precise status:
      - Snap Firefox flow now marked as implemented in CLI, with explicit “live re-run pending” limitation.
      - Files:
          - 2026-03-08-ubuntu24.04-first-field.md
          - e2e_matrix.md

  ### Validation

  - Ran cargo test: all tests passed.

  ### Note

  - There is an untracked file present: tests/e2e/results/test-log.txt (left untouched).

  Suggested commit message:
  fix(desktop): harden snap firefox wrapper flow based on ubuntu field results

dev@getac:~/resguard$ git status
On branch main
Your branch is up to date with 'origin/main'.

Changes not staged for commit:
  (use "git add <file>..." to update what will be committed)
  (use "git restore <file>..." to discard changes in working directory)
        modified:   crates/resguard-cli/src/commands/desktop.rs
        modified:   crates/resguard-cli/src/main.rs
        modified:   docs/cli.md
        modified:   tests/e2e/e2e_matrix.md
        modified:   tests/e2e/results/2026-03-08-ubuntu24.04-first-field.md

Untracked files:
  (use "git add <file>..." to include in what will be committed)
        tests/e2e/results/test-log.txt

no changes added to commit (use "git add" and/or "git commit -a")
dev@getac:~/resguard$ git add .
dev@getac:~/resguard$ git commit -m "fix(desktop): harden snap firefox wrapper flow based on ubuntu field results"
[main 0d57d6b] fix(desktop): harden snap firefox wrapper flow based on ubuntu field results
 6 files changed, 371 insertions(+), 9 deletions(-)
 create mode 100644 tests/e2e/results/test-log.txt


Prompt B — Confidence-Scoring für Suggest auf echten Ubuntu-Daten nachschärfen

• Implemented confidence tuning for common Ubuntu snap desktop apps with low-risk guardrails.

  ### What changed

  - Added a new confidence signal: identity
      - Derived from strong app identity in scope/exec (snap.* scope, snap run <app>, or executable token).
      - Only applies when app identity matches a known app-to-class mapping:
          - firefox/chrome/... -> browsers
          - code/codium/idea/... -> ide
          - docker/podman/containerd -> heavy
  - Updated scoring:
      - pattern = +40
      - memory = +30
      - desktop-id = +30
      - identity = +30
      - capped at 100

  This makes common snap Firefox/Code cases move from 40 to 70 (pattern + identity), so they pass the default threshold while weak matches remain low.

  ### Files changed

  - suggest.rs
  - main.rs
  - v0.3.0-checklist.md

  ### Tests added/updated

  - Snap Firefox confidence boost to 70
  - Snap Code confidence boost to 70
  - Weak/ambiguous case remains low (40)
  - Existing confidence tests updated for new signal

  All tests pass with cargo test.

  Suggested commit message:
  feat(suggest): tune confidence scoring for common ubuntu snap desktop applications

dev@getac:~/resguard$ git status
On branch main
Your branch is up to date with 'origin/main'.

Changes not staged for commit:
  (use "git add <file>..." to update what will be committed)
  (use "git restore <file>..." to discard changes in working directory)
        modified:   crates/resguard-cli/src/commands/suggest.rs
        modified:   crates/resguard-cli/src/main.rs
        modified:   docs/releases/v0.3.0-checklist.md

no changes added to commit (use "git add" and/or "git commit -a")
dev@getac:~/resguard$ git add .
dev@getac:~/resguard$ git commit -m "feat(suggest): tune confidence scoring for common ubuntu snap desktop applications"
[main 9364f1d] feat(suggest): tune confidence scoring for common ubuntu snap desktop applications
 3 files changed, 107 insertions(+), 4 deletions(-)


Prompt C — Zweiten Feldtestlauf besser vorbereiten


• Implemented. The next field run now captures comparable GNOME/KDE data with minimal manual parsing.

  ### What changed

  - Added structured capture output in run_e2e.sh:
      - install_method (--install-method apt|release-asset|other)
      - session/desktop capture via CAPTURE desktop_environment=..., CAPTURE session_type=...
      - targeted snap checks:
          - desktop list --filter firefox + alias wrap probe firefox.desktop
          - desktop list --filter code + alias wrap probe code.desktop
      - non-snap check:
          - discovers first non-snap desktop entry and tests wrap dry-run
      - suggest capture:
          - dry-run status
          - total suggestions
          - count >= threshold
          - per-scope confidence hints for firefox/code if present
      - suggest apply outcome:
          - suggest --apply status (enabled by default, disable via --no-suggest-apply)
          - counts of ok/warn/skip/hint lines
  - New concise operator runbook: RUNBOOK.md
      - one practical command for repeatable runs
      - explains exactly which CAPTURE keys to compare
  - Updated matrix format and guidance: e2e_matrix.md
      - added Install column
      - execution examples now include --install-method
      - coverage section now includes snap/non-snap and suggest apply capture
      - pass criteria now requires capture keys

  ### Validation done

  - bash -n tests/e2e/run_e2e.sh passes
  - tests/e2e/run_e2e.sh --help shows new options correctly

  Suggested commit message:
  docs(e2e): improve field test capture for snap and desktop wrapper validation

dev@getac:~/resguard$ git status
On branch main
Your branch is up to date with 'origin/main'.

Changes not staged for commit:
  (use "git add <file>..." to update what will be committed)
  (use "git restore <file>..." to discard changes in working directory)
        modified:   tests/e2e/e2e_matrix.md
        modified:   tests/e2e/run_e2e.sh

Untracked files:
  (use "git add <file>..." to include in what will be committed)
        tests/e2e/RUNBOOK.md

no changes added to commit (use "git add" and/or "git commit -a")
dev@getac:~/resguard$ git add .
dev@getac:~/resguard$ git commit -m "docs(e2e): improve field test capture for snap and desktop wrapper validation"
[main c7883c5] docs(e2e): improve field test capture for snap and desktop wrapper validation
 3 files changed, 224 insertions(+), 18 deletions(-)
 create mode 100644 tests/e2e/RUNBOOK.md