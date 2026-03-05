use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use regex::Regex;
use resguard_config::{load_profile_from_store, profile_path, save_profile, validate_profile_file};
use resguard_core::profile::{
    Class, Cpu, Memory, Metadata, Oomd, Profile, Spec, SuggestRule, SystemMemory, UserMemory,
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
#[cfg(feature = "tui")]
use resguard_system::{
    parse_prop_u64, read_mem_available_bytes, read_pressure, systemctl_list_units,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
#[cfg(feature = "tui")]
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::Duration;
#[cfg(feature = "tui")]
use std::time::Instant;
use std::{collections::HashMap, fs};

#[cfg(feature = "tui")]
use crossterm::event::{self, Event, KeyCode};
#[cfg(feature = "tui")]
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
#[cfg(feature = "tui")]
use crossterm::ExecutableCommand;
#[cfg(feature = "tui")]
use ratatui::backend::CrosstermBackend;
#[cfg(feature = "tui")]
use ratatui::layout::{Constraint, Direction, Layout};
#[cfg(feature = "tui")]
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table};
#[cfg(feature = "tui")]
use ratatui::Terminal;

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
    #[cfg(feature = "tui")]
    Tui,
    Panic {
        #[arg(long, help = "Temporary panic duration like 30s, 10m, 1h")]
        duration: Option<String>,
    },
    Status,
    Suggest {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        apply: bool,
    },
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
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCmd,
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
        #[arg(long)]
        dry_run: bool,
        #[arg(long = "print")]
        print_only: bool,
        #[arg(long = "override")]
        override_mode: bool,
        #[arg(long)]
        force: bool,
    },
    Unwrap {
        desktop_id: String,
        #[arg(long)]
        class: String,
        #[arg(long = "override")]
        override_mode: bool,
    },
    Doctor,
}

#[derive(Subcommand, Debug)]
enum DaemonCmd {
    Enable,
    Disable,
    Status,
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

#[derive(Debug, Clone, Copy)]
struct DesktopWrapOptions {
    force: bool,
    dry_run: bool,
    print_only: bool,
    override_mode: bool,
}

#[derive(Debug, Clone, Copy)]
struct DesktopUnwrapOptions {
    override_mode: bool,
}

#[derive(Debug, Clone, Serialize)]
struct Suggestion {
    scope: String,
    class: String,
    reason: String,
    slice: String,
    exec_start: String,
    memory_current: u64,
    cpu_usage_nsec: u64,
    desktop_id: Option<String>,
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
            suggest: None,
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

fn systemctl_user_scope_units() -> Result<Vec<String>> {
    let out = Command::new("systemctl")
        .args([
            "--user",
            "list-units",
            "--type=scope",
            "--all",
            "--no-legend",
            "--no-pager",
        ])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "systemctl --user list-units failed with status {}",
            out.status
        ));
    }

    let mut scopes = Vec::new();
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let unit = line.split_whitespace().next().unwrap_or_default();
        if unit.ends_with(".scope") {
            scopes.push(unit.to_string());
        }
    }
    Ok(scopes)
}

fn systemctl_user_show_scope(scope: &str) -> Result<BTreeMap<String, String>> {
    systemctl_show_props(
        true,
        scope,
        &["MemoryCurrent", "CPUUsageNSec", "Slice", "ExecStart", "Id"],
    )
}

fn parse_first_exec_token(exec: &str) -> Option<String> {
    let mut iter = exec.split_whitespace().peekable();
    while let Some(tok) = iter.next() {
        if tok == "env" {
            continue;
        }
        if tok.contains('=') && tok.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let cleaned = tok.trim_matches('"').trim_matches('\'');
        if cleaned.is_empty() {
            continue;
        }
        let base = Path::new(cleaned)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(cleaned)
            .to_string();
        return Some(base);
    }
    None
}

fn build_desktop_exec_index() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(entries) = discover_desktop_entries(DesktopOrigin::All, None) {
        for item in entries {
            if let Some(bin) = parse_first_exec_token(&item.exec) {
                map.entry(bin).or_insert(item.desktop_id);
            }
        }
    }
    map
}

fn default_suggest_rules() -> Vec<SuggestRule> {
    vec![
        SuggestRule {
            pattern: "(?i)docker|podman|containerd".to_string(),
            class: "heavy".to_string(),
        },
        SuggestRule {
            pattern: "(?i)code|codium|idea|pycharm|clion|goland".to_string(),
            class: "ide".to_string(),
        },
        SuggestRule {
            pattern: "(?i)firefox|chrome|chromium|brave|opera|vivaldi".to_string(),
            class: "browsers".to_string(),
        },
    ]
}

fn classify_scope(
    unit: &str,
    slice: &str,
    exec_start: &str,
    memory_current: u64,
    rules: &[SuggestRule],
) -> Option<(String, String)> {
    let hay = format!("{unit} {slice} {exec_start}");
    for rule in rules {
        if let Ok(re) = Regex::new(&rule.pattern) {
            if re.is_match(&hay) {
                return Some((
                    rule.class.clone(),
                    format!("matched profile rule /{}/", rule.pattern),
                ));
            }
        }
    }

    let h = hay.to_ascii_lowercase();
    if h.contains("docker") || h.contains("podman") {
        return Some((
            "heavy".to_string(),
            "container workload detected".to_string(),
        ));
    }
    if h.contains("code")
        || h.contains("codium")
        || h.contains("idea")
        || h.contains("pycharm")
        || h.contains("clion")
    {
        return Some(("ide".to_string(), "IDE workload detected".to_string()));
    }

    let gib = 1024_u64.pow(3);
    if slice == "app.slice" && memory_current >= 2 * gib {
        if h.contains("firefox")
            || h.contains("chrome")
            || h.contains("chromium")
            || h.contains("brave")
        {
            return Some((
                "browsers".to_string(),
                "high-memory app.slice browser process".to_string(),
            ));
        }
        return Some((
            "heavy".to_string(),
            "high-memory app.slice process".to_string(),
        ));
    }

    None
}

