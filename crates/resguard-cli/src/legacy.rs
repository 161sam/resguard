use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, Shell};
use regex::Regex;
use resguard_config::{load_profile_from_store, profile_path, save_profile, validate_profile_file};
use resguard_core::validate_profile;
use resguard_discovery::{
    build_desktop_exec_index as discovery_build_desktop_exec_index,
    discover_desktop_entries as discovery_discover_desktop_entries,
    parse_first_exec_token as discovery_parse_first_exec_token,
    parse_scope_identity as discovery_parse_scope_identity,
    resolve_desktop_id as discovery_resolve_desktop_id,
    unique_desktop_id_for_scope_exec as discovery_unique_desktop_id_for_scope_exec,
    DesktopOrigin as DiscoveryOrigin, ResolutionResult as DiscoveryResolutionResult,
};
use resguard_model::{AppIdentity, Profile, SuggestRule, Suggestion, SuggestionReason};
use resguard_policy::{
    build_auto_profile as policy_build_auto_profile, classify as policy_classify,
    default_suggest_rules as policy_default_suggest_rules,
    meets_confidence_threshold as policy_meets_confidence_threshold, score as policy_score,
    validate_confidence_threshold as policy_validate_confidence_threshold, AutoProfileSnapshot,
    ClassMatch as PolicyClassMatch, ClassificationInput, ConfidenceSignals,
};
use resguard_runtime::{
    build_apply_plan, check_command_success, cpu_count, daemon_reload_if_root, execute_action,
    is_root_user, planned_write_changes, read_mem_total_bytes, read_meminfo_kb, read_pressure_1min,
    resolve_user_runtime_dir, systemctl_cat_unit, systemctl_is_active, systemctl_list_units,
    systemctl_service_action, systemctl_set_slice_memory_limits, systemctl_show_props, systemd_run,
    write_file, Action, PlanOptions,
};
#[cfg(feature = "tui")]
use resguard_runtime::{parse_prop_u64, read_mem_available_bytes, read_pressure};
use resguard_state::{
    begin_transaction, manifest_from_transaction, read_backup_manifest, read_state,
    rollback_from_manifest, snapshot_before_write, state_from_manifest, write_backup_manifest,
    write_state,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
#[cfg(feature = "tui")]
use std::io;
#[cfg(feature = "tui")]
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;
#[cfg(feature = "tui")]
use std::time::Instant;
use std::{collections::HashMap, fs};

use crate::commands;
use crate::util;
pub(crate) use util::system::{
    format_bytes_human, list_system_slices, parse_u64_prop, partial_exit_code,
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
pub(crate) struct Cli {
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
pub(crate) enum Commands {
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
        #[arg(
            long,
            help = "Resource class (recommended). If omitted, strong auto-detect is used."
        )]
        class: Option<String>,
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
pub(crate) enum ProfileCmd {
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
pub(crate) enum DesktopCmd {
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
pub(crate) enum DaemonCmd {
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CompletionShell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum DesktopOrigin {
    User,
    System,
    All,
}

#[derive(Debug)]
pub(crate) struct ApplyOptions {
    dry_run: bool,
    no_oomd: bool,
    no_cpu: bool,
    no_classes: bool,
    force: bool,
    user_daemon_reload: bool,
}

#[derive(Debug)]
pub(crate) struct RunRequest {
    pub(crate) class: Option<String>,
    pub(crate) profile_override: Option<String>,
    pub(crate) slice_override: Option<String>,
    pub(crate) no_check: bool,
    pub(crate) wait: bool,
    pub(crate) command: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct SuggestRequest {
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
pub(crate) struct DesktopWrapOptions {
    pub(crate) force: bool,
    pub(crate) dry_run: bool,
    pub(crate) print_only: bool,
    pub(crate) override_mode: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DesktopUnwrapOptions {
    pub(crate) override_mode: bool,
}

pub(crate) fn print_global_context(cli: &Cli) {
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

pub(crate) fn json_log_enabled(cli: &Cli) -> bool {
    let env_val = env::var("RESGUARD_LOG").ok();
    json_log_enabled_from_env(cli.json_log, env_val.as_deref())
}

pub(crate) fn json_log_enabled_from_env(flag: bool, env_value: Option<&str>) -> bool {
    flag || env_value.is_some_and(|v| v.eq_ignore_ascii_case("json"))
}

pub(crate) fn emit_log(json_log: bool, level: &str, event: &str, message: &str) {
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

pub(crate) fn handle_completion(shell: CompletionShell) -> Result<i32> {
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

pub(crate) fn cli_version_output() -> String {
    let cmd = Cli::command();
    cmd.render_version().to_string()
}

#[cfg(test)]
pub(crate) fn round_down_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    (value / step) * step
}

#[cfg(test)]
pub(crate) fn round_up_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    value.div_ceil(step) * step
}

pub(crate) fn build_auto_profile(name: &str, total_mem_bytes: u64, cpu_cores: u32) -> Profile {
    policy_build_auto_profile(
        name,
        AutoProfileSnapshot {
            total_mem_bytes,
            cpu_cores,
        },
    )
}

pub(crate) fn resolve_with_root(root: &str, path: PathBuf) -> Result<PathBuf> {
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

pub(crate) fn print_plan(actions: &[Action]) {
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

pub(crate) fn handle_apply(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    profile_name: &str,
    opts: &crate::cli::ApplyOptions,
) -> Result<i32> {
    commands::apply::handle_apply(root, config_dir, state_dir, profile_name, opts)
}

pub(crate) fn handle_diff(root: &str, config_dir: &str, profile_name: &str) -> Result<i32> {
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

pub(crate) fn handle_init(
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
        let apply_opts = crate::cli::ApplyOptions {
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

pub(crate) fn handle_setup(
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

pub(crate) fn handle_rollback(
    root: &str,
    state_dir: &str,
    last: bool,
    to: Option<String>,
) -> Result<i32> {
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
    daemon_reload_if_root(root)?;

    write_state(&rooted_state_dir, &resguard_state::State::default())?;
    println!("result=ok");
    Ok(0)
}

pub(crate) fn resolve_class_slice(profile: &Profile, class_name: &str) -> Option<String> {
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
pub(crate) fn build_rescue_command(
    shell: &str,
    custom_command: Option<&str>,
    no_ui: bool,
) -> Vec<String> {
    commands::rescue::build_rescue_command(shell, custom_command, no_ui)
}

pub(crate) fn handle_run(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    req: RunRequest,
) -> Result<i32> {
    commands::run::run(root, config_dir, state_dir, req)
}

pub(crate) fn handle_rescue(
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

pub(crate) fn systemctl_user_scope_units() -> Result<Vec<String>> {
    Ok(systemctl_list_units(true, "scope")?
        .into_iter()
        .filter(|unit| unit.ends_with(".scope"))
        .collect())
}

pub(crate) fn systemctl_user_show_scope(scope: &str) -> Result<BTreeMap<String, String>> {
    systemctl_show_props(
        true,
        scope,
        &["MemoryCurrent", "CPUUsageNSec", "Slice", "ExecStart", "Id"],
    )
}

pub(crate) fn parse_first_exec_token(exec: &str) -> Option<String> {
    discovery_parse_first_exec_token(exec)
}

pub(crate) fn build_desktop_exec_index() -> HashMap<String, Vec<String>> {
    discovery_build_desktop_exec_index()
}

pub(crate) fn unique_desktop_id_for_scope_exec(
    scope: &str,
    exec_start: &str,
    desktop_by_exec: &HashMap<String, Vec<String>>,
) -> Option<String> {
    discovery_unique_desktop_id_for_scope_exec(scope, exec_start, desktop_by_exec)
}

#[derive(Debug, Clone)]
pub(crate) struct SuggestClassification {
    class: String,
    reason: String,
    pattern_match: bool,
    memory_threshold_match: bool,
}

pub(crate) fn default_suggest_rules() -> Vec<SuggestRule> {
    policy_default_suggest_rules()
}

pub(crate) fn classify_scope(
    unit: &str,
    slice: &str,
    exec_start: &str,
    memory_current: u64,
    rules: &[SuggestRule],
) -> Option<SuggestClassification> {
    policy_classify(
        &ClassificationInput {
            scope: unit.to_string(),
            slice: slice.to_string(),
            exec_start: exec_start.to_string(),
            memory_current,
        },
        rules,
    )
    .map(|m: PolicyClassMatch| SuggestClassification {
        class: m.class,
        reason: m.reason,
        pattern_match: m.pattern_match,
        memory_threshold_match: m.memory_threshold_match,
    })
}

#[cfg(test)]
pub(crate) fn strong_app_identity_match(scope: &str, exec_start: &str, class: &str) -> bool {
    let identity = discovery_parse_scope_identity(scope, exec_start);
    resguard_policy::strong_identity_match(&identity, class)
}

pub(crate) fn confidence_score(
    identity: &AppIdentity,
    class: &str,
    pattern_match: bool,
    memory_threshold_match: bool,
    known_desktop_id: bool,
) -> (u8, String) {
    let scored = policy_score(
        identity,
        &ConfidenceSignals {
            pattern_match,
            memory_threshold_match,
            known_desktop_id,
            class: class.to_string(),
        },
    );
    (scored.score, scored.reason)
}

pub(crate) fn resolve_suggest_profile(
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

pub(crate) fn suggestion_reason_text(reason: &SuggestionReason) -> String {
    match reason {
        SuggestionReason::PatternRule => "pattern-rule".to_string(),
        SuggestionReason::MemoryThreshold => "memory-threshold".to_string(),
        SuggestionReason::StrongIdentity => "strong-identity".to_string(),
        SuggestionReason::DesktopIdMatch => "desktop-id-match".to_string(),
        SuggestionReason::Manual { message } => message.clone(),
    }
}

pub(crate) fn print_suggestions_table(suggestions: &[Suggestion]) {
    println!("scope\tclass\tconfidence\tdesktop_id\tmemory\treason");
    for s in suggestions {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            s.scope,
            s.class,
            s.confidence,
            s.desktop_id.as_deref().unwrap_or("-"),
            format_bytes_human(s.memory_current),
            suggestion_reason_text(&s.reason)
        );
    }
}

pub(crate) fn status_value(props: &BTreeMap<String, String>, key: &str) -> String {
    props
        .get(key)
        .filter(|v| !v.is_empty())
        .cloned()
        .unwrap_or_else(|| "-".to_string())
}

fn status_slice_line(scope: &str, unit: &str, props: &BTreeMap<String, String>) -> String {
    format!(
        "slice\tscope={scope}\tunit={unit}\tMemoryLow={}\tMemoryHigh={}\tMemoryMax={}\tAllowedCPUs={}",
        status_value(props, "MemoryLow"),
        status_value(props, "MemoryHigh"),
        status_value(props, "MemoryMax"),
        status_value(props, "AllowedCPUs")
    )
}

pub(crate) fn collect_class_slices_from_state(state: &resguard_state::State) -> Vec<String> {
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

pub(crate) fn handle_status(root: &str, state_dir: &str) -> Result<i32> {
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
    println!("\n== Status: Slice Limits ==");

    let keys = ["MemoryHigh", "MemoryMax", "MemoryLow", "AllowedCPUs"];
    for unit in ["system.slice", "user.slice"] {
        match systemctl_show_props(false, unit, &keys) {
            Ok(props) => {
                println!("{}", status_slice_line("system", unit, &props));
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
                println!("{}", status_slice_line("system", slice, &props));
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
                "{}",
                status_slice_line("user", "resguard-browsers.slice", &props)
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

    println!("\n== Status: Runtime Signals ==");
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
        println!("\n== Status: Hints ==");
        println!("hint=if slices were updated recently run: systemctl --user daemon-reload");
        println!("hint=then verify: resguard doctor && resguard metrics");
    }

    if partial {
        Ok(1)
    } else {
        Ok(0)
    }
}

pub(crate) fn handle_doctor(root: &str, state_dir: &str) -> Result<i32> {
    commands::doctor::handle_doctor(root, state_dir)
}

pub(crate) fn handle_daemon_enable() -> Result<i32> {
    commands::daemon::handle_daemon_enable()
}

pub(crate) fn handle_daemon_disable() -> Result<i32> {
    commands::daemon::handle_daemon_disable()
}

pub(crate) fn handle_daemon_status() -> Result<i32> {
    commands::daemon::handle_daemon_status()
}

pub(crate) fn handle_metrics() -> Result<i32> {
    commands::metrics::handle_metrics()
}

#[cfg(feature = "tui")]
pub(crate) fn handle_tui(interval_ms: u64, no_top: bool) -> Result<i32> {
    commands::tui::handle_tui("/etc/resguard", "/var/lib/resguard", interval_ms, no_top)
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DesktopListItem {
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
pub(crate) struct DesktopSourceEntry {
    pub(crate) desktop_id: String,
    pub(crate) source_path: PathBuf,
    pub(crate) origin: DesktopOrigin,
    pub(crate) source_content: String,
    pub(crate) fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DesktopMappingEntry {
    pub(crate) wrapper_desktop_id: String,
    pub(crate) wrapper_path: String,
    pub(crate) source_path: String,
    pub(crate) created_at: String,
    pub(crate) mode: Option<String>,
    pub(crate) backup_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DesktopMappingStore {
    pub(crate) version: u32,
    pub(crate) mappings: BTreeMap<String, BTreeMap<String, DesktopMappingEntry>>,
}

impl Default for DesktopMappingStore {
    fn default() -> Self {
        Self {
            version: 1,
            mappings: BTreeMap::new(),
        }
    }
}

pub(crate) fn parse_desktop_entry(s: &str) -> HashMap<String, String> {
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

pub(crate) fn validate_desktop_id(id: &str) -> Result<()> {
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

pub(crate) fn validate_class_name(class: &str) -> Result<()> {
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

pub(crate) fn wrapper_desktop_id(desktop_id: &str, class: &str) -> String {
    format!("{desktop_id}.resguard-{class}.desktop")
}

pub(crate) fn validate_wrapper_filename(name: &str) -> Result<()> {
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

pub(crate) fn wrapper_path_for(desktop_id: &str, class: &str) -> Result<PathBuf> {
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

pub(crate) fn override_path_for(desktop_id: &str) -> Result<PathBuf> {
    validate_desktop_id(desktop_id)?;
    Ok(user_applications_dir()?.join(desktop_id))
}

pub(crate) fn wrap_exec(exec: &str, class: &str) -> String {
    format!("resguard run --class {class} -- {}", exec.trim())
}

pub(crate) fn render_wrapper(source: &HashMap<String, String>, class: &str) -> Result<String> {
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

pub(crate) fn user_home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME is not set"))
}

pub(crate) fn user_applications_dir() -> Result<PathBuf> {
    Ok(user_home_dir()?.join(".local/share/applications"))
}

pub(crate) fn desktop_mapping_path() -> Result<PathBuf> {
    Ok(user_home_dir()?.join(".config/resguard/desktop-mapping.yml"))
}

pub(crate) fn read_desktop_mapping_store() -> Result<DesktopMappingStore> {
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

pub(crate) fn write_desktop_mapping_store(store: &DesktopMappingStore) -> Result<()> {
    let path = desktop_mapping_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_yaml::to_string(store)?;
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(crate) fn now_timestamp_utc() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("unix:{}", d.as_secs()),
        Err(_) => "unix:0".to_string(),
    }
}

pub(crate) fn now_timestamp_for_path() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("unix-{}", d.as_secs()),
        Err(_) => "unix-0".to_string(),
    }
}

pub(crate) fn create_override_backup(path: &Path) -> Result<PathBuf> {
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

pub(crate) fn render_line_diff(
    source_label: &str,
    source: &str,
    target_label: &str,
    target: &str,
) -> String {
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

pub(crate) fn short_exec(exec: &str) -> String {
    let max = 80usize;
    if exec.chars().count() <= max {
        return exec.to_string();
    }
    exec.chars().take(max - 3).collect::<String>() + "..."
}

pub(crate) fn resolve_desktop_source(desktop_id: &str) -> Result<DesktopSourceEntry> {
    validate_desktop_id(desktop_id)?;
    let resolved = match discovery_resolve_desktop_id(desktop_id) {
        DiscoveryResolutionResult::Exact(entry) => entry,
        DiscoveryResolutionResult::Alias { resolved, .. } => resolved,
        DiscoveryResolutionResult::Ambiguous { candidates, .. } => {
            return Err(anyhow!(
                "desktop id '{}' not found exactly; multiple candidates found: {}",
                desktop_id,
                candidates.join(", ")
            ));
        }
        DiscoveryResolutionResult::NotFound { .. } => {
            return Err(anyhow!(
                "desktop id '{}' not found in XDG search paths",
                desktop_id
            ));
        }
    };

    Ok(DesktopSourceEntry {
        desktop_id: resolved.desktop_id,
        source_path: PathBuf::from(resolved.path),
        origin: match resolved.origin {
            DiscoveryOrigin::User => DesktopOrigin::User,
            DiscoveryOrigin::System => DesktopOrigin::System,
            DiscoveryOrigin::All => DesktopOrigin::All,
        },
        source_content: resolved.source_content,
        fields: resolved.fields.into_iter().collect(),
    })
}

pub(crate) fn discover_desktop_entries(
    origin_filter: DesktopOrigin,
    name_filter: Option<&Regex>,
) -> Result<Vec<DesktopListItem>> {
    let d_origin = match origin_filter {
        DesktopOrigin::User => DiscoveryOrigin::User,
        DesktopOrigin::System => DiscoveryOrigin::System,
        DesktopOrigin::All => DiscoveryOrigin::All,
    };

    let mut items = Vec::new();
    for entry in discovery_discover_desktop_entries(d_origin) {
        if let Some(re) = name_filter {
            let hay = format!("{} {} {}", entry.desktop_id, entry.name, entry.exec);
            if !re.is_match(&hay) {
                continue;
            }
        }
        let fields = entry.fields;
        items.push(DesktopListItem {
            desktop_id: entry.desktop_id,
            name: entry.name,
            exec: entry.exec,
            icon: fields.get("Icon").cloned(),
            try_exec: fields.get("TryExec").cloned(),
            terminal: fields.get("Terminal").cloned(),
            entry_type: fields.get("Type").cloned(),
            path: entry.path,
            origin: match entry.origin {
                DiscoveryOrigin::User => "user".to_string(),
                DiscoveryOrigin::System => "system".to_string(),
                DiscoveryOrigin::All => "all".to_string(),
            },
            fields,
        });
    }
    items.sort_by(|a, b| {
        a.desktop_id
            .cmp(&b.desktop_id)
            .then(a.origin.cmp(&b.origin))
    });
    Ok(items)
}

pub(crate) fn print_desktop_table(items: &[DesktopListItem]) {
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

pub(crate) fn handle_desktop_list(
    format: &str,
    filter: Option<String>,
    origin: DesktopOrigin,
) -> Result<i32> {
    let mapped = match origin {
        DesktopOrigin::User => crate::cli::DesktopOrigin::User,
        DesktopOrigin::System => crate::cli::DesktopOrigin::System,
        DesktopOrigin::All => crate::cli::DesktopOrigin::All,
    };
    commands::desktop::handle_desktop_list(format, filter, mapped)
}

pub(crate) fn command_exists_in_path(cmd: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|p| p.join(cmd).is_file())
}

pub(crate) fn handle_desktop_wrap(
    desktop_id: &str,
    class: &str,
    opts: DesktopWrapOptions,
) -> Result<i32> {
    commands::desktop::handle_desktop_wrap(desktop_id, class, opts)
}

pub(crate) fn handle_desktop_unwrap(
    desktop_id: &str,
    class: &str,
    opts: DesktopUnwrapOptions,
) -> Result<i32> {
    commands::desktop::handle_desktop_unwrap(desktop_id, class, opts)
}

pub(crate) fn validate_wrapper_file(path: &Path) -> Result<()> {
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

pub(crate) fn run_desktop_doctor_checks(
    print_command: bool,
    require_mapping: bool,
) -> Result<(bool, bool)> {
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
        for hint in desktop_launcher_refresh_hints() {
            println!("fix: {hint}");
        }
    }

    Ok((partial, has_mappings))
}

pub(crate) fn desktop_launcher_refresh_hints() -> &'static [&'static str] {
    &[
        "systemctl --user daemon-reload",
        "update-desktop-database \"$HOME/.local/share/applications\"   # optional, if available",
        "gtk-update-icon-cache \"$HOME/.local/share/icons\"   # optional, if used",
        "log out and log back in (or reboot) to refresh launcher cache",
    ]
}

pub(crate) fn handle_desktop_doctor() -> Result<i32> {
    commands::desktop::handle_desktop_doctor()
}

pub(crate) fn handle_panic(root: &str, duration: Option<String>) -> Result<i32> {
    resguard_services::panic_service::panic_mode(root, duration)
}

#[allow(dead_code)]
pub(crate) fn legacy_main_disabled() {}
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

    fn test_apply_opts() -> crate::cli::ApplyOptions {
        crate::cli::ApplyOptions {
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
            resguard_discovery::parse_snap_run_app("/usr/bin/snap run firefox").as_deref(),
            Some("firefox")
        );
        assert_eq!(
            resguard_discovery::parse_snap_run_app(
                "env BAMF=1 /usr/bin/snap run --command=sh code"
            )
            .as_deref(),
            Some("code")
        );
        assert_eq!(
            resguard_discovery::parse_snap_app_from_scope("app-snap.firefox.firefox-1234.scope")
                .as_deref(),
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
        let id_firefox = AppIdentity {
            executable: Some("firefox".to_string()),
            snap_app: Some("firefox".to_string()),
            desktop_id: None,
        };
        let id_unknown = AppIdentity {
            executable: Some("unknown".to_string()),
            snap_app: None,
            desktop_id: None,
        };

        let (s1, r1) = confidence_score(&id_firefox, "browsers", true, true, true);
        assert_eq!(s1, 100);
        assert!(r1.contains("pattern"));
        assert!(r1.contains("memory"));
        assert!(r1.contains("desktop-id"));
        assert!(r1.contains("identity"));

        let (s2, _) = confidence_score(&id_unknown, "heavy", true, false, true);
        assert_eq!(s2, 70);

        let (s3, _) = confidence_score(&id_unknown, "heavy", false, true, false);
        assert_eq!(s3, 30);
    }

    #[test]
    fn confidence_score_boosts_common_snap_firefox_identity() {
        let class = classify_scope(
            "app-snap.firefox.firefox-1234.scope",
            "app.slice",
            "/usr/bin/snap run firefox",
            300 * 1024_u64.pow(2),
            &default_suggest_rules(),
        )
        .expect("classified firefox");
        assert_eq!(class.class, "browsers");
        let strong = strong_app_identity_match(
            "app-snap.firefox.firefox-1234.scope",
            "/usr/bin/snap run firefox",
            &class.class,
        );
        assert!(strong);
        let identity = discovery_parse_scope_identity(
            "app-snap.firefox.firefox-1234.scope",
            "/usr/bin/snap run firefox",
        );
        let (score, reason) =
            confidence_score(&identity, &class.class, class.pattern_match, false, false);
        assert_eq!(score, 70);
        assert!(reason.contains("pattern"));
        assert!(reason.contains("identity"));
    }

    #[test]
    fn confidence_score_boosts_common_snap_code_identity() {
        let class = classify_scope(
            "app-snap.code.code-42.scope",
            "app.slice",
            "/usr/bin/snap run code --new-window",
            300 * 1024_u64.pow(2),
            &default_suggest_rules(),
        )
        .expect("classified code");
        assert_eq!(class.class, "ide");
        let strong = strong_app_identity_match(
            "app-snap.code.code-42.scope",
            "/usr/bin/snap run code --new-window",
            &class.class,
        );
        assert!(strong);
        let identity = discovery_parse_scope_identity(
            "app-snap.code.code-42.scope",
            "/usr/bin/snap run code --new-window",
        );
        let (score, _) =
            confidence_score(&identity, &class.class, class.pattern_match, false, false);
        assert_eq!(score, 70);
    }

    #[test]
    fn confidence_score_keeps_weak_or_ambiguous_matches_low() {
        let class = classify_scope(
            "app-random.scope",
            "app.slice",
            "/usr/bin/firefox --private-window",
            256 * 1024_u64.pow(2),
            &default_suggest_rules(),
        )
        .expect("classified weak firefox");
        assert_eq!(class.class, "browsers");
        let strong =
            strong_app_identity_match("app-random.scope", "/usr/bin/unknown-browser", &class.class);
        assert!(!strong);
        let identity =
            discovery_parse_scope_identity("app-random.scope", "/usr/bin/unknown-browser");
        let (score, reason) =
            confidence_score(&identity, &class.class, class.pattern_match, false, false);
        assert_eq!(score, 40);
        assert!(reason.contains("pattern"));
        assert!(!reason.contains("identity"));
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

        let dirs = resguard_discovery::desktop_scan_dirs()
            .into_iter()
            .map(|(path, origin)| {
                (
                    path,
                    match origin {
                        DiscoveryOrigin::User => DesktopOrigin::User,
                        DiscoveryOrigin::System => DesktopOrigin::System,
                        DiscoveryOrigin::All => DesktopOrigin::All,
                    },
                )
            })
            .collect::<Vec<_>>();
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
    fn render_wrapper_for_snap_firefox_keeps_exec_path() {
        let mut src = HashMap::new();
        src.insert("Name".to_string(), "Firefox Web Browser".to_string());
        src.insert("Exec".to_string(), "/snap/bin/firefox %u".to_string());
        src.insert("Type".to_string(), "Application".to_string());
        src.insert("Icon".to_string(), "firefox".to_string());
        src.insert("StartupWMClass".to_string(), "firefox".to_string());

        let wrapped = render_wrapper(&src, "browsers").expect("render wrapper");
        assert!(wrapped.contains("Name=Firefox Web Browser (Resguard: browsers)\n"));
        assert!(wrapped.contains("Exec=resguard run --class browsers -- /snap/bin/firefox %u\n"));
        assert!(wrapped.contains("Icon=firefox\n"));
        assert!(wrapped.contains("StartupWMClass=firefox\n"));
    }

    #[test]
    fn desktop_launcher_refresh_hints_include_actionable_steps() {
        let hints = desktop_launcher_refresh_hints();
        assert!(hints
            .iter()
            .any(|hint| hint.contains("systemctl --user daemon-reload")));
        assert!(hints
            .iter()
            .any(|hint| hint.contains("update-desktop-database")));
        assert!(hints
            .iter()
            .any(|hint| hint.contains("log out and log back in")));
    }

    #[test]
    fn status_slice_line_is_readable_and_script_friendly() {
        let mut props = BTreeMap::new();
        props.insert("MemoryLow".to_string(), "1G".to_string());
        props.insert("MemoryHigh".to_string(), "2G".to_string());
        props.insert("MemoryMax".to_string(), "3G".to_string());
        props.insert("AllowedCPUs".to_string(), "0-7".to_string());
        let line = status_slice_line("system", "user.slice", &props);
        assert!(line.starts_with("slice\tscope=system\tunit=user.slice\t"));
        assert!(line.contains("MemoryHigh=2G"));
        assert!(line.contains("AllowedCPUs=0-7"));
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
        let source_path = apps.join("firefox_firefox.desktop");
        let original =
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/snap/bin/firefox %u\n";
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
        assert!(wrapped.contains("Exec=resguard run --class browsers -- "));
        assert!(wrapped.contains("%u"));

        let unwrap_code = handle_desktop_unwrap(
            "firefox_firefox.desktop",
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
