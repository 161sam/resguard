use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use regex::Regex;
use resguard_config::{load_profile_from_store, profile_path, save_profile, validate_profile_file};
use resguard_core::profile::{
    Class, Cpu, Memory, Metadata, Oomd, Profile, Spec, SystemMemory, UserMemory,
};
use resguard_core::{build_apply_plan, validate_profile, Action, PlanOptions};
use resguard_state::{
    begin_transaction, manifest_from_transaction, read_backup_manifest, read_state,
    rollback_from_manifest, snapshot_before_write, state_from_manifest, write_backup_manifest,
    write_state,
};
use resguard_system::{
    cpu_count, default_reserve_bytes, ensure_dir, exec_command, is_root_user, read_mem_total_bytes,
    read_pressure_1min, systemctl_cat_unit, systemctl_is_active, systemctl_show_props, systemd_run,
    write_file,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::Duration;
use std::{collections::HashMap, fs};

#[derive(Parser, Debug)]
#[command(name = "resguard", about = "Linux resource guard using systemd slices")]
struct Cli {
    #[arg(long, global = true, default_value = "table")]
    format: String,
    #[arg(long, global = true)]
    verbose: bool,
    #[arg(long, global = true)]
    quiet: bool,
    #[arg(long, global = true)]
    no_color: bool,
    #[arg(long, global = true, default_value = "/")]
    root: String,
    #[arg(long, global = true, default_value = "/etc/resguard")]
    config_dir: String,
    #[arg(long, global = true, default_value = "/var/lib/resguard")]
    state_dir: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        out: Option<String>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Apply {
        profile: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_oomd: bool,
        #[arg(long)]
        no_cpu: bool,
        #[arg(long)]
        no_classes: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        user_daemon_reload: bool,
    },
    Diff {
        profile: String,
    },
    Rollback {
        #[arg(long)]
        last: bool,
        #[arg(long)]
        to: Option<String>,
    },
    Doctor,
    Metrics,
    Panic {
        #[arg(long, help = "Temporary panic duration like 30s, 10m, 1h")]
        duration: Option<String>,
    },
    Status,
    Run {
        #[arg(long)]
        class: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        slice: Option<String>,
        #[arg(long, help = "Skip slice existence check (unsafe, poweruser only)")]
        no_check: bool,
        #[arg(long)]
        wait: bool,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    Desktop {
        #[command(subcommand)]
        cmd: DesktopCmd,
    },
}

#[derive(Subcommand, Debug)]
enum ProfileCmd {
    List,
    Show {
        name: String,
    },
    Import {
        file: String,
    },
    Export {
        name: String,
        #[arg(long)]
        out: String,
    },
    Validate {
        target: String,
    },
    New {
        name: String,
        #[arg(long)]
        from: Option<String>,
    },
    Edit {
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum DesktopCmd {
    List {
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, value_enum, default_value_t = DesktopOrigin::All)]
        origin: DesktopOrigin,
    },
    Wrap {
        desktop_id: String,
        #[arg(long)]
        class: String,
    },
    Unwrap {
        desktop_id: String,
    },
    Doctor,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum DesktopOrigin {
    User,
    System,
    All,
}

#[derive(Debug)]
struct ApplyOptions {
    dry_run: bool,
    no_oomd: bool,
    no_cpu: bool,
    no_classes: bool,
    force: bool,
    user_daemon_reload: bool,
}

#[derive(Debug)]
struct RunRequest {
    class: String,
    profile_override: Option<String>,
    slice_override: Option<String>,
    no_check: bool,
    wait: bool,
    command: Vec<String>,
}

fn print_global_context(cli: &Cli) {
    println!(
        "format={} verbose={} quiet={} no_color={} root={} config_dir={} state_dir={}",
        cli.format, cli.verbose, cli.quiet, cli.no_color, cli.root, cli.config_dir, cli.state_dir
    );
}

fn format_bytes_binary(bytes: u64) -> String {
    let gb = 1024_u64.pow(3);
    let mb = 1024_u64.pow(2);
    if bytes >= gb {
        format!("{}G", bytes / gb)
    } else if bytes >= mb {
        format!("{}M", bytes / mb)
    } else {
        bytes.to_string()
    }
}

fn clamp(value: u64, min: u64, max: u64) -> u64 {
    value.max(min).min(max)
}

fn round_down_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    (value / step) * step
}

fn round_up_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    value.div_ceil(step) * step
}

fn reserve_rounding_step(reserve: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    let mib_256 = 256 * 1024_u64.pow(2);
    if reserve >= 2 * gb {
        gb
    } else {
        mib_256
    }
}

fn class_cap_percent(user_max: u64, pct: u64, hard_cap: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    let raw = user_max.saturating_mul(pct) / 100;
    let rounded = round_up_to_step(raw, gb);
    clamp(rounded, gb.min(user_max), hard_cap.min(user_max))
}

fn build_auto_profile(name: &str, total_mem_bytes: u64, cpu_cores: u32) -> Profile {
    let gb = 1024_u64.pow(3);
    let base_reserve = default_reserve_bytes(total_mem_bytes).min(total_mem_bytes);
    let reserve_step = reserve_rounding_step(base_reserve);
    let reserve = round_up_to_step(base_reserve, reserve_step).min(total_mem_bytes);

    let mut user_max = round_down_to_step(total_mem_bytes.saturating_sub(reserve), gb);
    if user_max == 0 {
        user_max = round_down_to_step(total_mem_bytes, 256 * 1024_u64.pow(2));
    }

    let high_margin = (user_max / 10).min(2 * gb);
    let mut user_high = round_down_to_step(user_max.saturating_sub(high_margin), gb);
    if user_high == 0 {
        user_high = user_max;
    }

    let (cpu, oomd) = if cpu_cores >= 4 {
        (
            Some(Cpu {
                enabled: Some(true),
                reserve_core_for_system: Some(true),
                system_allowed_cpus: Some("0".to_string()),
                user_allowed_cpus: Some(format!("1-{}", cpu_cores - 1)),
            }),
            Some(Oomd {
                enabled: Some(true),
                memory_pressure: Some("kill".to_string()),
                memory_pressure_limit: Some("60%".to_string()),
            }),
        )
    } else {
        (
            Some(Cpu {
                enabled: Some(false),
                reserve_core_for_system: Some(false),
                system_allowed_cpus: None,
                user_allowed_cpus: None,
            }),
            Some(Oomd {
                enabled: Some(true),
                memory_pressure: Some("kill".to_string()),
                memory_pressure_limit: Some("60%".to_string()),
            }),
        )
    };

    let browsers_max = class_cap_percent(user_max, 40, 6 * gb);
    let ide_max = class_cap_percent(user_max, 25, 4 * gb);
    let heavy_rest = user_max.saturating_sub(browsers_max.saturating_add(ide_max));
    let heavy_max = if heavy_rest == 0 {
        gb.min(user_max)
    } else {
        let rounded = round_down_to_step(heavy_rest, gb);
        if rounded == 0 {
            round_down_to_step(heavy_rest, 256 * 1024_u64.pow(2))
        } else {
            rounded
        }
    }
    .min(8 * gb)
    .min(user_max);

    let mut classes = BTreeMap::new();
    classes.insert(
        "browsers".to_string(),
        Class {
            slice_name: Some("resguard-browsers.slice".to_string()),
            memory_high: None,
            memory_max: Some(format_bytes_binary(browsers_max)),
            cpu_weight: Some(80),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("55%".to_string()),
        },
    );
    classes.insert(
        "ide".to_string(),
        Class {
            slice_name: Some("resguard-ide.slice".to_string()),
            memory_high: None,
            memory_max: Some(format_bytes_binary(ide_max)),
            cpu_weight: Some(70),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("60%".to_string()),
        },
    );
    classes.insert(
        "heavy".to_string(),
        Class {
            slice_name: Some("resguard-heavy.slice".to_string()),
            memory_high: None,
            memory_max: Some(format_bytes_binary(heavy_max)),
            cpu_weight: Some(90),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("50%".to_string()),
        },
    );

    Profile {
        api_version: "resguard.io/v1".to_string(),
        kind: "Profile".to_string(),
        metadata: Metadata {
            name: name.to_string(),
        },
        spec: Spec {
            memory: Some(Memory {
                system: Some(SystemMemory {
                    memory_low: Some(format_bytes_binary(reserve)),
                }),
                user: Some(UserMemory {
                    memory_high: Some(format_bytes_binary(user_high)),
                    memory_max: Some(format_bytes_binary(user_max)),
                }),
            }),
            cpu,
            oomd,
            classes,
            slices: None,
        },
    }
}

fn resolve_with_root(root: &str, path: PathBuf) -> Result<PathBuf> {
    if root == "/" {
        return Ok(path);
    }

    let root_path = Path::new(root);
    if !root_path.is_absolute() {
        return Err(anyhow!("--root must be an absolute path"));
    }

    if path.is_absolute() {
        let rel = path
            .strip_prefix("/")
            .context("failed to strip leading slash")?;
        Ok(root_path.join(rel))
    } else {
        Ok(path)
    }
}

fn execute_action(action: &Action) -> Result<()> {
    match action {
        Action::EnsureDir { path } => {
            ensure_dir(path)?;
            Ok(())
        }
        Action::WriteFile { path, content } => {
            write_file(path, content)?;
            Ok(())
        }
        Action::Exec {
            program,
            args,
            env,
            best_effort,
        } => match exec_command(program, args, env) {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else if *best_effort {
                    eprintln!(
                        "warn: best-effort command failed: {} {} (status={})",
                        program,
                        args.join(" "),
                        status
                    );
                    Ok(())
                } else {
                    Err(anyhow!(
                        "external command failed: {} {} (status={})",
                        program,
                        args.join(" "),
                        status
                    ))
                }
            }
            Err(err) => {
                if *best_effort {
                    eprintln!(
                        "warn: best-effort command failed to execute: {} {} ({})",
                        program,
                        args.join(" "),
                        err
                    );
                    Ok(())
                } else {
                    Err(err)
                }
            }
        },
    }
}