fn resolve_suggest_profile(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    profile_override: Option<&str>,
) -> Result<(Option<String>, Option<Profile>)> {
    let rooted_config_dir = resolve_with_root(root, PathBuf::from(config_dir))?;
    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;

    let profile_name = if let Some(p) = profile_override {
        Some(p.to_string())
    } else {
        read_state(&rooted_state_dir)
            .ok()
            .and_then(|s| s.active_profile)
    };

    if let Some(name) = profile_name.clone() {
        let profile = load_profile_from_store(&rooted_config_dir, &name).ok();
        Ok((Some(name), profile))
    } else {
        Ok((None, None))
    }
}

fn print_suggestions_table(suggestions: &[Suggestion]) {
    println!("scope\tclass\tdesktop_id\tmemory\treason");
    for s in suggestions {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            s.scope,
            s.class,
            s.desktop_id.as_deref().unwrap_or("-"),
            format_bytes_human(s.memory_current),
            s.reason
        );
    }
}

fn handle_suggest(
    format: &str,
    root: &str,
    config_dir: &str,
    state_dir: &str,
    profile: Option<String>,
    apply: bool,
) -> Result<i32> {
    println!("command=suggest");
    println!("apply={} profile={:?}", apply, profile);

    let (resolved_profile_name, resolved_profile) =
        resolve_suggest_profile(root, config_dir, state_dir, profile.as_deref())?;
    if let Some(name) = &resolved_profile_name {
        println!("profile_source={name}");
    } else {
        println!("profile_source=none (using built-in rules only)");
    }

    let mut rules = Vec::new();
    if let Some(p) = &resolved_profile {
        if let Some(cfg) = &p.spec.suggest {
            for r in &cfg.rules {
                rules.push(r.clone());
            }
        }
    }
    rules.extend(default_suggest_rules());

    let desktop_by_exec = build_desktop_exec_index();
    let scopes = match systemctl_user_scope_units() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("warn: could not query user scopes: {err}");
            return Ok(1);
        }
    };

    let mut suggestions = Vec::new();
    for scope in scopes {
        let props = match systemctl_user_show_scope(&scope) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let exec_start = props.get("ExecStart").cloned().unwrap_or_default();
        let slice = props
            .get("Slice")
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        let memory_current = props
            .get("MemoryCurrent")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let cpu_usage_nsec = props
            .get("CPUUsageNSec")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let Some((class, reason)) =
            classify_scope(&scope, &slice, &exec_start, memory_current, &rules)
        else {
            continue;
        };

        let desktop_id =
            parse_first_exec_token(&exec_start).and_then(|bin| desktop_by_exec.get(&bin).cloned());

        suggestions.push(Suggestion {
            scope,
            class,
            reason,
            slice,
            exec_start,
            memory_current,
            cpu_usage_nsec,
            desktop_id,
        });
    }

    suggestions.sort_by(|a, b| {
        b.memory_current
            .cmp(&a.memory_current)
            .then(a.scope.cmp(&b.scope))
    });
    suggestions.dedup_by(|a, b| a.scope == b.scope && a.class == b.class);

    if suggestions.is_empty() {
        println!("result=no-suggestions");
        println!("hint=run workload, then retry: resguard suggest");
        return Ok(0);
    }

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&suggestions)?),
        "yaml" => println!("{}", serde_yaml::to_string(&suggestions)?),
        _ => print_suggestions_table(&suggestions),
    }

    if apply {
        println!();
        println!("apply_results");
        for s in &suggestions {
            if let Some(desktop_id) = &s.desktop_id {
                let wrapper_path = wrapper_path_for(desktop_id, &s.class)?;
                if wrapper_path.exists() {
                    println!(
                        "skip\t{}\t{}\talready wrapped ({})",
                        desktop_id,
                        s.class,
                        wrapper_path.display()
                    );
                    continue;
                }

                match handle_desktop_wrap(
                    desktop_id,
                    &s.class,
                    DesktopWrapOptions {
                        force: false,
                        dry_run: false,
                        print_only: false,
                        override_mode: false,
                    },
                ) {
                    Ok(0) => println!("ok\t{}\t{}\twrapped", desktop_id, s.class),
                    Ok(code) => {
                        println!("warn\t{}\t{}\twrap returned {}", desktop_id, s.class, code)
                    }
                    Err(err) => println!("warn\t{}\t{}\t{}", desktop_id, s.class, err),
                }
            } else {
                let profile_hint = resolved_profile_name.as_deref().unwrap_or("<profile>");
                println!(
                    "hint\t{}\t{}\tno desktop_id match; wrap manually: resguard desktop list --filter '{}' && resguard desktop wrap <desktop_id> --class {} (then sudo resguard apply {} --user-daemon-reload)",
                    s.scope,
                    s.class,
                    parse_first_exec_token(&s.exec_start).unwrap_or_else(|| s.scope.clone()),
                    s.class,
                    profile_hint
                );
            }
        }
    } else {
        println!();
        println!("next_steps");
        println!("1) review suggestions above");
        println!("2) auto-wrap known desktop entries: resguard suggest --apply");
        println!("3) apply profile so user slices exist: sudo resguard apply <profile> --user-daemon-reload");
    }

    Ok(0)
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

    let has_desktop_mappings = read_desktop_mapping_store()
        .map(|s| !s.mappings.is_empty())
        .unwrap_or(false);
    if has_desktop_mappings {
        println!();
        let (desktop_partial, _) = run_desktop_doctor_checks(false, false)?;
        if desktop_partial {
            partial = true;
        }
    }

    println!();
    println!("Hints");
    if env::var("SUDO_USER").is_ok() {
        println!("OK  sudo session detected");
    } else {
        println!("WARN user daemon reload may be required in active session");
        println!("fix: systemctl --user daemon-reload");
        println!("fix: logout/login");
        partial = true;
    }

    Ok(if partial { 1 } else { 0 })
}

