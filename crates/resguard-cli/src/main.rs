use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
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
#[cfg(feature = "tui")]
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::Duration;
#[cfg(feature = "tui")]
use std::time::Instant;
use std::{collections::HashMap, fs};

mod commands;
mod util;
pub(crate) use util::system::{
    check_command_success, format_bytes_human, list_system_slices, parse_u64_prop,
    partial_exit_code, read_meminfo_kb,
};

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
#[command(
    name = "resguard",
    about = "Linux resource guard using systemd slices",
    version = env!("CARGO_PKG_VERSION")
)]
struct Cli {
    #[arg(long, global = true, default_value = "table")]
    format: String,
    #[arg(long, global = true, help = "Emit structured logs to stderr")]
    json_log: bool,
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
    Setup {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        apply: bool,
        #[arg(long)]
        suggest: bool,
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
    Tui {
        #[arg(long, default_value_t = 1000)]
        interval: u64,
        #[arg(long)]
        no_top: bool,
    },
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
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 70)]
        confidence_threshold: u8,
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
    Rescue {
        #[arg(long, default_value = "rescue")]
        class: String,
        #[arg(long, help = "Custom shell command to run instead of htop/top")]
        command: Option<String>,
        #[arg(long, help = "Start an interactive shell without launching htop/top")]
        no_ui: bool,
        #[arg(
            long,
            help = "If rescue class/slice is missing, fallback to system.slice (unsafe, poweruser only)"
        )]
        no_check: bool,
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
    Completion {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    Version,
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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
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

#[derive(Debug)]
struct SuggestRequest {
    format: String,
    root: String,
    config_dir: String,
    state_dir: String,
    profile: Option<String>,
    apply: bool,
    dry_run: bool,
    confidence_threshold: u8,
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
    confidence: u8,
    confidence_reason: String,
}

fn print_global_context(cli: &Cli) {
    println!(
        "format={} json_log={} verbose={} quiet={} no_color={} root={} config_dir={} state_dir={}",
        cli.format,
        cli.json_log,
        cli.verbose,
        cli.quiet,
        cli.no_color,
        cli.root,
        cli.config_dir,
        cli.state_dir
    );
}

fn json_log_enabled(cli: &Cli) -> bool {
    let env_val = env::var("RESGUARD_LOG").ok();
    json_log_enabled_from_env(cli.json_log, env_val.as_deref())
}

fn json_log_enabled_from_env(flag: bool, env_value: Option<&str>) -> bool {
    flag || env_value.is_some_and(|v| v.eq_ignore_ascii_case("json"))
}

fn emit_log(json_log: bool, level: &str, event: &str, message: &str) {
    if !json_log {
        return;
    }
    let payload = serde_json::json!({
        "level": level,
        "event": event,
        "message": message
    });
    eprintln!("{payload}");
}

fn handle_completion(shell: CompletionShell) -> Result<i32> {
    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    let mut out = std::io::stdout();
    let target = match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
    };
    generate(target, &mut cmd, bin_name, &mut out);
    Ok(0)
}