fn print_plan(actions: &[Action]) {
    println!("plan:");
    for action in actions {
        match action {
            Action::EnsureDir { path } => println!("  ensure_dir\t{}", path.display()),
            Action::WriteFile { path, .. } => println!("  write_file\t{}", path.display()),
            Action::Exec {
                program, args, env, ..
            } => {
                if env.is_empty() {
                    println!("  exec\t{} {}", program, args.join(" "));
                } else {
                    let env_rendered = env
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("  exec\t{} {} {}", env_rendered, program, args.join(" "));
                }
            }
        }
    }
}

fn write_needs_change(path: &Path, desired: &str) -> Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(current) => Ok(current != desired),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err)
            if err.kind() == std::io::ErrorKind::IsADirectory
                || err.kind() == std::io::ErrorKind::InvalidData =>
        {
            Ok(true)
        }
        Err(err) => Err(anyhow!("failed to read {}: {}", path.display(), err)),
    }
}

fn planned_write_changes(actions: &[Action]) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    for action in actions {
        if let Action::WriteFile { path, content } = action {
            if write_needs_change(path, content)? {
                out.push((path.clone(), content.clone()));
            }
        }
    }
    Ok(out)
}

fn maybe_daemon_reload_for_root(root: &str) -> Result<()> {
    if root == "/" {
        let status = exec_command(
            "systemctl",
            &["daemon-reload".to_string()],
            &std::collections::BTreeMap::new(),
        )?;
        if !status.success() {
            return Err(anyhow!(
                "systemctl daemon-reload failed with status {status}"
            ));
        }
    }
    Ok(())
}

fn resolve_user_runtime_dir(user: &str) -> Option<String> {
    let loginctl_output = Command::new("loginctl")
        .arg("show-user")
        .arg(user)
        .arg("-p")
        .arg("RuntimePath")
        .arg("--value")
        .output()
        .ok();

    if let Some(out) = loginctl_output {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }

    let id_output = Command::new("id").arg("-u").arg(user).output().ok()?;
    if !id_output.status.success() {
        return None;
    }
    let uid = String::from_utf8_lossy(&id_output.stdout)
        .trim()
        .to_string();
    if uid.is_empty() {
        return None;
    }
    Some(format!("/run/user/{uid}"))
}