fn run_systemctl_service_action(action: &str, service: &str) -> Result<i32> {
    let status = Command::new("systemctl")
        .arg(action)
        .arg(service)
        .status()?;
    if status.success() {
        println!("result=ok action={} service={}", action, service);
        Ok(0)
    } else {
        eprintln!(
            "systemctl {} {} failed with status {}",
            action, service, status
        );
        Ok(1)
    }
}

fn handle_daemon_enable() -> Result<i32> {
    println!("command=daemon enable");
    run_systemctl_service_action("enable", "resguardd.service")
}

fn handle_daemon_disable() -> Result<i32> {
    println!("command=daemon disable");
    run_systemctl_service_action("disable", "resguardd.service")
}

fn handle_daemon_status() -> Result<i32> {
    println!("command=daemon status");
    let enabled = check_command_success("systemctl", &["is-enabled", "resguardd.service"]);
    let active = check_command_success("systemctl", &["is-active", "resguardd.service"]);
    println!("resguardd.enabled={}", enabled);
    println!("resguardd.active={}", active);
    if !enabled {
        println!("fix: sudo systemctl enable resguardd.service");
    }
    if !active {
        println!("fix: sudo systemctl start resguardd.service");
    }
    Ok(if enabled && active { 0 } else { 1 })
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

#[cfg(feature = "tui")]
#[derive(Debug, Clone)]
struct TuiRow {
    unit: String,
    memory_current: u64,
}

#[cfg(feature = "tui")]
#[derive(Debug, Clone, Default)]
struct TuiSnapshot {
    cpu_avg10: Option<f64>,
    cpu_avg60: Option<f64>,
    mem_avg10: Option<f64>,
    mem_avg60: Option<f64>,
    io_avg10: Option<f64>,
    io_avg60: Option<f64>,
    mem_total: Option<u64>,
    mem_available: Option<u64>,
    user_slice_current: Option<u64>,
    user_slice_max: Option<u64>,
    top_units: Vec<TuiRow>,
}

#[cfg(feature = "tui")]
fn collect_tui_snapshot() -> TuiSnapshot {
    let mut snap = TuiSnapshot::default();

    if let Ok(Some(v)) = read_pressure("/proc/pressure/cpu") {
        snap.cpu_avg10 = Some(v.avg10);
        snap.cpu_avg60 = Some(v.avg60);
    }
    if let Ok(Some(v)) = read_pressure("/proc/pressure/memory") {
        snap.mem_avg10 = Some(v.avg10);
        snap.mem_avg60 = Some(v.avg60);
    }
    if let Ok(Some(v)) = read_pressure("/proc/pressure/io") {
        snap.io_avg10 = Some(v.avg10);
        snap.io_avg60 = Some(v.avg60);
    }

    snap.mem_total = read_mem_total_bytes().ok();
    snap.mem_available = read_mem_available_bytes().ok();

    if let Ok(props) = systemctl_show_props(false, "user.slice", &["MemoryCurrent", "MemoryMax"]) {
        snap.user_slice_current = parse_prop_u64(&props, "MemoryCurrent");
        snap.user_slice_max = parse_prop_u64(&props, "MemoryMax");
    }

    let mut units = Vec::new();
    for unit_type in ["slice", "scope"] {
        if let Ok(list) = systemctl_list_units(false, unit_type) {
            for unit in list {
                if !unit.ends_with(".slice") && !unit.ends_with(".scope") {
                    continue;
                }
                if let Ok(props) = systemctl_show_props(false, &unit, &["MemoryCurrent"]) {
                    if let Some(cur) = parse_prop_u64(&props, "MemoryCurrent") {
                        if cur > 0 {
                            units.push(TuiRow {
                                unit,
                                memory_current: cur,
                            });
                        }
                    }
                }
            }
        }
    }
    units.sort_by(|a, b| b.memory_current.cmp(&a.memory_current));
    units.dedup_by(|a, b| a.unit == b.unit);
    snap.top_units = units.into_iter().take(10).collect();

    snap
}

#[cfg(feature = "tui")]
fn opt_f64(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}"))
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(feature = "tui")]
fn opt_bytes(v: Option<u64>) -> String {
    v.map(format_bytes_human).unwrap_or_else(|| "-".to_string())
}

