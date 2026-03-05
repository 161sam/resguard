# Repo-Struktur (geplant)

    resguard/
    Cargo.toml                 # workspace
    README.md
    LICENSE
    SECURITY.md
    CHANGELOG.md

    crates/
        resguard-cli/             # bin: resguard
        Cargo.toml
        src/
            main.rs               # CLI wiring
            cli.rs                # clap definitions
            output.rs             # table/json/yaml output
            commands/
            mod.rs
            profile.rs
            apply.rs
            status.rs
            rollback.rs
            run.rs
            rules.rs
            rescue.rs
        resguard-core/            # lib: domain logic (profiles, planning, diff)
        Cargo.toml
        src/
            lib.rs
            model/                # Profile schema structs
            mod.rs
            profile.rs
            rule.rs
            plan/                 # "desired state" → "actions"
            mod.rs
            planner.rs
            actions.rs
            diff.rs
            validate/
            mod.rs
            checks.rs
        resguard-system/          # lib: Linux/systemd adapters
        Cargo.toml
        src/
            lib.rs
            systemd/
            mod.rs
            dropins.rs          # write drop-ins
            units.rs            # slice unit generation
            oomd.rs             # ManagedOOM* props
            reload.rs           # daemon-reload etc.
            proc/
            mod.rs
            pressure.rs         # /proc/pressure parsing
            meminfo.rs
            cgroup/
            mod.rs
            inspect.rs          # systemd-cgls parsing fallback
            exec/
            mod.rs
            runner.rs           # wrapper for systemctl/systemd-run (MVP)
        resguard-config/          # lib: load/save YAML, profile store
        Cargo.toml
        src/
            lib.rs
            store.rs              # /etc/resguard/profiles
            io.rs                 # serde_yaml
        resguard-state/           # lib: state + backup mgmt
        Cargo.toml
        src/
            lib.rs
            state.rs              # applied profile, timestamps
            backup.rs             # /var/lib/resguard/backups
            rollback.rs

    packaging/
        debian/                   # optional later
        systemd/                  # example units/templates (if any)

    docs/
        design.md
        profiles.md
        safety.md
        examples/
        workstation-16g.yml
        dev-docker-heavy.yml

    tests/
        fixtures/
        sample_profiles/
        integration/