fn handle_apply(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    profile_name: &str,
    opts: &ApplyOptions,
) -> Result<i32> {
    println!("command=apply");
    println!(
        "profile={} dry_run={} no_oomd={} no_cpu={} no_classes={} force={} user_daemon_reload={}",
        profile_name,
        opts.dry_run,
        opts.no_oomd,
        opts.no_cpu,
        opts.no_classes,
        opts.force,
        opts.user_daemon_reload
    );

    if !opts.dry_run && root == "/" && !is_root_user()? {
        return Ok(3);
    }

    let rooted_config_dir = resolve_with_root(root, PathBuf::from(config_dir))?;
    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;
    let profile = load_profile_from_store(&rooted_config_dir, profile_name).map_err(|err| {
        anyhow!(
            "failed to load profile '{profile_name}' from {}: {err}",
            rooted_config_dir.display()
        )
    })?;

    let validation_errors = validate_profile(&profile);
    if !validation_errors.is_empty() {
        println!("result=invalid");
        for err in validation_errors {
            println!("error\t{}\t{}", err.path, err.message);
        }
        return Ok(2);
    }

    let sudo_user = env::var("SUDO_USER").ok().and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    });
    let sudo_runtime_dir = sudo_user.as_deref().and_then(resolve_user_runtime_dir);

    if opts.user_daemon_reload && root == "/" {
        if let Some(user) = &sudo_user {
            if sudo_runtime_dir.is_none() {
                println!(
                    "hint=could not resolve XDG_RUNTIME_DIR for {}; will try plain sudo --user reload",
                    user
                );
                println!("hint=if this fails, run in user session: systemctl --user daemon-reload");
            }
        } else {
            println!("hint=--user-daemon-reload requested but SUDO_USER is not set");
            println!("hint=run in user session: systemctl --user daemon-reload");
        }
    } else if opts.user_daemon_reload && root != "/" {
        println!("hint=--user-daemon-reload skipped because --root is not '/' (test root mode)");
    }

    let plan = build_apply_plan(
        &profile,
        Path::new(root),
        &PlanOptions {
            no_oomd: opts.no_oomd,
            no_cpu: opts.no_cpu,
            no_classes: opts.no_classes,
            user_daemon_reload: opts.user_daemon_reload,
            sudo_user,
            sudo_runtime_dir,
        },
    );
    let changed_writes = planned_write_changes(&plan)?;

    print_plan(&plan);
    println!("plan_write_changes={}", changed_writes.len());
    if opts.dry_run {
        println!("result=dry-run");
        return Ok(0);
    }

    if changed_writes.is_empty() {
        println!("result=no-changes");
        return Ok(0);
    }

    let mut tx = begin_transaction(&rooted_state_dir)?;
    for action in &plan {
        let step = match action {
            Action::WriteFile { path, .. } => snapshot_before_write(&mut tx, path, Path::new(root))
                .and_then(|_| execute_action(action)),
            _ => execute_action(action),
        };

        if let Err(err) = step {
            eprintln!("apply failed: {err}");
            let failure_manifest = manifest_from_transaction(&tx, Some(profile_name.to_string()));
            let rollback_result =
                rollback_from_manifest(Path::new(root), &rooted_state_dir, &failure_manifest)
                    .and_then(|_| maybe_daemon_reload_for_root(root));
            if rollback_result.is_ok() {
                println!("rollback=attempted");
                return Ok(4);
            }
            eprintln!(
                "rollback attempt failed: {}",
                rollback_result
                    .err()
                    .unwrap_or_else(|| anyhow!("unknown rollback error"))
            );
            return Ok(5);
        }
    }

    let manifest = manifest_from_transaction(&tx, Some(profile_name.to_string()));
    write_backup_manifest(&rooted_state_dir, &manifest)?;
    write_state(&rooted_state_dir, &state_from_manifest(&manifest))?;

    println!("result=ok");
    Ok(0)
}

fn handle_diff(root: &str, config_dir: &str, profile_name: &str) -> Result<i32> {
    println!("command=diff");
    println!("profile={profile_name}");

    let rooted_config_dir = resolve_with_root(root, PathBuf::from(config_dir))?;
    let profile = load_profile_from_store(&rooted_config_dir, profile_name).map_err(|err| {
        anyhow!(
            "failed to load profile '{profile_name}' from {}: {err}",
            rooted_config_dir.display()
        )
    })?;

    let validation_errors = validate_profile(&profile);
    if !validation_errors.is_empty() {
        println!("result=invalid");
        for err in validation_errors {
            println!("error\t{}\t{}", err.path, err.message);
        }
        return Ok(2);
    }

    let plan = build_apply_plan(&profile, Path::new(root), &PlanOptions::default());
    let changed_writes = planned_write_changes(&plan)?;
    for (path, _) in &changed_writes {
        println!("change\t{}", path.display());
    }
    println!("changes={}", changed_writes.len());
    Ok(0)
}

fn handle_init(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    name: Option<String>,
    out: Option<String>,
    apply: bool,
    dry_run: bool,
) -> Result<i32> {
    println!("command=init");

    if dry_run && apply {
        return Ok(2);
    }

    let profile_name = name.unwrap_or_else(|| "auto".to_string());
    let mem_total = read_mem_total_bytes()?;
    let cpus = cpu_count()?;
    let profile = build_auto_profile(&profile_name, mem_total, cpus);

    let yaml = serde_yaml::to_string(&profile)?;

    if dry_run {
        println!("result=dry-run");
        println!("{yaml}");
        return Ok(0);
    }

    let is_root = is_root_user()?;
    if apply && !is_root {
        return Ok(3);
    }

    let destination = if let Some(out_path) = out {
        resolve_with_root(root, PathBuf::from(out_path))?
    } else if is_root {
        let default_store_path = profile_path(config_dir, &profile_name)?;
        resolve_with_root(root, default_store_path)?
    } else {
        PathBuf::from(format!("./{profile_name}.yml"))
    };

    save_profile(&destination, &profile)?;
    println!("result=written");
    println!("path={}", destination.display());

    if apply {
        let apply_opts = ApplyOptions {
            dry_run: false,
            no_oomd: false,
            no_cpu: false,
            no_classes: false,
            force: false,
            user_daemon_reload: false,
        };

        let code = handle_apply(root, config_dir, state_dir, &profile_name, &apply_opts)?;
        return Ok(code);
    }

    Ok(0)
}

fn handle_rollback(root: &str, state_dir: &str, last: bool, to: Option<String>) -> Result<i32> {
    println!("command=rollback");
    println!("last={} to={to:?}", last);

    if root == "/" && !is_root_user()? {
        return Ok(3);
    }

    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;
    let backup_id = if let Some(id) = to {
        id
    } else {
        if !last {
            return Ok(2);
        }
        let state = read_state(&rooted_state_dir)?;
        state
            .backup_id
            .ok_or_else(|| anyhow!("state file has no backup id"))?
    };

    let manifest = read_backup_manifest(&rooted_state_dir, &backup_id)?;
    rollback_from_manifest(Path::new(root), &rooted_state_dir, &manifest)?;
    maybe_daemon_reload_for_root(root)?;

    write_state(&rooted_state_dir, &resguard_state::State::default())?;
    println!("result=ok");
    Ok(0)
}

fn resolve_class_slice(profile: &Profile, class_name: &str) -> Option<String> {
    if let Some(class) = profile.spec.classes.get(class_name) {
        return Some(
            class
                .slice_name
                .clone()
                .unwrap_or_else(|| format!("resguard-{class_name}.slice")),
        );
    }

    if let Some(slices) = &profile.spec.slices {
        if let Some(class) = slices.classes.get(class_name) {
            return Some(
                class
                    .slice_name
                    .clone()
                    .unwrap_or_else(|| format!("resguard-{class_name}.slice")),
            );
        }
    }

    None
}