#[cfg(feature = "tui")]
fn handle_tui() -> Result<i32> {
    println!("command=tui");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = (|| -> Result<i32> {
        let tick = Duration::from_secs(1);
        let mut last = Instant::now()
            .checked_sub(tick)
            .unwrap_or_else(Instant::now);

        loop {
            if last.elapsed() >= tick {
                let snapshot = collect_tui_snapshot();
                terminal.draw(|f| {
                    let area = f.area();
                    let layout = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(5),
                            Constraint::Length(4),
                            Constraint::Length(3),
                            Constraint::Min(8),
                        ])
                        .split(area);

                    let psi_text = format!(
                        "CPU avg10={} avg60={}   MEM avg10={} avg60={}   IO avg10={} avg60={}",
                        opt_f64(snapshot.cpu_avg10),
                        opt_f64(snapshot.cpu_avg60),
                        opt_f64(snapshot.mem_avg10),
                        opt_f64(snapshot.mem_avg60),
                        opt_f64(snapshot.io_avg10),
                        opt_f64(snapshot.io_avg60)
                    );
                    let psi = Paragraph::new(psi_text)
                        .block(Block::default().borders(Borders::ALL).title("PSI"));
                    f.render_widget(psi, layout[0]);

                    let mem_text = format!(
                        "MemTotal={}  MemAvailable={}  user.slice MemoryCurrent={}  MemoryMax={}",
                        opt_bytes(snapshot.mem_total),
                        opt_bytes(snapshot.mem_available),
                        opt_bytes(snapshot.user_slice_current),
                        opt_bytes(snapshot.user_slice_max)
                    );
                    let mem = Paragraph::new(mem_text)
                        .block(Block::default().borders(Borders::ALL).title("Memory"));
                    f.render_widget(mem, layout[1]);

                    let ratio = match (snapshot.user_slice_current, snapshot.user_slice_max) {
                        (Some(cur), Some(max)) if max > 0 => {
                            (cur as f64 / max as f64).clamp(0.0, 1.0)
                        }
                        _ => 0.0,
                    };
                    let gauge = Gauge::default()
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("user.slice usage"),
                        )
                        .ratio(ratio)
                        .label(format!("{:.0}%", ratio * 100.0));
                    f.render_widget(gauge, layout[2]);

                    let rows: Vec<Row> = snapshot
                        .top_units
                        .iter()
                        .map(|r| {
                            Row::new(vec![
                                Cell::from(r.unit.clone()),
                                Cell::from(format_bytes_human(r.memory_current)),
                            ])
                        })
                        .collect();
                    let table = Table::new(
                        rows,
                        [Constraint::Percentage(70), Constraint::Percentage(30)],
                    )
                    .header(Row::new(vec!["unit", "memory"]))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Top scopes/slices by MemoryCurrent (q to quit)"),
                    );
                    f.render_widget(table, layout[3]);
                })?;
                last = Instant::now();
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
                        return Ok(0);
                    }
                }
            }
        }
    })();

    let _ = disable_raw_mode();
    let _ = terminal.backend_mut().execute(LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    result
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

#[derive(Debug, Clone)]
struct DesktopSourceEntry {
    desktop_id: String,
    source_path: PathBuf,
    origin: DesktopOrigin,
    source_content: String,
    fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopMappingEntry {
    wrapper_desktop_id: String,
    wrapper_path: String,
    source_path: String,
    created_at: String,
    mode: Option<String>,
    backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DesktopMappingStore {
    version: u32,
    mappings: BTreeMap<String, BTreeMap<String, DesktopMappingEntry>>,
}

impl Default for DesktopMappingStore {
    fn default() -> Self {
        Self {
            version: 1,
            mappings: BTreeMap::new(),
        }
    }
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

fn validate_desktop_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 200 {
        return Err(anyhow!("invalid desktop id length"));
    }
    if !id.ends_with(".desktop") {
        return Err(anyhow!("desktop id must end with .desktop"));
    }
    if id.contains('/') || id.contains('\\') {
        return Err(anyhow!("desktop id must not contain path separators"));
    }
    if id.contains("..") {
        return Err(anyhow!("desktop id must not contain '..'"));
    }
    Ok(())
}

fn validate_class_name(class: &str) -> Result<()> {
    if class.is_empty() {
        return Err(anyhow!("class must not be empty"));
    }
    if class
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(anyhow!(
            "class contains invalid characters (allowed: a-z A-Z 0-9 - _)"
        ))
    }
}

fn wrapper_desktop_id(desktop_id: &str, class: &str) -> String {
    format!("{desktop_id}.resguard-{class}.desktop")
}

fn validate_wrapper_filename(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 {
        return Err(anyhow!("invalid wrapper filename length"));
    }
    if !name.ends_with(".desktop") {
        return Err(anyhow!("wrapper filename must end with .desktop"));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(anyhow!("wrapper filename contains invalid path components"));
    }
    Ok(())
}

fn wrapper_path_for(desktop_id: &str, class: &str) -> Result<PathBuf> {
    validate_desktop_id(desktop_id)?;
    validate_class_name(class)?;

    let filename = wrapper_desktop_id(desktop_id, class);
    validate_wrapper_filename(&filename)?;

    let base = user_applications_dir()?;
    let path = base.join(filename);
    let rel = path
        .strip_prefix(&base)
        .context("wrapper path escaped base directory")?;
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(anyhow!("wrapper path contains parent traversal"));
    }
    Ok(path)
}

fn override_path_for(desktop_id: &str) -> Result<PathBuf> {
    validate_desktop_id(desktop_id)?;
    Ok(user_applications_dir()?.join(desktop_id))
}

fn wrap_exec(exec: &str, class: &str) -> String {
    format!("resguard run --class {class} -- {}", exec.trim())
}

fn render_wrapper(source: &HashMap<String, String>, class: &str) -> Result<String> {
    let name = source
        .get("Name")
        .cloned()
        .unwrap_or_else(|| "Wrapped App".to_string());
    let exec = source
        .get("Exec")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("Exec missing"))?;

    let mut out = String::new();
    out.push_str("[Desktop Entry]\n");
    out.push_str(&format!("Name={} (Resguard: {})\n", name, class));
    out.push_str(&format!("Exec={}\n", wrap_exec(exec, class)));
    for k in [
        "Type",
        "Icon",
        "TryExec",
        "Terminal",
        "Categories",
        "StartupWMClass",
        "MimeType",
        "Path",
        "GenericName",
        "Comment",
        "DBusActivatable",
    ] {
        if let Some(v) = source.get(k) {
            out.push_str(&format!("{k}={v}\n"));
        }
    }
    Ok(out)
}