fn cli_version_output() -> String {
    let cmd = Cli::command();
    cmd.render_version().to_string()
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
    classes.insert(
        "rescue".to_string(),
        Class {
            slice_name: Some("resguard-rescue.slice".to_string()),
            memory_high: None,
            memory_max: Some("1G".to_string()),
            cpu_weight: Some(100),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("70%".to_string()),
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
    commands::apply::handle_apply(root, config_dir, state_dir, profile_name, opts)
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

fn handle_setup(
    format: &str,
    root: &str,
    config_dir: &str,
    state_dir: &str,
    name: Option<String>,
    apply: bool,
    suggest: bool,
) -> Result<i32> {
    commands::setup::handle_setup(format, root, config_dir, state_dir, name, apply, suggest)
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

#[cfg(test)]
fn build_rescue_command(shell: &str, custom_command: Option<&str>, no_ui: bool) -> Vec<String> {
    commands::rescue::build_rescue_command(shell, custom_command, no_ui)
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

fn handle_rescue(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    class: String,
    custom_command: Option<String>,
    no_ui: bool,
    no_check: bool,
) -> Result<i32> {
    commands::rescue::handle_rescue(
        root,
        config_dir,
        state_dir,
        class,
        custom_command,
        no_ui,
        no_check,
    )
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
    for tok in exec.split_whitespace() {
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

fn parse_snap_run_app(exec: &str) -> Option<String> {
    let mut cleaned = Vec::new();
    for tok in exec.split_whitespace() {
        if tok == "env" {
            continue;
        }
        if tok.contains('=') && tok.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let t = tok.trim_matches('"').trim_matches('\'');
        if !t.is_empty() {
            cleaned.push(t.to_string());
        }
    }

    let mut i = 0usize;
    while i + 2 < cleaned.len() {
        let base = Path::new(&cleaned[i])
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(&cleaned[i]);
        if base == "snap" && cleaned[i + 1] == "run" {
            for app in &cleaned[(i + 2)..] {
                if app.starts_with('-') {
                    continue;
                }
                return Some(app.to_string());
            }
            return None;
        }
        i += 1;
    }
    None
}

fn parse_snap_app_from_scope(scope: &str) -> Option<String> {
    let mut s = scope.strip_suffix(".scope").unwrap_or(scope);
    if let Some(rest) = s.strip_prefix("app-") {
        s = rest;
    }
    let rest = s.strip_prefix("snap.")?;
    let mut parts = rest.split('.');
    let _snap_name = parts.next()?;
    let app_raw = parts.next()?;
    let app = app_raw
        .split_once('-')
        .map(|(left, _)| left)
        .unwrap_or(app_raw);
    if app.is_empty() {
        None
    } else {
        Some(app.to_string())
    }
}

fn index_desktop_exec_key(map: &mut HashMap<String, Vec<String>>, key: String, desktop_id: &str) {
    if key.is_empty() {
        return;
    }
    let ids = map.entry(key).or_default();
    if !ids.iter().any(|v| v == desktop_id) {
        ids.push(desktop_id.to_string());
    }
}

fn desktop_id_stem(desktop_id: &str) -> Option<&str> {
    desktop_id.strip_suffix(".desktop")
}

fn snap_app_from_desktop_id(desktop_id: &str) -> Option<String> {
    let stem = desktop_id_stem(desktop_id)?;
    if let Some((_, app)) = stem.split_once('_') {
        if !app.is_empty() {
            return Some(app.to_string());
        }
    }
    if let Some(rest) = stem.strip_prefix("snap.") {
        let mut parts = rest.split('.');
        let _snap_name = parts.next()?;
        let app = parts.next()?;
        if !app.is_empty() {
            return Some(app.to_string());
        }
    }
    None
}

fn build_desktop_exec_index() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(entries) = discover_desktop_entries(DesktopOrigin::All, None) {
        for item in entries {
            if let Some(bin) = parse_first_exec_token(&item.exec) {
                index_desktop_exec_key(&mut map, bin, &item.desktop_id);
            }
            if let Some(snap_app) = parse_snap_run_app(&item.exec) {
                index_desktop_exec_key(&mut map, format!("snap:{snap_app}"), &item.desktop_id);
            }
            if let Some(snap_app) = snap_app_from_desktop_id(&item.desktop_id) {
                index_desktop_exec_key(&mut map, format!("snap:{snap_app}"), &item.desktop_id);
            }
        }
    }
    map
}

fn unique_desktop_id_for_scope_exec(
    scope: &str,
    exec_start: &str,
    desktop_by_exec: &HashMap<String, Vec<String>>,
) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(bin) = parse_first_exec_token(exec_start) {
        candidates.push(bin);
    }
    if let Some(snap_app) = parse_snap_run_app(exec_start) {
        candidates.push(format!("snap:{snap_app}"));
        candidates.push(snap_app);
    }
    if let Some(snap_app) = parse_snap_app_from_scope(scope) {
        candidates.push(format!("snap:{snap_app}"));
        candidates.push(snap_app);
    }

    let mut matches: Vec<String> = Vec::new();
    for key in candidates {
        if let Some(ids) = desktop_by_exec.get(&key) {
            for id in ids {
                if !matches.iter().any(|v| v == id) {
                    matches.push(id.clone());
                }
            }
        }
    }

    if matches.len() == 1 {
        return matches.first().cloned();
    }
    None
}

#[derive(Debug, Clone)]
struct SuggestClassification {
    class: String,
    reason: String,
    pattern_match: bool,
    memory_threshold_match: bool,
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
) -> Option<SuggestClassification> {
    let hay = format!("{unit} {slice} {exec_start}");
    for rule in rules {
        if let Ok(re) = Regex::new(&rule.pattern) {
            if re.is_match(&hay) {
                return Some(SuggestClassification {
                    class: rule.class.clone(),
                    reason: format!("matched profile rule /{}/", rule.pattern),
                    pattern_match: true,
                    memory_threshold_match: false,
                });
            }
        }
    }

    let h = hay.to_ascii_lowercase();
    if h.contains("docker") || h.contains("podman") {
        return Some(SuggestClassification {
            class: "heavy".to_string(),
            reason: "container workload detected".to_string(),
            pattern_match: true,
            memory_threshold_match: false,
        });
    }
    if h.contains("code")
        || h.contains("codium")
        || h.contains("idea")
        || h.contains("pycharm")
        || h.contains("clion")
    {
        return Some(SuggestClassification {
            class: "ide".to_string(),
            reason: "IDE workload detected".to_string(),
            pattern_match: true,
            memory_threshold_match: false,
        });
    }

    let gib = 1024_u64.pow(3);
    if slice == "app.slice" && memory_current >= 2 * gib {
        if h.contains("firefox")
            || h.contains("chrome")
            || h.contains("chromium")
            || h.contains("brave")
        {
            return Some(SuggestClassification {
                class: "browsers".to_string(),
                reason: "high-memory app.slice browser process".to_string(),
                pattern_match: true,
                memory_threshold_match: true,
            });
        }
        return Some(SuggestClassification {
            class: "heavy".to_string(),
            reason: "high-memory app.slice process".to_string(),
            pattern_match: false,
            memory_threshold_match: true,
        });
    }

    None
}

fn confidence_score(
    pattern_match: bool,
    memory_threshold_match: bool,
    known_desktop_id: bool,
) -> (u8, String) {
    let mut score = 0u8;
    let mut reasons = Vec::new();
    if pattern_match {
        score = score.saturating_add(40);
        reasons.push("pattern");
    }
    if memory_threshold_match {
        score = score.saturating_add(30);
        reasons.push("memory");
    }
    if known_desktop_id {
        score = score.saturating_add(30);
        reasons.push("desktop-id");
    }
    if reasons.is_empty() {
        reasons.push("none");
    }
    (score.min(100), reasons.join("+"))
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
    println!("scope\tclass\tconfidence\tdesktop_id\tmemory\treason");
    for s in suggestions {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            s.scope,
            s.class,
            s.confidence,
            s.desktop_id.as_deref().unwrap_or("-"),
            format_bytes_human(s.memory_current),
            s.reason
        );
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

fn handle_doctor(root: &str, state_dir: &str) -> Result<i32> {
    commands::doctor::handle_doctor(root, state_dir)
}

fn handle_daemon_enable() -> Result<i32> {
    commands::daemon::handle_daemon_enable()
}

fn handle_daemon_disable() -> Result<i32> {
    commands::daemon::handle_daemon_disable()
}

fn handle_daemon_status() -> Result<i32> {
    commands::daemon::handle_daemon_status()
}

fn handle_metrics() -> Result<i32> {
    commands::metrics::handle_metrics()
}

#[cfg(feature = "tui")]
fn handle_tui(interval_ms: u64, no_top: bool) -> Result<i32> {
    commands::tui::handle_tui(interval_ms, no_top)
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
    ] {
        if let Some(v) = source.get(k) {
            out.push_str(&format!("{k}={v}\n"));
        }
    }

    // Wrappers must route via Exec; force DBus activation off so launchers do not bypass Exec.
    if source.contains_key("DBusActivatable") {
        out.push_str("DBusActivatable=false\n");
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

fn push_scan_dir(
    out: &mut Vec<(PathBuf, DesktopOrigin)>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    dir: PathBuf,
    origin: DesktopOrigin,
) {
    if seen.insert(dir.clone()) {
        out.push((dir, origin));
    }
}

fn desktop_scan_dirs() -> Vec<(PathBuf, DesktopOrigin)> {
    let mut dirs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    let user_data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(data_home) = user_data_home {
        push_scan_dir(
            &mut dirs,
            &mut seen,
            data_home.join("applications"),
            DesktopOrigin::User,
        );
    }

    if let Some(raw) = env::var_os("XDG_DATA_DIRS") {
        for dir in env::split_paths(&raw) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            push_scan_dir(
                &mut dirs,
                &mut seen,
                dir.join("applications"),
                DesktopOrigin::System,
            );
        }
    }

    for path in [
        "/usr/local/share/applications",
        "/usr/share/applications",
        "/var/lib/snapd/desktop/applications",
    ] {
        push_scan_dir(
            &mut dirs,
            &mut seen,
            PathBuf::from(path),
            DesktopOrigin::System,
        );
    }

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

    let requested_stem = desktop_id
        .strip_suffix(".desktop")
        .ok_or_else(|| anyhow!("desktop id must end with .desktop"))?;
    let entries = discover_desktop_entries(DesktopOrigin::All, None)?;
    let mut alias_hits_by_id: BTreeMap<String, DesktopListItem> = BTreeMap::new();
    for item in entries {
        let Some(stem) = item.desktop_id.strip_suffix(".desktop") else {
            continue;
        };
        let stem_match = stem.ends_with(&format!("_{requested_stem}"))
            || stem.starts_with(&format!("snap.{requested_stem}."));
        let exec_match =
            parse_first_exec_token(&item.exec).is_some_and(|bin| bin == requested_stem);
        let name_match = item.name.eq_ignore_ascii_case(requested_stem);
        if stem_match || exec_match || name_match {
            match alias_hits_by_id.get(&item.desktop_id) {
                Some(existing) if existing.origin == "user" => {}
                _ => {
                    alias_hits_by_id.insert(item.desktop_id.clone(), item);
                }
            }
        }
    }
    let alias_hits: Vec<DesktopListItem> = alias_hits_by_id.into_values().collect();

    if alias_hits.len() == 1 {
        let only = alias_hits
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("internal alias resolution error"))?;
        let source_path = PathBuf::from(&only.path);
        let source_content = fs::read_to_string(&source_path)
            .with_context(|| format!("failed to read desktop file {}", source_path.display()))?;
        let fields = parse_desktop_entry(&source_content);
        if fields.is_empty() {
            return Err(anyhow!(
                "desktop entry {} has no [Desktop Entry] group",
                source_path.display()
            ));
        }
        return Ok(DesktopSourceEntry {
            desktop_id: only.desktop_id,
            source_path,
            origin: if only.origin == "user" {
                DesktopOrigin::User
            } else {
                DesktopOrigin::System
            },
            source_content,
            fields,
        });
    }

    if !alias_hits.is_empty() {
        let suggestions = alias_hits
            .iter()
            .take(5)
            .map(|v| v.desktop_id.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(anyhow!(
            "desktop id '{}' not found exactly; multiple candidates found: {}",
            desktop_id,
            suggestions
        ));
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
    commands::desktop::handle_desktop_list(format, filter, origin)
}

fn command_exists_in_path(cmd: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|p| p.join(cmd).is_file())
}

fn handle_desktop_wrap(desktop_id: &str, class: &str, opts: DesktopWrapOptions) -> Result<i32> {
    commands::desktop::handle_desktop_wrap(desktop_id, class, opts)
}

fn handle_desktop_unwrap(desktop_id: &str, class: &str, opts: DesktopUnwrapOptions) -> Result<i32> {
    commands::desktop::handle_desktop_unwrap(desktop_id, class, opts)
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
    commands::desktop::handle_desktop_doctor()
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
    let json_log = json_log_enabled(&cli);
    let is_completion = matches!(&cli.command, Commands::Completion { .. });
    let is_version = matches!(&cli.command, Commands::Version);
    if !is_completion && !is_version {
        emit_log(json_log, "INFO", "cli.start", "starting command");
    }
    if !is_completion && !is_version {
        print_global_context(&cli);
    }
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
        Commands::Setup {
            name,
            apply,
            suggest,
        } => match handle_setup(
            &format,
            &root,
            &config_dir,
            &state_dir,
            name,
            apply,
            suggest,
        ) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("setup failed: {err}");
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
        Commands::Tui { interval, no_top } => match handle_tui(interval, no_top) {
            Ok(code) => {
                if code == 2 {
                    eprintln!("invalid arguments: --interval must be > 0");
                }
                code
            }
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
        Commands::Suggest {
            profile,
            apply,
            dry_run,
            confidence_threshold,
        } => match commands::suggest::handle_suggest(SuggestRequest {
            format: format.clone(),
            root: root.clone(),
            config_dir: config_dir.clone(),
            state_dir: state_dir.clone(),
            profile,
            apply,
            dry_run,
            confidence_threshold,
        }) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("suggest failed: {err}");
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
        Commands::Rescue {
            class,
            command,
            no_ui,
            no_check,
        } => match handle_rescue(
            &root,
            &config_dir,
            &state_dir,
            class,
            command,
            no_ui,
            no_check,
        ) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("rescue failed: {err}");
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
        Commands::Completion { shell } => match handle_completion(shell) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("completion failed: {err}");
                1
            }
        },
        Commands::Version => {
            print!("{}", cli_version_output());
            0
        }
    };

    if !is_completion && !is_version {
        emit_log(
            json_log,
            "INFO",
            "cli.exit",
            &format!("exit_code={exit_code}"),
        );
    }
    process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use resguard_core::parse_size_to_bytes;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tempfile::tempdir;

    static HOME_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct HomeEnvGuard {
        old_home: Option<std::ffi::OsString>,
        old_xdg_data_home: Option<std::ffi::OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl HomeEnvGuard {
        fn set(home: &Path) -> Self {
            let lock = HOME_ENV_LOCK.get_or_init(|| Mutex::new(()));
            let guard = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let old_home = std::env::var_os("HOME");
            let old_xdg_data_home = std::env::var_os("XDG_DATA_HOME");
            std::env::set_var("HOME", home);
            std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
            Self {
                old_home,
                old_xdg_data_home,
                _lock: guard,
            }
        }
    }

    impl Drop for HomeEnvGuard {
        fn drop(&mut self) {
            match self.old_home.take() {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match self.old_xdg_data_home.take() {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    struct DesktopEnvGuard {
        old_xdg_data_home: Option<std::ffi::OsString>,
        old_xdg_data_dirs: Option<std::ffi::OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl DesktopEnvGuard {
        fn set(
            xdg_data_home: Option<&std::path::Path>,
            xdg_data_dirs: Option<&std::ffi::OsStr>,
        ) -> Self {
            let lock = HOME_ENV_LOCK.get_or_init(|| Mutex::new(()));
            let guard = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let old_xdg_data_home = std::env::var_os("XDG_DATA_HOME");
            let old_xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS");
            match xdg_data_home {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match xdg_data_dirs {
                Some(v) => std::env::set_var("XDG_DATA_DIRS", v),
                None => std::env::remove_var("XDG_DATA_DIRS"),
            }
            Self {
                old_xdg_data_home,
                old_xdg_data_dirs,
                _lock: guard,
            }
        }
    }

    impl Drop for DesktopEnvGuard {
        fn drop(&mut self) {
            match self.old_xdg_data_home.take() {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match self.old_xdg_data_dirs.take() {
                Some(v) => std::env::set_var("XDG_DATA_DIRS", v),
                None => std::env::remove_var("XDG_DATA_DIRS"),
            }
        }
    }

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
        assert_eq!(
            classes["rescue"].slice_name.as_deref(),
            Some("resguard-rescue.slice")
        );
        assert_eq!(
            mem_to_bytes(
                classes["rescue"]
                    .memory_max
                    .as_deref()
                    .expect("rescue memoryMax")
            ),
            gb
        );
    }

    #[test]
    fn auto_profile_serializes_and_validates_with_rescue_class() {
        let profile = build_auto_profile("init-demo", 16 * 1024_u64.pow(3), 8);
        let yaml = serde_yaml::to_string(&profile).expect("serialize profile");
        assert!(yaml.contains("rescue:"));
        assert!(yaml.contains("resguard-rescue.slice"));

        let errors = validate_profile(&profile);
        assert!(errors.is_empty(), "validation errors: {:?}", errors);
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
    fn apply_writes_rescue_slice_units_for_auto_profile() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).expect("create root");
        seed_profile(&root, "rescue-prof");

        let code = handle_apply(
            root.to_str().expect("root str"),
            "/etc/resguard",
            "/var/lib/resguard",
            "rescue-prof",
            &test_apply_opts(),
        )
        .expect("apply result");
        assert_eq!(code, 0);

        let system_slice = root.join("etc/systemd/system/resguard-rescue.slice");
        let user_slice = root.join("etc/systemd/user/resguard-rescue.slice");
        assert!(system_slice.exists(), "missing {}", system_slice.display());
        assert!(user_slice.exists(), "missing {}", user_slice.display());
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
        let _home_guard = HomeEnvGuard::set(&home);

        let path = wrapper_path_for("firefox.desktop", "browsers").expect("wrapper path");
        assert!(path.starts_with(home.join(".local/share/applications")));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("firefox.desktop.resguard-browsers.desktop")
        );

        assert!(wrapper_path_for("../firefox.desktop", "browsers").is_err());
        assert!(wrapper_path_for("firefox.desktop", "bad/class").is_err());
    }

    #[test]
    fn wrap_exec_keeps_original_placeholders() {
        let wrapped = wrap_exec("firefox %u", "browsers");
        assert_eq!(wrapped, "resguard run --class browsers -- firefox %u");
    }

    #[test]
    fn rescue_command_defaults_to_htop_fallback_top() {
        let cmd = build_rescue_command("/bin/bash", None, false);
        assert_eq!(
            cmd,
            vec![
                "/bin/bash".to_string(),
                "-lc".to_string(),
                "htop || top".to_string()
            ]
        );
    }

    #[test]
    fn rescue_command_supports_no_ui_and_custom_command() {
        let no_ui_cmd = build_rescue_command("/bin/sh", None, true);
        assert_eq!(no_ui_cmd, vec!["/bin/sh".to_string()]);

        let custom = build_rescue_command("/bin/bash", Some("vmstat 1"), false);
        assert_eq!(
            custom,
            vec![
                "/bin/bash".to_string(),
                "-lc".to_string(),
                "vmstat 1".to_string()
            ]
        );
    }

    #[test]
    fn json_log_mode_flag_and_env_resolution() {
        assert!(json_log_enabled_from_env(true, None));
        assert!(json_log_enabled_from_env(false, Some("json")));
        assert!(json_log_enabled_from_env(false, Some("JSON")));
        assert!(!json_log_enabled_from_env(false, Some("table")));
        assert!(!json_log_enabled_from_env(false, None));
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
        assert_eq!(
            parse_snap_run_app("/usr/bin/snap run firefox").as_deref(),
            Some("firefox")
        );
        assert_eq!(
            parse_snap_run_app("env BAMF=1 /usr/bin/snap run --command=sh code").as_deref(),
            Some("code")
        );
        assert_eq!(
            parse_snap_app_from_scope("app-snap.firefox.firefox-1234.scope").as_deref(),
            Some("firefox")
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
        assert_eq!(got.map(|v| v.class), Some("browsers".to_string()));

        let got2 = classify_scope(
            "app-bar.scope",
            "app.slice",
            "podman run something",
            512 * 1024_u64.pow(2),
            &rules,
        );
        assert_eq!(got2.map(|v| v.class), Some("heavy".to_string()));
    }

    #[test]
    fn confidence_score_uses_all_signals() {
        let (s1, r1) = confidence_score(true, true, true);
        assert_eq!(s1, 100);
        assert!(r1.contains("pattern"));
        assert!(r1.contains("memory"));
        assert!(r1.contains("desktop-id"));

        let (s2, _) = confidence_score(true, false, true);
        assert_eq!(s2, 70);

        let (s3, _) = confidence_score(false, true, false);
        assert_eq!(s3, 30);
    }

    #[test]
    fn unique_desktop_id_resolution_requires_single_match() {
        let mut idx: HashMap<String, Vec<String>> = HashMap::new();
        idx.insert("firefox".to_string(), vec!["firefox.desktop".to_string()]);
        idx.insert(
            "code".to_string(),
            vec![
                "code.desktop".to_string(),
                "code-insiders.desktop".to_string(),
            ],
        );
        assert_eq!(
            unique_desktop_id_for_scope_exec("app-foo.scope", "/usr/bin/firefox %u", &idx)
                .as_deref(),
            Some("firefox.desktop")
        );
        assert!(
            unique_desktop_id_for_scope_exec("app-bar.scope", "code --new-window", &idx).is_none()
        );
    }

    #[test]
    fn unique_desktop_id_resolution_supports_snap_scope_exec() {
        let mut idx: HashMap<String, Vec<String>> = HashMap::new();
        idx.insert(
            "snap:firefox".to_string(),
            vec!["firefox_firefox.desktop".to_string()],
        );

        assert_eq!(
            unique_desktop_id_for_scope_exec(
                "app-snap.firefox.firefox-1234.scope",
                "/usr/bin/snap run firefox",
                &idx
            )
            .as_deref(),
            Some("firefox_firefox.desktop")
        );
    }

    #[test]
    fn desktop_scan_dirs_include_snap_and_xdg_paths() {
        let temp = tempdir().expect("tempdir");
        let xdg_home = temp.path().join("xdg-home");
        let _xdg_guard = DesktopEnvGuard::set(
            Some(&xdg_home),
            Some(std::ffi::OsStr::new("/opt/share:/custom/share")),
        );

        let dirs = desktop_scan_dirs();
        let paths = dirs.into_iter().map(|(p, _)| p).collect::<Vec<_>>();
        assert!(paths.contains(&xdg_home.join("applications")));
        assert!(paths.contains(&PathBuf::from("/opt/share/applications")));
        assert!(paths.contains(&PathBuf::from("/custom/share/applications")));
        assert!(paths.contains(&PathBuf::from("/var/lib/snapd/desktop/applications")));
    }

    #[test]
    fn resolve_desktop_source_allows_unambiguous_snap_alias() {
        let temp = tempdir().expect("tempdir");
        let xdg_home = temp.path().join("xdg-home");
        let _xdg_guard = DesktopEnvGuard::set(Some(&xdg_home), None);
        let apps = xdg_home.join("applications");
        std::fs::create_dir_all(&apps).expect("create app dir");
        std::fs::write(
            apps.join("resguard-testsnap_resguard-testapp.desktop"),
            "[Desktop Entry]\nType=Application\nName=Resguard Test App\nExec=/usr/bin/snap run resguard-testapp %u\n",
        )
        .expect("write desktop file");

        let source = resolve_desktop_source("resguard-testapp.desktop").expect("resolve source");
        assert_eq!(
            source.desktop_id,
            "resguard-testsnap_resguard-testapp.desktop"
        );
    }

    #[test]
    fn resolve_desktop_source_allows_firefox_snap_alias() {
        let temp = tempdir().expect("tempdir");
        let xdg_home = temp.path().join("xdg-home");
        let _xdg_guard = DesktopEnvGuard::set(Some(&xdg_home), None);
        let apps = xdg_home.join("applications");
        std::fs::create_dir_all(&apps).expect("create app dir");
        std::fs::write(
            apps.join("firefox_firefox.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox Web Browser\nExec=/snap/bin/firefox %u\n",
        )
        .expect("write firefox desktop file");

        let source = resolve_desktop_source("firefox.desktop").expect("resolve firefox alias");
        assert_eq!(source.desktop_id, "firefox_firefox.desktop");
    }

    #[test]
    fn resolve_desktop_source_rejects_ambiguous_alias() {
        let temp = tempdir().expect("tempdir");
        let xdg_home = temp.path().join("xdg-home");
        let _xdg_guard = DesktopEnvGuard::set(Some(&xdg_home), None);
        let apps = xdg_home.join("applications");
        std::fs::create_dir_all(&apps).expect("create app dir");
        std::fs::write(
            apps.join("resguard-testsnapa_resguard-ambig.desktop"),
            "[Desktop Entry]\nType=Application\nName=Resguard Ambig A\nExec=/usr/bin/snap run resguard-ambig %u\n",
        )
        .expect("write desktop file A");
        std::fs::write(
            apps.join("resguard-testsnapb_resguard-ambig.desktop"),
            "[Desktop Entry]\nType=Application\nName=Resguard Ambig B\nExec=/usr/bin/snap run resguard-ambig %u\n",
        )
        .expect("write desktop file B");

        let err = resolve_desktop_source("resguard-ambig.desktop").expect_err("ambiguous alias");
        assert!(err.to_string().contains("multiple candidates"));
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
    fn render_wrapper_forces_dbus_activatable_false() {
        let mut src = HashMap::new();
        src.insert("Name".to_string(), "Firefox".to_string());
        src.insert("Exec".to_string(), "/snap/bin/firefox %u".to_string());
        src.insert("Type".to_string(), "Application".to_string());
        src.insert("DBusActivatable".to_string(), "true".to_string());

        let wrapped = render_wrapper(&src, "browsers").expect("render wrapper");
        assert!(wrapped.contains("Exec=resguard run --class browsers -- /snap/bin/firefox %u\n"));
        assert!(wrapped.contains("DBusActivatable=false\n"));
        assert!(!wrapped.contains("DBusActivatable=true\n"));
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
        let _home_guard = HomeEnvGuard::set(&home);

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
        let _home_guard = HomeEnvGuard::set(&home);

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
        let _home_guard = HomeEnvGuard::set(&home);

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
    }
}