fn handle_run(root: &str, config_dir: &str, state_dir: &str, req: RunRequest) -> Result<i32> {
    println!("command=run");
    println!(
        "class={} profile={:?} slice={:?} no_check={} wait={} command={:?}",
        req.class, req.profile_override, req.slice_override, req.no_check, req.wait, req.command
    );

    let (resolved_slice, source, profile_for_fix) = if let Some(slice) = req.slice_override {
        (
            slice,
            "slice override via --slice".to_string(),
            req.profile_override,
        )
    } else {
        let rooted_config_dir = resolve_with_root(root, PathBuf::from(config_dir))?;
        let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;

        let (profile_name, profile_source) = if let Some(name) = req.profile_override {
            (name, "explicit --profile".to_string())
        } else {
            let state = read_state(&rooted_state_dir)
                .map_err(|_| {
                    anyhow!(
                        "cannot resolve class '{}': no state found at {}/state.json and no --profile provided",
                        req.class,
                        rooted_state_dir.display()
                    )
                })?;
            let active = state.active_profile.ok_or_else(|| {
                anyhow!(
                    "cannot resolve class '{}': state at {}/state.json has no activeProfile and no --profile provided",
                    req.class,
                    rooted_state_dir.display()
                )
            })?;
            (
                active,
                format!(
                    "activeProfile from {}/state.json",
                    rooted_state_dir.display()
                ),
            )
        };

        let profile =
            load_profile_from_store(&rooted_config_dir, &profile_name).map_err(|err| {
                anyhow!(
                    "failed to load profile '{profile_name}' from {}: {err}",
                    rooted_config_dir.display()
                )
            })?;

        let slice = resolve_class_slice(&profile, &req.class).ok_or_else(|| {
            anyhow!(
                "class '{}' not found in profile '{}' (source: {})",
                req.class,
                profile_name,
                profile_source
            )
        })?;

        (
            slice,
            format!(
                "class '{}' from profile '{}' ({})",
                req.class, profile_name, profile_source
            ),
            Some(profile_name),
        )
    };

    let user_mode = !is_root_user()?;
    if !req.no_check {
        let exists = systemctl_cat_unit(user_mode, &resolved_slice).map_err(|err| {
            let apply_hint_profile = profile_for_fix
                .as_deref()
                .unwrap_or("<profile>");
            anyhow!(
                "slice check failed\nexpected slice: {}\nresolution source: {}\nmode: {}\ncheck error: {}\nnext steps:\n  1) sudo resguard apply {}\n  2) systemctl --user daemon-reload",
                resolved_slice,
                source,
                if user_mode { "user" } else { "system" },
                err,
                apply_hint_profile
            )
        })?;
        if !exists {
            let apply_hint_profile = profile_for_fix.as_deref().unwrap_or("<profile>");
            return Err(anyhow!(
                "slice not found\nexpected slice: {}\nresolution source: {}\nmode: {}\nnext steps:\n  1) sudo resguard apply {}\n  2) systemctl --user daemon-reload",
                resolved_slice,
                source,
                if user_mode { "user" } else { "system" },
                apply_hint_profile
            ));
        }
    } else {
        eprintln!(
            "warn: skipping slice existence check due to --no-check (unsafe, poweruser mode)"
        );
    }

    let code = systemd_run(user_mode, &resolved_slice, req.wait, &req.command)?;
    if req.wait {
        return Ok(code);
    }
    if code == 0 {
        Ok(0)
    } else {
        Ok(6)
    }
}

fn status_value(props: &BTreeMap<String, String>, key: &str) -> String {
    props
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| "-".to_string())
}

fn collect_class_slices_from_state(state: &resguard_state::State) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    for path in &state.managed_paths {
        if let Some(name) = Path::new(path).file_name().and_then(|s| s.to_str()) {
            if name.starts_with("resguard-") && name.ends_with(".slice") {
                out.insert(name.to_string());
            }
        }
    }
    out.into_iter().collect()
}

fn handle_status(root: &str, state_dir: &str) -> Result<i32> {
    println!("command=status");
    let mut partial = false;

    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;
    let state = match read_state(&rooted_state_dir) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("warn: failed to read state: {err}");
            partial = true;
            resguard_state::State::default()
        }
    };

    println!(
        "active_profile={}",
        state
            .active_profile
            .clone()
            .unwrap_or_else(|| "-".to_string())
    );
    let class_slices = collect_class_slices_from_state(&state);
    println!(
        "class_slices={}",
        if class_slices.is_empty() {
            "-".to_string()
        } else {
            class_slices.join(",")
        }
    );

    let keys = ["MemoryHigh", "MemoryMax", "MemoryLow", "AllowedCPUs"];
    for unit in ["system.slice", "user.slice"] {
        match systemctl_show_props(false, unit, &keys) {
            Ok(props) => {
                println!(
                    "{}\tMemoryLow={}\tMemoryHigh={}\tMemoryMax={}\tAllowedCPUs={}",
                    unit,
                    status_value(&props, "MemoryLow"),
                    status_value(&props, "MemoryHigh"),
                    status_value(&props, "MemoryMax"),
                    status_value(&props, "AllowedCPUs")
                );
            }
            Err(err) => {
                eprintln!("warn: failed to read {}: {}", unit, err);
                partial = true;
            }
        }
    }

    for slice in &class_slices {
        match systemctl_show_props(false, slice, &keys) {
            Ok(props) => {
                println!(
                    "{}\tMemoryLow={}\tMemoryHigh={}\tMemoryMax={}\tAllowedCPUs={}",
                    slice,
                    status_value(&props, "MemoryLow"),
                    status_value(&props, "MemoryHigh"),
                    status_value(&props, "MemoryMax"),
                    status_value(&props, "AllowedCPUs")
                );
            }
            Err(err) => {
                eprintln!("warn: failed to read system {}: {}", slice, err);
                partial = true;
            }
        }
    }

    match systemctl_show_props(true, "resguard-browsers.slice", &keys) {
        Ok(props) => {
            println!(
                "user:resguard-browsers.slice\tMemoryLow={}\tMemoryHigh={}\tMemoryMax={}\tAllowedCPUs={}",
                status_value(&props, "MemoryLow"),
                status_value(&props, "MemoryHigh"),
                status_value(&props, "MemoryMax"),
                status_value(&props, "AllowedCPUs")
            );
        }
        Err(err) => {
            eprintln!(
                "warn: failed to read user slice resguard-browsers.slice: {}",
                err
            );
            partial = true;
        }
    }

    match systemctl_is_active("systemd-oomd") {
        Ok(active) => println!("oomd_active={}", active),
        Err(err) => {
            eprintln!("warn: failed to check systemd-oomd: {}", err);
            partial = true;
        }
    }

    let mem_psi_path = if root == "/" {
        "/proc/pressure/memory".to_string()
    } else {
        format!("{}/proc/pressure/memory", root.trim_end_matches('/'))
    };
    let cpu_psi_path = if root == "/" {
        "/proc/pressure/cpu".to_string()
    } else {
        format!("{}/proc/pressure/cpu", root.trim_end_matches('/'))
    };

    match read_pressure_1min(&mem_psi_path) {
        Ok(v) => println!(
            "psi_memory_avg60={}",
            v.map(|x| format!("{x:.2}"))
                .unwrap_or_else(|| "-".to_string())
        ),
        Err(err) => {
            eprintln!("warn: failed to read memory PSI: {}", err);
            partial = true;
        }
    }
    match read_pressure_1min(&cpu_psi_path) {
        Ok(v) => println!(
            "psi_cpu_avg60={}",
            v.map(|x| format!("{x:.2}"))
                .unwrap_or_else(|| "-".to_string())
        ),
        Err(err) => {
            eprintln!("warn: failed to read cpu PSI: {}", err);
            partial = true;
        }
    }

    if partial {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn check_command_success(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn handle_doctor(root: &str, state_dir: &str) -> Result<i32> {
    println!("command=doctor");
    let mut partial = false;

    println!("System checks");
    let systemd_ok = check_command_success("systemctl", &["--version"]);
    if systemd_ok {
        println!("OK  systemd detected");
    } else {
        println!("ERR systemd missing or unavailable (systemctl --version failed)");
        partial = true;
    }

    let cgroup_v2_path = if root == "/" {
        "/sys/fs/cgroup/cgroup.controllers".to_string()
    } else {
        format!(
            "{}/sys/fs/cgroup/cgroup.controllers",
            root.trim_end_matches('/')
        )
    };
    if Path::new(&cgroup_v2_path).exists() {
        println!("OK  cgroups v2 active");
    } else {
        println!("ERR cgroups v2 not detected ({})", cgroup_v2_path);
        partial = true;
    }

    let oomd_enabled = check_command_success("systemctl", &["is-enabled", "systemd-oomd"]);
    if oomd_enabled {
        println!("OK  systemd-oomd enabled");
    } else {
        println!("WARN systemd-oomd not enabled");
        partial = true;
    }

    println!();
    println!("Resguard checks");
    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;
    let state_path = rooted_state_dir.join("state.json");
    let state_present = state_path.exists();
    if state_present {
        println!("OK  state.json present ({})", state_path.display());
    } else {
        println!("WARN state.json missing ({})", state_path.display());
        partial = true;
    }

    let mut slice_paths = Vec::new();
    if let Ok(state) = read_state(&rooted_state_dir) {
        for p in state.managed_paths {
            if p.ends_with(".slice") {
                slice_paths.push(p);
            }
        }
    }
    if slice_paths.is_empty() {
        println!("WARN class slices not found in state");
        partial = true;
    } else {
        let missing = slice_paths
            .iter()
            .filter(|p| !Path::new(p).exists())
            .count();
        if missing == 0 {
            println!("OK  class slices installed");
        } else {
            println!("WARN class slices partially missing (missing {})", missing);
            partial = true;
        }
    }

    println!();
    println!("Hints");
    if env::var("SUDO_USER").is_ok() {
        println!("OK  sudo session detected");
    } else {
        println!("WARN user daemon reload may be required in active session");
        println!("     run: systemctl --user daemon-reload");
        partial = true;
    }

    Ok(if partial { 1 } else { 0 })
}

fn read_meminfo_kb(field: &str) -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let kb = parts.first()?.parse::<u64>().ok()?;
            return Some(kb);
        }
    }
    None
}