fn user_home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

fn user_applications_dir() -> Result<PathBuf> {
    Ok(user_home_dir()?.join(".local/share/applications"))
}

fn desktop_mapping_path() -> Result<PathBuf> {
    Ok(user_home_dir()?.join(".config/resguard/desktop-mapping.yml"))
}

fn read_desktop_mapping_store() -> Result<DesktopMappingStore> {
    let path = desktop_mapping_path()?;
    if !path.exists() {
        return Ok(DesktopMappingStore::default());
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read mapping store {}", path.display()))?;
    let store: DesktopMappingStore = serde_yaml::from_str(&content)
        .with_context(|| format!("failed to parse mapping store {}", path.display()))?;
    Ok(store)
}

fn write_desktop_mapping_store(store: &DesktopMappingStore) -> Result<()> {
    let path = desktop_mapping_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(store)?;
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn now_timestamp_utc() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("unix:{}", d.as_secs()),
        Err(_) => "unix:0".to_string(),
    }
}

fn now_timestamp_for_path() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("unix-{}", d.as_secs()),
        Err(_) => "unix-0".to_string(),
    }
}

fn create_override_backup(path: &Path) -> Result<PathBuf> {
    let backup_dir = user_applications_dir()?
        .join(".resguard-backup")
        .join(now_timestamp_for_path());
    fs::create_dir_all(&backup_dir)
        .with_context(|| format!("failed to create backup dir {}", backup_dir.display()))?;
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow!("override target has no filename"))?;
    let backup_path = backup_dir.join(filename);
    fs::copy(path, &backup_path).with_context(|| {
        format!(
            "failed to back up {} to {}",
            path.display(),
            backup_path.display()
        )
    })?;
    Ok(backup_path)
}

