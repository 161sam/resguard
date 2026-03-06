# Releases

This document describes the release cut flow for Resguard.

## Scope and Version Gates

- `v0.3.0`: suggestion/classification release line
- `v0.4.0`: TUI + daemon release line
- `resguardd` remains optional in packaging
- `tui` remains feature-gated

## Scripted Release Flow

Primary workflow:

```bash
./scripts/release.sh --version <x.y.z> --dry-run
./scripts/release.sh --version <x.y.z> [--with-daemon]
```

What the script does:

1. checks clean git tree (non-dry-run)
2. bumps versions in all crate `Cargo.toml` files
3. updates `packaging/deb/control` version
4. builds Debian artifact via `scripts/build-deb.sh`
5. prints tag/push commands

## v0.3.0 Cut Plan (suggest-focused)

1. Update changelog sections for `v0.3.0`.
2. Run release script in dry-run mode:

```bash
./scripts/release.sh --version 0.3.0 --dry-run
```

3. Execute release:

```bash
./scripts/release.sh --version 0.3.0
```

4. Run validation:

```bash
cargo build
cargo test -- --test-threads=1
```

5. Tag and push using commands printed by the script.

## v0.4.0 Cut Plan (tui + daemon)

1. Update changelog sections for `v0.4.0` (include TUI + daemon notes).
2. Run release script in dry-run mode:

```bash
./scripts/release.sh --version 0.4.0 --dry-run --with-daemon
```

3. Execute release with daemon artifact:

```bash
./scripts/release.sh --version 0.4.0 --with-daemon
```

4. Run validation:

```bash
cargo build --features tui
cargo build -p resguard-daemon
cargo test -- --test-threads=1
```

5. Tag and push using commands printed by the script.

## Verification Checklist

- `scripts/release.sh --dry-run` prints expected actions.
- `scripts/build-deb.sh` produces `resguard_<version>_amd64.deb`.
- `packaging/deb/control` version matches crate versions.