fn format_bytes_human(bytes: u64) -> String {
    let gb = 1024_u64.pow(3);
    let mb = 1024_u64.pow(2);
    if bytes >= gb {
        format!("{}G", bytes / gb)
    } else if bytes >= mb {
        format!("{}M", bytes / mb)
    } else {
        format!("{}B", bytes)
    }
}

fn parse_u64_prop(props: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    props.get(key).and_then(|v| v.parse::<u64>().ok())
}

fn list_system_slices() -> Vec<String> {
    let out = Command::new("systemctl")
        .args([
            "list-units",
            "--type=slice",
            "--all",
            "--no-legend",
            "--no-pager",
        ])
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut units = Vec::new();
    for line in text.lines() {
        if let Some(unit) = line.split_whitespace().next() {
            if unit.ends_with(".slice") {
                units.push(unit.to_string());
            }
        }
    }
    units
}

fn handle_metrics() -> Result<i32> {
    println!("command=metrics");
    let mut partial = false;

    let cpu_p = read_pressure_1min("/proc/pressure/cpu").ok().flatten();
    let mem_p = read_pressure_1min("/proc/pressure/memory").ok().flatten();
    let io_p = read_pressure_1min("/proc/pressure/io").ok().flatten();

    println!("CPU pressure");
    match cpu_p {
        Some(v) => println!("avg60={:.2}", v),
        None => {
            println!("avg60=-");
            partial = true;
        }
    }
    println!("Memory pressure");
    match mem_p {
        Some(v) => println!("avg60={:.2}", v),
        None => {
            println!("avg60=-");
            partial = true;
        }
    }
    println!("IO pressure");
    match io_p {
        Some(v) => println!("avg60={:.2}", v),
        None => {
            println!("avg60=-");
            partial = true;
        }
    }
    println!();

    println!("System memory");
    let total = read_meminfo_kb("MemTotal:");
    let available = read_meminfo_kb("MemAvailable:");
    match (total, available) {
        (Some(t), Some(a)) => {
            println!("total={}", format_bytes_human(t * 1024));
            println!("available={}", format_bytes_human(a * 1024));
            println!("used={}", format_bytes_human((t.saturating_sub(a)) * 1024));
        }
        _ => {
            println!("total=-");
            println!("available=-");
            partial = true;
        }
    }
    println!();

    let keys = [
        "MemoryCurrent",
        "MemoryPeak",
        "MemoryLow",
        "MemoryHigh",
        "MemoryMax",
    ];
    println!("User slice usage");
    match systemctl_show_props(false, "user.slice", &keys) {
        Ok(props) => {
            let current = parse_u64_prop(&props, "MemoryCurrent").unwrap_or(0);
            let max = status_value(&props, "MemoryMax");
            let high = status_value(&props, "MemoryHigh");
            println!("user.slice MemoryCurrent: {}", format_bytes_human(current));
            println!("user.slice MemoryHigh: {}", high);
            println!("user.slice MemoryMax: {}", max);
        }
        Err(err) => {
            println!("user.slice: unavailable ({})", err);
            partial = true;
        }
    }
    println!();

    println!("Top slices");
    let mut slice_usage: Vec<(String, u64)> = Vec::new();
    for unit in list_system_slices() {
        if let Ok(props) = systemctl_show_props(false, &unit, &["MemoryCurrent"]) {
            if let Some(cur) = parse_u64_prop(&props, "MemoryCurrent") {
                slice_usage.push((unit, cur));
            }
        }
    }
    if slice_usage.is_empty() {
        println!("unavailable");
        partial = true;
    } else {
        slice_usage.sort_by(|a, b| b.1.cmp(&a.1));
        for (unit, cur) in slice_usage.into_iter().take(5) {
            println!("{} {}", unit, format_bytes_human(cur));
        }
    }

    Ok(if partial { 1 } else { 0 })
}

#[derive(Debug, Clone, Serialize)]
struct DesktopListItem {
    desktop_id: String,
    name: String,
    exec: String,
    icon: Option<String>,
    try_exec: Option<String>,
    terminal: Option<String>,
    entry_type: Option<String>,
    path: String,
    origin: String,
    fields: BTreeMap<String, String>,
}