fn render_line_diff(source_label: &str, source: &str, target_label: &str, target: &str) -> String {
    let a: Vec<&str> = source.lines().collect();
    let b: Vec<&str> = target.lines().collect();
    let m = a.len();
    let n = b.len();
    let mut lcs = vec![vec![0usize; n + 1]; m + 1];

    for i in (0..m).rev() {
        for j in (0..n).rev() {
            if a[i] == b[j] {
                lcs[i][j] = lcs[i + 1][j + 1] + 1;
            } else {
                lcs[i][j] = lcs[i + 1][j].max(lcs[i][j + 1]);
            }
        }
    }

    let mut out = String::new();
    out.push_str(&format!("--- {source_label}\n"));
    out.push_str(&format!("+++ {target_label}\n"));

    let (mut i, mut j) = (0usize, 0usize);
    while i < m || j < n {
        if i < m && j < n && a[i] == b[j] {
            out.push_str(&format!(" {}\n", a[i]));
            i += 1;
            j += 1;
            continue;
        }
        if j < n && (i == m || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push_str(&format!("+{}\n", b[j]));
            j += 1;
            continue;
        }
        if i < m {
            out.push_str(&format!("-{}\n", a[i]));
            i += 1;
        }
    }

    out
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

fn resolve_desktop_source(desktop_id: &str) -> Result<DesktopSourceEntry> {
    validate_desktop_id(desktop_id)?;

    for (dir, origin) in desktop_scan_dirs() {
        let path = dir.join(desktop_id);
        if !path.is_file() {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read desktop file {}", path.display()))?;
        let fields = parse_desktop_entry(&content);
        if fields.is_empty() {
            return Err(anyhow!(
                "desktop entry {} has no [Desktop Entry] group",
                path.display()
            ));
        }
        return Ok(DesktopSourceEntry {
            desktop_id: desktop_id.to_string(),
            source_path: path,
            origin,
            source_content: content,
            fields,
        });
    }

    Err(anyhow!(
        "desktop id '{}' not found in XDG search paths",
        desktop_id
    ))
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

fn command_exists_in_path(cmd: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|p| p.join(cmd).is_file())
}

fn handle_desktop_wrap(desktop_id: &str, class: &str, opts: DesktopWrapOptions) -> Result<i32> {
    let source = resolve_desktop_source(desktop_id)?;
    let wrapper_id = wrapper_desktop_id(&source.desktop_id, class);
    let target_path = if opts.override_mode {
        override_path_for(&source.desktop_id)?
    } else {
        wrapper_path_for(&source.desktop_id, class)?
    };
    let target_id = if opts.override_mode {
        source.desktop_id.clone()
    } else {
        wrapper_id.clone()
    };

    if opts.print_only && opts.dry_run {
        return Err(anyhow!(
            "invalid arguments: --print and --dry-run cannot be combined"
        ));
    }
    if opts.override_mode && !opts.force {
        return Err(anyhow!(
            "override mode is destructive by design: pass both --override and --force"
        ));
    }

    if target_path.exists() && !opts.force {
        return Err(anyhow!(
            "target already exists at {} (use --force to overwrite)",
            target_path.display()
        ));
    }

    if source
        .fields
        .get("DBusActivatable")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        eprintln!(
            "warn: source desktop entry has DBusActivatable=true; wrapper may not be used by all launchers"
        );
    }

    let wrapper_content = render_wrapper(&source.fields, class)?;
    if opts.print_only {
        print!("{wrapper_content}");
        return Ok(0);
    }

    if opts.dry_run {
        println!("command=desktop wrap");
        println!(
            "mode={}",
            if opts.override_mode {
                "override"
            } else {
                "wrapper"
            }
        );
        println!("desktop_id={}", source.desktop_id);
        println!("class={class}");
        println!("target_id={target_id}");
        println!("target_path={}", target_path.display());
        println!("write=false");
        println!(
            "{}",
            render_line_diff(
                &source.source_path.display().to_string(),
                &source.source_content,
                &target_path.display().to_string(),
                &wrapper_content
            )
        );
        return Ok(0);
    }

    if opts.override_mode {
        eprintln!("warn: --override writes directly to user desktop-id path");
        eprintln!("warn: target={}", target_path.display());
        eprintln!(
            "warn: backup will be stored in ~/.local/share/applications/.resguard-backup/<timestamp>/"
        );
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut backup_path: Option<PathBuf> = None;
    if opts.override_mode && target_path.exists() {
        backup_path = Some(create_override_backup(&target_path)?);
    }

    write_file(&target_path, &wrapper_content)
        .with_context(|| format!("failed to write wrapper {}", target_path.display()))?;

    let mut store = read_desktop_mapping_store()?;
    let by_class = store
        .mappings
        .entry(source.desktop_id.clone())
        .or_insert_with(BTreeMap::new);
    by_class.insert(
        class.to_string(),
        DesktopMappingEntry {
            wrapper_desktop_id: target_id.clone(),
            wrapper_path: target_path.display().to_string(),
            source_path: source.source_path.display().to_string(),
            created_at: now_timestamp_utc(),
            mode: Some(if opts.override_mode {
                "override".to_string()
            } else {
                "wrapper".to_string()
            }),
            backup_path: backup_path.as_ref().map(|p| p.display().to_string()),
        },
    );
    write_desktop_mapping_store(&store)?;

    println!("command=desktop wrap");
    println!("desktop_id={}", source.desktop_id);
    println!("class={class}");
    println!(
        "mode={}",
        if opts.override_mode {
            "override"
        } else {
            "wrapper"
        }
    );
    println!(
        "source={} ({})",
        source.source_path.display(),
        match source.origin {
            DesktopOrigin::User => "user",
            DesktopOrigin::System => "system",
            DesktopOrigin::All => "all",
        }
    );
    println!("wrapper_id={target_id}");
    println!("wrapper_path={}", target_path.display());
    if let Some(path) = backup_path {
        println!("backup_path={}", path.display());
    }
    println!("mapping_file={}", desktop_mapping_path()?.display());
    Ok(0)
}

fn handle_desktop_unwrap(desktop_id: &str, class: &str, opts: DesktopUnwrapOptions) -> Result<i32> {
    println!("command=desktop unwrap");
    let expected_wrapper_path = if opts.override_mode {
        override_path_for(desktop_id)?
    } else {
        wrapper_path_for(desktop_id, class)?
    };

    let mut store = read_desktop_mapping_store()?;
    let mut removed = false;
    let mut restored_backup_path: Option<PathBuf> = None;

    if let Some(by_class) = store.mappings.get_mut(desktop_id) {
        if let Some(entry) = by_class.remove(class) {
            if Path::new(&entry.wrapper_path) != expected_wrapper_path {
                eprintln!(
                    "warn: ignoring non-canonical wrapper path in mapping: {} (expected {})",
                    entry.wrapper_path,
                    expected_wrapper_path.display()
                );
            }
            if opts.override_mode {
                if let Some(backup) = &entry.backup_path {
                    let backup_path = PathBuf::from(backup);
                    if backup_path.is_file() {
                        if let Some(parent) = expected_wrapper_path.parent() {
                            fs::create_dir_all(parent).with_context(|| {
                                format!("failed to create {}", parent.display())
                            })?;
                        }
                        fs::copy(&backup_path, &expected_wrapper_path).with_context(|| {
                            format!(
                                "failed to restore backup {} to {}",
                                backup_path.display(),
                                expected_wrapper_path.display()
                            )
                        })?;
                        removed = true;
                        restored_backup_path = Some(backup_path);
                    }
                }
            }
            if !removed && expected_wrapper_path.exists() {
                fs::remove_file(&expected_wrapper_path).with_context(|| {
                    format!(
                        "failed to remove wrapper {}",
                        expected_wrapper_path.display()
                    )
                })?;
                removed = true;
            }
        }
        if by_class.is_empty() {
            store.mappings.remove(desktop_id);
        }
    }

    if !removed {
        if expected_wrapper_path.exists() {
            fs::remove_file(&expected_wrapper_path).with_context(|| {
                format!(
                    "failed to remove wrapper {}",
                    expected_wrapper_path.display()
                )
            })?;
            removed = true;
        }
    }

    write_desktop_mapping_store(&store)?;

    if removed {
        println!("desktop_id={desktop_id}");
        println!("class={class}");
        println!(
            "mode={}",
            if opts.override_mode {
                "override"
            } else {
                "wrapper"
            }
        );
        if let Some(path) = restored_backup_path {
            println!("status=restored-backup");
            println!("backup_path={}", path.display());
        } else {
            println!("status=removed");
        }
        Ok(0)
    } else {
        println!("desktop_id={desktop_id}");
        println!("class={class}");
        println!(
            "mode={}",
            if opts.override_mode {
                "override"
            } else {
                "wrapper"
            }
        );
        println!("status=not-found");
        Ok(1)
    }
}

fn validate_wrapper_file(path: &Path) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read wrapper {}", path.display()))?;
    let fields = parse_desktop_entry(&content);
    if fields.is_empty() {
        return Err(anyhow!("missing [Desktop Entry] group"));
    }
    let exec = fields
        .get("Exec")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Exec missing"))?;
    if !exec.starts_with("resguard run --class ") || !exec.contains(" -- ") {
        return Err(anyhow!(
            "Exec is not a resguard wrapper command: expected 'resguard run --class <class> -- ...'"
        ));
    }
    Ok(())
}

fn run_desktop_doctor_checks(print_command: bool, require_mapping: bool) -> Result<(bool, bool)> {
    if print_command {
        println!("command=desktop doctor");
    }
    let mut partial = false;

    println!("Desktop checks");
    if command_exists_in_path("resguard") {
        println!("OK  resguard found in PATH");
    } else {
        println!("WARN resguard not found in PATH");
        println!("fix: export PATH=\"$HOME/.local/bin:$PATH\"");
        partial = true;
    }

    let store = read_desktop_mapping_store()?;
    if store.mappings.is_empty() {
        if require_mapping {
            println!("WARN no desktop wrappers in mapping store");
            println!("fix: resguard desktop wrap <desktop_id> --class <class>");
            return Ok((true, false));
        }
        println!("INFO no desktop wrappers in mapping store; skipping desktop checks");
        return Ok((false, false));
    }
    let has_mappings = true;

    println!(
        "OK  mapping store loaded ({} desktop ids)",
        store.mappings.len()
    );

    let mut needs_user_reload_hint = false;
    for (desktop_id, by_class) in &store.mappings {
        for (class, entry) in by_class {
            let wrapper_path = PathBuf::from(&entry.wrapper_path);
            if wrapper_path.exists() {
                match validate_wrapper_file(&wrapper_path) {
                    Ok(()) => println!(
                        "OK  wrapper exists and is parseable: {} [{}]",
                        desktop_id, class
                    ),
                    Err(err) => {
                        println!(
                            "WARN wrapper invalid: {} [{}] at {} ({})",
                            desktop_id, class, entry.wrapper_path, err
                        );
                        println!(
                            "fix: resguard desktop wrap {} --class {} --force",
                            desktop_id, class
                        );
                        partial = true;
                    }
                }
            } else {
                println!(
                    "WARN wrapper missing: {} [{}] at {}",
                    desktop_id, class, entry.wrapper_path
                );
                println!(
                    "fix: resguard desktop wrap {} --class {}",
                    desktop_id, class
                );
                partial = true;
            }

            let slice = format!("resguard-{class}.slice");
            match systemctl_cat_unit(true, &slice) {
                Ok(true) => println!("OK  user slice present: {}", slice),
                Ok(false) | Err(_) => {
                    println!(
                        "WARN user slice missing or user daemon unavailable: {}",
                        slice
                    );
                    println!("fix: sudo resguard apply <profile> --user-daemon-reload");
                    partial = true;
                    needs_user_reload_hint = true;
                }
            }
        }
    }

    if needs_user_reload_hint {
        println!("Hints");
        println!("WARN user daemon reload may be required");
        println!("fix: systemctl --user daemon-reload");
        println!("fix: loginctl terminate-user \"$USER\"   # or logout/login");
    }

    Ok((partial, has_mappings))
}

fn handle_desktop_doctor() -> Result<i32> {
    let (partial, _) = run_desktop_doctor_checks(true, true)?;
    Ok(if partial { 1 } else { 0 })
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
        #[cfg(feature = "tui")]
        Commands::Tui => match handle_tui() {
            Ok(code) => code,
            Err(err) => {
                eprintln!("tui failed: {err}");
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
        Commands::Suggest { profile, apply } => {
            match handle_suggest(&format, &root, &config_dir, &state_dir, profile, apply) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("suggest failed: {err}");
                    1
                }
            }
        }
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
            DesktopCmd::Wrap {
                desktop_id,
                class,
                dry_run,
                print_only,
                override_mode,
                force,
            } => match handle_desktop_wrap(
                &desktop_id,
                &class,
                DesktopWrapOptions {
                    force,
                    dry_run,
                    print_only,
                    override_mode,
                },
            ) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("desktop wrap failed: {err}");
                    1
                }
            },
            DesktopCmd::Unwrap {
                desktop_id,
                class,
                override_mode,
            } => {
                match handle_desktop_unwrap(
                    &desktop_id,
                    &class,
                    DesktopUnwrapOptions { override_mode },
                ) {
                    Ok(code) => code,
                    Err(err) => {
                        eprintln!("desktop unwrap failed: {err}");
                        1
                    }
                }
            }
            DesktopCmd::Doctor => match handle_desktop_doctor() {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("desktop doctor failed: {err}");
                    1
                }
            },
        },
        Commands::Daemon { cmd } => match cmd {
            DaemonCmd::Enable => match handle_daemon_enable() {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("daemon enable failed: {err}");
                    1
                }
            },
            DaemonCmd::Disable => match handle_daemon_disable() {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("daemon disable failed: {err}");
                    1
                }
            },
            DaemonCmd::Status => match handle_daemon_status() {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("daemon status failed: {err}");
                    1
                }
            },
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

    #[test]
    fn desktop_id_validation_enforces_safe_format() {
        assert!(validate_desktop_id("firefox.desktop").is_ok());
        assert!(validate_desktop_id("").is_err());
        assert!(validate_desktop_id("firefox").is_err());
        assert!(validate_desktop_id("../firefox.desktop").is_err());
        assert!(validate_desktop_id("foo/bar.desktop").is_err());
    }

    #[test]
    fn wrapper_path_is_normalized_under_user_applications() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).expect("create home");

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let path = wrapper_path_for("firefox.desktop", "browsers").expect("wrapper path");
        assert!(path.starts_with(home.join(".local/share/applications")));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("firefox.desktop.resguard-browsers.desktop")
        );

        assert!(wrapper_path_for("../firefox.desktop", "browsers").is_err());
        assert!(wrapper_path_for("firefox.desktop", "bad/class").is_err());

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn wrap_exec_keeps_original_placeholders() {
        let wrapped = wrap_exec("firefox %u", "browsers");
        assert_eq!(wrapped, "resguard run --class browsers -- firefox %u");
    }

    #[test]
    fn parse_first_exec_token_extracts_binary() {
        assert_eq!(
            parse_first_exec_token("env FOO=1 /usr/bin/firefox %u").as_deref(),
            Some("firefox")
        );
        assert_eq!(
            parse_first_exec_token("code --new-window").as_deref(),
            Some("code")
        );
    }

    #[test]
    fn classify_scope_applies_default_heuristics() {
        let rules = default_suggest_rules();
        let got = classify_scope(
            "app-foo.scope",
            "app.slice",
            "/usr/bin/firefox %u",
            3 * 1024_u64.pow(3),
            &rules,
        );
        assert_eq!(got.map(|v| v.0), Some("browsers".to_string()));

        let got2 = classify_scope(
            "app-bar.scope",
            "app.slice",
            "podman run something",
            512 * 1024_u64.pow(2),
            &rules,
        );
        assert_eq!(got2.map(|v| v.0), Some("heavy".to_string()));
    }

    #[test]
    fn render_wrapper_preserves_common_fields() {
        let mut src = HashMap::new();
        src.insert("Name".to_string(), "Firefox".to_string());
        src.insert("Exec".to_string(), "firefox %u".to_string());
        src.insert("Type".to_string(), "Application".to_string());
        src.insert("Icon".to_string(), "firefox".to_string());
        src.insert("Terminal".to_string(), "false".to_string());
        src.insert("Categories".to_string(), "Network;WebBrowser;".to_string());

        let wrapped = render_wrapper(&src, "browsers").expect("render wrapper");
        assert!(wrapped.contains("Name=Firefox (Resguard: browsers)\n"));
        assert!(wrapped.contains("Exec=resguard run --class browsers -- firefox %u\n"));
        assert!(wrapped.contains("Type=Application\n"));
        assert!(wrapped.contains("Icon=firefox\n"));
        assert!(wrapped.contains("Terminal=false\n"));
        assert!(wrapped.contains("Categories=Network;WebBrowser;\n"));
    }

    #[test]
    fn render_line_diff_marks_changes() {
        let before = "[Desktop Entry]\nName=Firefox\nExec=firefox %u\n";
        let after = "[Desktop Entry]\nName=Firefox (Resguard: browsers)\nExec=resguard run --class browsers -- firefox %u\n";
        let diff = render_line_diff("before.desktop", before, "after.desktop", after);
        assert!(diff.contains("--- before.desktop\n"));
        assert!(diff.contains("+++ after.desktop\n"));
        assert!(diff.contains("-Name=Firefox\n"));
        assert!(diff.contains("+Name=Firefox (Resguard: browsers)\n"));
    }

    #[test]
    fn desktop_wrap_dry_run_and_print_only_do_not_write_files() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).expect("create apps dir");
        std::fs::write(
            apps.join("firefox.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n",
        )
        .expect("write source desktop");

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let wrapper_path = wrapper_path_for("firefox.desktop", "browsers").expect("wrapper path");
        let mapping_path = desktop_mapping_path().expect("mapping path");

        let dry_run_code = handle_desktop_wrap(
            "firefox.desktop",
            "browsers",
            DesktopWrapOptions {
                force: false,
                dry_run: true,
                print_only: false,
                override_mode: false,
            },
        )
        .expect("dry-run code");
        assert_eq!(dry_run_code, 0);
        assert!(!wrapper_path.exists());
        assert!(!mapping_path.exists());

        let print_code = handle_desktop_wrap(
            "firefox.desktop",
            "browsers",
            DesktopWrapOptions {
                force: false,
                dry_run: false,
                print_only: true,
                override_mode: false,
            },
        )
        .expect("print-only code");
        assert_eq!(print_code, 0);
        assert!(!wrapper_path.exists());
        assert!(!mapping_path.exists());

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn desktop_wrap_override_requires_force() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).expect("create apps dir");
        std::fs::write(
            apps.join("firefox.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n",
        )
        .expect("write source desktop");

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let err = handle_desktop_wrap(
            "firefox.desktop",
            "browsers",
            DesktopWrapOptions {
                force: false,
                dry_run: false,
                print_only: false,
                override_mode: true,
            },
        )
        .expect_err("override without force should fail");
        assert!(err.to_string().contains("--override"));

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    fn desktop_unwrap_override_restores_backup() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).expect("create apps dir");
        let source_path = apps.join("firefox.desktop");
        let original = "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n";
        std::fs::write(&source_path, original).expect("write source desktop");

        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);

        let wrap_code = handle_desktop_wrap(
            "firefox.desktop",
            "browsers",
            DesktopWrapOptions {
                force: true,
                dry_run: false,
                print_only: false,
                override_mode: true,
            },
        )
        .expect("wrap override code");
        assert_eq!(wrap_code, 0);
        let wrapped = std::fs::read_to_string(&source_path).expect("read wrapped content");
        assert!(wrapped.contains("Exec=resguard run --class browsers -- firefox %u"));

        let unwrap_code = handle_desktop_unwrap(
            "firefox.desktop",
            "browsers",
            DesktopUnwrapOptions {
                override_mode: true,
            },
        )
        .expect("unwrap override code");
        assert_eq!(unwrap_code, 0);
        let restored = std::fs::read_to_string(&source_path).expect("read restored content");
        assert_eq!(restored, original);

        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
