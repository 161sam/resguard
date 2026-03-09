# Prompt D1:
• Implemented a real split Debian packaging flow for optional daemon installation.

  Root cause

  - The old build path always used one package metadata file (Package: resguard) and only toggled extra payload files, so the “daemon variant” was still
    package resguard. Installing it could not produce a distinct resguard-daemon package state.

  What I changed

  - Split Debian metadata/scripts into two real packages:
      - Core:
          - control
          - postinst
          - prerm
      - Daemon:
          - control
          - postinst
          - prerm
  - Removed legacy single-package files:
      - old control (deleted)
      - old postinst (deleted)
      - old prerm (deleted)
  - Reworked packaging build script:
      - scripts/build-deb.sh
      - Supports RESGUARD_DEB_PACKAGE=core|daemon.
      - Builds:
          - resguard_<version>_amd64.deb
          - resguard-daemon_<version>_amd64.deb
      - Enforces version consistency across:
          - CLI crate
          - daemon crate
          - core control
          - daemon control
      - resguard-daemon package now:
          - depends on resguard (= <same-version>)
          - installs /usr/bin/resguardd
          - installs templates under /usr/share/resguard-daemon/...
          - postinst copies unit/config into /etc safely (config only if missing)
          - does not auto-enable/start daemon
  - Updated release/publish automation:
      - release-upload.yml
      - apt-pages.yml
      - release.sh
      - Now build/publish the real daemon package name and validate both control versions.
  - Updated operator docs:
      - docs/install.md
      - README.md
      - docs/releases.md
      - docs/releases/v0.4.0.md
      - packaging/README.md
      - Added explicit validation steps for daemon package and systemctl cat resguardd.service.

  Validation run

  - cargo build passed.
  - cargo build --features tui passed.
  - cargo test --workspace --all-targets passed.
  - RESGUARD_DEB_PACKAGE=core ./scripts/build-deb.sh passed.
  - RESGUARD_DEB_PACKAGE=daemon ./scripts/build-deb.sh passed.
  - ./scripts/release.sh --version 0.4.0 --dry-run passed.
  - Built package metadata confirms split:
      - dpkg-deb -I resguard_0.4.0_amd64.deb => Package: resguard
      - dpkg-deb -I resguard-daemon_0.4.0_amd64.deb => Package: resguard-daemon, Depends: resguard (= 0.4.0)
  - Payload checks confirm:
      - core package contains resguard + docs/completions/man, no resguardd
      - daemon package contains resguardd + daemon templates.

  Suggested commit message: fix(packaging): split optional daemon into a real separate deb package

  Note: there are unrelated pre-existing worktree changes in Feature-Roadmap.md and PROMPTS.md that I did not modify.


# Codex Prompt D2 — Release/APT Workflow an das echte Daemon-Paket anpassen

• Aligned release assets, APT generation, and install docs with the real resguard / resguard-daemon split.

  What I changed

  - Release asset workflow now builds/verifies correct package names:
      - release-upload.yml
      - outputs:
          - resguard_<version>_amd64.deb
          - resguard-daemon_<version>_amd64.deb
      - adds dpkg-deb -I checks for package identity.
  - APT pages workflow now validates split package presence in Packages index:
      - apt-pages.yml
      - new check confirms both Package: resguard and Package: resguard-daemon.
  - APT repo generator fixed to include both real package filename patterns:
      - generate-apt-repo.sh
      - key fix: include resguard-daemon_*_<arch>.deb and avoid stale legacy suffix matching.
  - Docs updated for split-package install behavior and verification commands:
      - README.md
      - docs/install.md
      - docs/releases.md
      - includes requested verification commands:
          - apt policy resguard
          - apt policy resguard-daemon
          - resguardd --help
          - systemctl status resguardd --no-pager

  Validation performed

  - RESGUARD_DEB_PACKAGE=core ./scripts/build-deb.sh passed.
  - RESGUARD_DEB_PACKAGE=daemon ./scripts/build-deb.sh passed.
  - ./scripts/generate-apt-repo.sh ... passed.
  - Verified generated APT Packages contains both entries:
      - Package: resguard
      - Package: resguard-daemon
  - cargo test --workspace --all-targets passed.
  - cargo build --features tui passed.

  Suggested commit message: chore(packaging): align release and apt publishing with separate daemon package


## Prompt C2 — `resguard run` als Durchbruch-Feature ausbauen

• Implemented resguard run as a service-driven, developer-first flow with conservative autodetect and clearer diagnostics.

  Changed files:

  - crates/resguard-services/src/run_service.rs
  - crates/resguard-services/src/lib.rs
  - crates/resguard-cli/src/commands/run.rs
  - crates/resguard-cli/src/commands/rescue.rs
  - crates/resguard-cli/src/cli.rs
  - crates/resguard-cli/src/legacy.rs
  - README.md
  - docs/cli.md

  What was improved:

  - run now supports both:
      - explicit mode: resguard run --class <class> <cmd...>
      - conservative autodetect mode: resguard run <cmd...> (only when confidence is strong)
  - Resolution source is explicit in output (explicit_class, profile, autodetect), along with selected class and slice.
  - Missing-slice handling now gives actionable setup/apply guidance.
  - Argument handling remains direct argv, no shell interpolation.
  - Legacy path delegates to the modular command flow.

  Tests added/updated:

  - explicit class run path
  - profile-backed resolution
  - missing slice guidance
  - autodetect strong match
  - autodetect ambiguous/weak rejection

  Validation:

  - cargo fmt --all passed
  - cargo test --workspace --all-targets passed

  Suggested commit message:

  - feat(run): make resguard run the flagship resource-isolated command launcher

---

## Prompt C3 — `resguard top` / operator visibility