fn parse_desktop_entry(s: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut in_entry = false;
    for line in s.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

fn short_exec(exec: &str) -> String {
    let max = 80usize;
    if exec.chars().count() <= max {
        return exec.to_string();
    }
    exec.chars().take(max - 3).collect::<String>() + "..."
}

fn desktop_scan_dirs() -> Vec<(PathBuf, DesktopOrigin)> {
    let mut dirs = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        dirs.push((
            PathBuf::from(home).join(".local/share/applications"),
            DesktopOrigin::User,
        ));
    }
    dirs.push((
        PathBuf::from("/usr/local/share/applications"),
        DesktopOrigin::System,
    ));
    dirs.push((
        PathBuf::from("/usr/share/applications"),
        DesktopOrigin::System,
    ));
    dirs
}

fn origin_matches(filter: DesktopOrigin, item_origin: DesktopOrigin) -> bool {
    match filter {
        DesktopOrigin::All => true,
        DesktopOrigin::User => item_origin == DesktopOrigin::User,
        DesktopOrigin::System => item_origin == DesktopOrigin::System,
    }
}

fn discover_desktop_entries(
    origin_filter: DesktopOrigin,
    name_filter: Option<&Regex>,
) -> Result<Vec<DesktopListItem>> {
    let mut items = Vec::new();

    for (dir, origin) in desktop_scan_dirs() {
        if !origin_matches(origin_filter, origin) || !dir.exists() {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };

        for entry in entries {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let map = parse_desktop_entry(&content);
            if map.is_empty() {
                continue;
            }

            if let Some(t) = map.get("Type") {
                if t != "Application" {
                    continue;
                }
            }

            let desktop_id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let name = map.get("Name").cloned().unwrap_or_default();
            let exec = map.get("Exec").cloned().unwrap_or_default();

            if let Some(re) = name_filter {
                let hay = format!("{desktop_id} {name} {exec}");
                if !re.is_match(&hay) {
                    continue;
                }
            }

            let mut fields = BTreeMap::new();
            for (k, v) in map {
                fields.insert(k, v);
            }

            items.push(DesktopListItem {
                desktop_id,
                name,
                exec,
                icon: fields.get("Icon").cloned(),
                try_exec: fields.get("TryExec").cloned(),
                terminal: fields.get("Terminal").cloned(),
                entry_type: fields.get("Type").cloned(),
                path: path.display().to_string(),
                origin: match origin {
                    DesktopOrigin::User => "user".to_string(),
                    DesktopOrigin::System => "system".to_string(),
                    DesktopOrigin::All => "all".to_string(),
                },
                fields,
            });
        }
    }

    items.sort_by(|a, b| {
        a.desktop_id
            .cmp(&b.desktop_id)
            .then(a.origin.cmp(&b.origin))
    });
    Ok(items)
}

fn print_desktop_table(items: &[DesktopListItem]) {
    println!("desktop_id\tname\texec\torigin");
    for item in items {
        println!(
            "{}\t{}\t{}\t{}",
            item.desktop_id,
            if item.name.is_empty() {
                "-"
            } else {
                &item.name
            },
            short_exec(&item.exec),
            item.origin
        );
    }
}

fn handle_desktop_list(format: &str, filter: Option<String>, origin: DesktopOrigin) -> Result<i32> {
    println!("command=desktop list");

    let regex = if let Some(pat) = filter {
        Some(Regex::new(&pat).map_err(|err| anyhow!("invalid --filter regex: {}", err))?)
    } else {
        None
    };

    let items = discover_desktop_entries(origin, regex.as_ref())?;

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&items)?),
        "yaml" => println!("{}", serde_yaml::to_string(&items)?),
        _ => print_desktop_table(&items),
    }

    Ok(0)
}

fn parse_duration_arg(input: &str) -> Result<Duration> {
    let s = input.trim();
    if s.is_empty() {
        return Err(anyhow!("duration must not be empty"));
    }

    let split_at = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());

    let (num_s, unit_s) = s.split_at(split_at);
    let n: u64 = num_s
        .parse()
        .map_err(|_| anyhow!("invalid duration value: {}", num_s))?;

    let secs = match unit_s {
        "" | "s" => n,
        "m" => n.saturating_mul(60),
        "h" => n.saturating_mul(60 * 60),
        _ => return Err(anyhow!("invalid duration unit '{}', use s/m/h", unit_s)),
    };
    Ok(Duration::from_secs(secs))
}

fn handle_panic(root: &str, duration: Option<String>) -> Result<i32> {
    println!("command=panic");
    if root != "/" {
        return Err(anyhow!("panic mode requires --root /"));
    }
    if !is_root_user()? {
        return Ok(3);
    }

    let props = systemctl_show_props(false, "user.slice", &["MemoryMax", "MemoryCurrent"])?;
    let before_max = props
        .get("MemoryMax")
        .cloned()
        .unwrap_or_else(|| "infinity".to_string());
    let before_high = systemctl_show_props(false, "user.slice", &["MemoryHigh"])?
        .get("MemoryHigh")
        .cloned()
        .unwrap_or_else(|| "infinity".to_string());

    let base = parse_u64_prop(&props, "MemoryMax")
        .filter(|v| *v > 0)
        .or_else(|| parse_u64_prop(&props, "MemoryCurrent").filter(|v| *v > 0))
        .or_else(|| read_meminfo_kb("MemTotal:").map(|kb| kb * 1024))
        .ok_or_else(|| anyhow!("failed to resolve base memory for panic mode"))?;

    let target_high = (base as f64 * 0.5) as u64;
    let target_max = (base as f64 * 0.6) as u64;

    let env = BTreeMap::new();
    let set_args = vec![
        "set-property".to_string(),
        "user.slice".to_string(),
        format!("MemoryHigh={}", target_high),
        format!("MemoryMax={}", target_max),
    ];
    let status = exec_command("systemctl", &set_args, &env)?;
    if !status.success() {
        return Err(anyhow!(
            "systemctl set-property failed with status {}",
            status
        ));
    }

    println!(
        "panic_applied user.slice MemoryHigh={} MemoryMax={}",
        format_bytes_human(target_high),
        format_bytes_human(target_max)
    );

    if let Some(d) = duration {
        let wait = parse_duration_arg(&d)?;
        println!("panic_duration={}s", wait.as_secs());
        std::thread::sleep(wait);

        let revert_args = vec![
            "set-property".to_string(),
            "user.slice".to_string(),
            format!("MemoryHigh={}", before_high),
            format!("MemoryMax={}", before_max),
        ];
        let revert_status = exec_command("systemctl", &revert_args, &env)?;
        if !revert_status.success() {
            return Err(anyhow!("panic revert failed with status {}", revert_status));
        }
        println!("panic_reverted");
    } else {
        println!("hint=to revert manually run: sudo systemctl revert user.slice");
    }

    Ok(0)
}

fn main() {
    let cli = Cli::parse();
    print_global_context(&cli);
    let format = cli.format.clone();
    let root = cli.root.clone();
    let config_dir = cli.config_dir.clone();
    let state_dir = cli.state_dir.clone();

    let exit_code = match cli.command {
        Commands::Init {
            name,
            out,
            apply,
            dry_run,
        } => match handle_init(&root, &config_dir, &state_dir, name, out, apply, dry_run) {
            Ok(code) => {
                if code == 2 {
                    eprintln!("invalid arguments: --dry-run and --apply cannot be combined");
                } else if code == 3 {
                    eprintln!("permission denied: --apply requires root");
                }
                code
            }
            Err(err) => {
                eprintln!("init failed: {err}");
                1
            }
        },
        Commands::Apply {
            profile,
            dry_run,
            no_oomd,
            no_cpu,
            no_classes,
            force,
            user_daemon_reload,
        } => match handle_apply(
            &root,
            &config_dir,
            &state_dir,
            &profile,
            &ApplyOptions {
                dry_run,
                no_oomd,
                no_cpu,
                no_classes,
                force,
                user_daemon_reload,
            },
        ) {
            Ok(code) => {
                if code == 3 {
                    eprintln!("permission denied: apply requires root when --root is /");
                }
                code
            }
            Err(err) => {
                eprintln!("apply failed: {err}");
                1
            }
        },
        Commands::Diff { profile } => match handle_diff(&root, &config_dir, &profile) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("diff failed: {err}");
                1
            }
        },
        Commands::Rollback { last, to } => match handle_rollback(&root, &state_dir, last, to) {
            Ok(code) => {
                if code == 2 {
                    eprintln!("invalid arguments: use --last or --to <backup-id>");
                } else if code == 3 {
                    eprintln!("permission denied: rollback requires root when --root is /");
                }
                code
            }
            Err(err) => {
                eprintln!("rollback failed: {err}");
                5
            }
        },
        Commands::Doctor => match handle_doctor(&root, &state_dir) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("doctor failed: {err}");
                1
            }
        },
        Commands::Metrics => match handle_metrics() {
            Ok(code) => code,
            Err(err) => {
                eprintln!("metrics failed: {err}");
                1
            }
        },
        Commands::Panic { duration } => match handle_panic(&root, duration) {
            Ok(code) => {
                if code == 3 {
                    eprintln!("permission denied: panic mode requires root");
                }
                code
            }
            Err(err) => {
                eprintln!("panic failed: {err}");
                1
            }
        },
        Commands::Status => match handle_status(&root, &state_dir) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("status failed: {err}");
                1
            }
        },
        Commands::Run {
            class,
            profile,
            slice,
            no_check,
            wait,
            command,
        } => match handle_run(
            &root,
            &config_dir,
            &state_dir,
            RunRequest {
                class,
                profile_override: profile,
                slice_override: slice,
                no_check,
                wait,
                command,
            },
        ) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("run failed: {err}");
                6
            }
        },
        Commands::Profile { cmd } => match cmd {
            ProfileCmd::List => {
                println!("command=profile list");
                0
            }
            ProfileCmd::Show { name } => {
                println!("command=profile show");
                println!("name={name}");
                0
            }
            ProfileCmd::Import { file } => {
                println!("command=profile import");
                println!("file={file}");
                0
            }
            ProfileCmd::Export { name, out } => {
                println!("command=profile export");
                println!("name={} out={}", name, out);
                0
            }
            ProfileCmd::Validate { target } => {
                println!("command=profile validate");
                if Path::new(&target).exists() {
                    match validate_profile_file(&target) {
                        Ok(errors) if errors.is_empty() => {
                            println!("result=ok");
                            0
                        }
                        Ok(errors) => {
                            println!("result=invalid");
                            for err in errors {
                                println!("error\t{}\t{}", err.path, err.message);
                            }
                            2
                        }
                        Err(err) => {
                            eprintln!("failed to validate profile file: {err}");
                            1
                        }
                    }
                } else {
                    match load_profile_from_store(&config_dir, &target) {
                        Ok(profile) => {
                            let errors = resguard_core::validate_profile(&profile);
                            if errors.is_empty() {
                                println!("result=ok");
                                0
                            } else {
                                println!("result=invalid");
                                for err in errors {
                                    println!("error\t{}\t{}", err.path, err.message);
                                }
                                2
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "failed to load profile '{target}' from store {}: {err}",
                                config_dir
                            );
                            1
                        }
                    }
                }
            }
            ProfileCmd::New { name, from } => {
                println!("command=profile new");
                println!("name={} from={from:?}", name);
                0
            }
            ProfileCmd::Edit { name } => {
                println!("command=profile edit");
                println!("name={name}");
                0
            }
        },
        Commands::Desktop { cmd } => match cmd {
            DesktopCmd::List { filter, origin } => {
                match handle_desktop_list(&format, filter, origin) {
                    Ok(code) => code,
                    Err(err) => {
                        eprintln!("desktop list failed: {err}");
                        1
                    }
                }
            }
            DesktopCmd::Wrap { desktop_id, class } => {
                println!("command=desktop wrap");
                println!("desktop_id={} class={}", desktop_id, class);
                println!("status=stub");
                0
            }
            DesktopCmd::Unwrap { desktop_id } => {
                println!("command=desktop unwrap");
                println!("desktop_id={}", desktop_id);
                println!("status=stub");
                0
            }
            DesktopCmd::Doctor => {
                println!("command=desktop doctor");
                println!("status=stub");
                0
            }
        },
    };

    process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use resguard_core::parse_size_to_bytes;
    use tempfile::tempdir;

    fn test_apply_opts() -> ApplyOptions {
        ApplyOptions {
            dry_run: false,
            no_oomd: false,
            no_cpu: false,
            no_classes: false,
            force: false,
            user_daemon_reload: false,
        }
    }

    fn rooted_state_file(root: &Path) -> PathBuf {
        root.join("var/lib/resguard/state.json")
    }

    fn seed_profile(root: &Path, name: &str) {
        let profile = build_auto_profile(name, 16 * 1024_u64.pow(3), 8);
        let path = root
            .join("etc/resguard/profiles")
            .join(format!("{name}.yml"));
        save_profile(path, &profile).expect("seed profile");
    }

    fn mem_to_bytes(s: &str) -> u64 {
        parse_size_to_bytes(s).expect("parse memory size")
    }

    #[test]
    fn rounding_helpers_round_to_expected_boundaries() {
        let mib_256 = 256 * 1024_u64.pow(2);
        let gb = 1024_u64.pow(3);

        assert_eq!(round_down_to_step(3 * gb + 123, gb), 3 * gb);
        assert_eq!(round_up_to_step(3 * gb + 123, gb), 4 * gb);
        assert_eq!(round_down_to_step(7 * mib_256 + 1, mib_256), 7 * mib_256);
        assert_eq!(round_up_to_step(7 * mib_256 + 1, mib_256), 8 * mib_256);
    }

    #[test]
    fn auto_profile_uses_sane_rounded_caps_for_16g() {
        let gb = 1024_u64.pow(3);
        let profile = build_auto_profile("demo", 16 * gb, 8);
        let memory = profile.spec.memory.expect("memory");
        let user = memory.user.expect("user memory");
        let system = memory.system.expect("system memory");

        assert_eq!(
            mem_to_bytes(system.memory_low.as_deref().expect("memoryLow")),
            2 * gb
        );
        assert_eq!(
            mem_to_bytes(user.memory_max.as_deref().expect("memoryMax")),
            14 * gb
        );
        assert_eq!(
            mem_to_bytes(user.memory_high.as_deref().expect("memoryHigh")),
            12 * gb
        );

        let classes = profile.spec.classes;
        assert_eq!(
            mem_to_bytes(
                classes["browsers"]
                    .memory_max
                    .as_deref()
                    .expect("browsers memoryMax")
            ),
            6 * gb
        );
        assert_eq!(
            mem_to_bytes(classes["ide"].memory_max.as_deref().expect("ide memoryMax")),
            4 * gb
        );
        assert_eq!(
            mem_to_bytes(
                classes["heavy"]
                    .memory_max
                    .as_deref()
                    .expect("heavy memoryMax")
            ),
            4 * gb
        );
    }

    #[test]
    fn auto_profile_cpu_policy_respects_core_count() {
        let gb = 1024_u64.pow(3);

        let low_core = build_auto_profile("low", 8 * gb, 2);
        let low_cpu = low_core.spec.cpu.expect("cpu");
        assert_eq!(low_cpu.enabled, Some(false));
        assert_eq!(low_cpu.reserve_core_for_system, Some(false));
        assert_eq!(low_cpu.system_allowed_cpus, None);
        assert_eq!(low_cpu.user_allowed_cpus, None);

        let high_core = build_auto_profile("high", 16 * gb, 8);
        let high_cpu = high_core.spec.cpu.expect("cpu");
        assert_eq!(high_cpu.enabled, Some(true));
        assert_eq!(high_cpu.reserve_core_for_system, Some(true));
        assert_eq!(high_cpu.system_allowed_cpus.as_deref(), Some("0"));
        assert_eq!(high_cpu.user_allowed_cpus.as_deref(), Some("1-7"));
    }

    #[test]
    fn apply_is_idempotent_and_state_stable() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        seed_profile(&root, "idem");

        let code_first = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "idem",
            &test_apply_opts(),
        )
        .expect("first apply result");
        assert_eq!(code_first, 0);

        let state_before = std::fs::read_to_string(rooted_state_file(&root)).expect("read state 1");
        let code_second = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "idem",
            &test_apply_opts(),
        )
        .expect("second apply result");
        assert_eq!(code_second, 0);
        let state_after = std::fs::read_to_string(rooted_state_file(&root)).expect("read state 2");

        assert_eq!(state_before, state_after);
    }

    #[test]
    fn diff_shows_changes_before_apply_and_none_after_apply() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        seed_profile(&root, "diffprof");

        let rooted_config_dir = root.join("etc/resguard");
        let profile =
            load_profile_from_store(&rooted_config_dir, "diffprof").expect("load profile");
        let plan_before = build_apply_plan(&profile, &root, &PlanOptions::default());
        let changes_before = planned_write_changes(&plan_before).expect("changes before");
        assert!(!changes_before.is_empty());

        let diff_before = handle_diff(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "diffprof",
        )
        .expect("handle diff before");
        assert_eq!(diff_before, 0);

        let apply_code = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "diffprof",
            &test_apply_opts(),
        )
        .expect("apply result");
        assert_eq!(apply_code, 0);

        let plan_after = build_apply_plan(&profile, &root, &PlanOptions::default());
        let changes_after = planned_write_changes(&plan_after).expect("changes after");
        assert!(changes_after.is_empty());

        let diff_after = handle_diff(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "diffprof",
        )
        .expect("handle diff after");
        assert_eq!(diff_after, 0);
    }

    #[test]
    fn apply_then_rollback_restores_file() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        seed_profile(&root, "testprof");

        let managed = root.join("etc/systemd/system/system.slice.d/50-resguard.conf");
        std::fs::create_dir_all(managed.parent().expect("parent")).expect("mkdir parent");
        std::fs::write(&managed, "PRE-APPLY\n").expect("seed managed");

        let code = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "testprof",
            &test_apply_opts(),
        )
        .expect("apply result");
        assert_eq!(code, 0);

        let original = "PRE-APPLY\n".to_string();
        std::fs::write(&managed, "# tampered\n").expect("tamper");

        let rollback_code = handle_rollback(
            root.to_str().expect("root str"),
            "/var/lib/resguard",
            true,
            None,
        )
        .expect("rollback result");
        assert_eq!(rollback_code, 0);

        let restored = std::fs::read_to_string(&managed).expect("read restored");
        assert_eq!(restored, original);
    }

    #[test]
    fn apply_failure_triggers_rollback_attempt() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        seed_profile(&root, "testprof");

        let system_dropin = root.join("etc/systemd/system/system.slice.d/50-resguard.conf");
        std::fs::create_dir_all(system_dropin.parent().expect("parent")).expect("mkdir");
        std::fs::write(&system_dropin, "ORIGINAL\n").expect("seed system dropin");

        let bad_target = root.join("etc/systemd/system/user.slice.d/50-resguard.conf");
        std::fs::create_dir_all(&bad_target).expect("create bad directory target");

        let code = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "testprof",
            &test_apply_opts(),
        )
        .expect("apply result");
        assert_eq!(code, 4);

        let content = std::fs::read_to_string(&system_dropin).expect("read restored system dropin");
        assert_eq!(content, "ORIGINAL\n");
    }

    #[test]
    fn rollback_restores_backed_up_and_removes_created_paths() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        seed_profile(&root, "inv");

        let backed_up_target = root.join("etc/systemd/system/system.slice.d/50-resguard.conf");
        std::fs::create_dir_all(backed_up_target.parent().expect("parent")).expect("mkdir");
        std::fs::write(&backed_up_target, "BEFORE\n").expect("seed backed-up file");

        let code = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "inv",
            &test_apply_opts(),
        )
        .expect("apply result");
        assert_eq!(code, 0);

        std::fs::write(&backed_up_target, "MUTATED\n").expect("mutate backed-up target");
        let created_target = root.join("etc/systemd/user/resguard-browsers.slice");
        assert!(created_target.exists());

        let rollback_code = handle_rollback(
            root.to_str().expect("root str"),
            "/var/lib/resguard",
            true,
            None,
        )
        .expect("rollback result");
        assert_eq!(rollback_code, 0);

        let restored = std::fs::read_to_string(&backed_up_target).expect("restored content");
        assert_eq!(restored, "BEFORE\n");
        assert!(!created_target.exists());
    }
}
