#![allow(dead_code, unused_imports)]

use anyhow::{anyhow, Context, Result};
use clap::CommandFactory;
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
use std::path::{Path, PathBuf};
use std::process;
#[cfg(feature = "tui")]
use std::time::{Duration, Instant};
use std::{collections::HashMap, fs};

mod cli;
mod commands;
mod legacy;
mod output;
mod util;

use crate::cli::Commands as CliCommands;
use crate::legacy::*;
pub(crate) use util::system::{
    format_bytes_human, list_system_slices, parse_u64_prop, partial_exit_code,
};

fn main() {
    let cli = cli::parse();
    let is_completion = matches!(&cli.command, CliCommands::Completion { .. });
    let is_version = matches!(&cli.command, CliCommands::Version);

    if !is_completion && !is_version {
        println!(
            "format={} json_log={} verbose={} quiet={} no_color={} root={} config_dir={} state_dir={}",
            cli.format, cli.json_log, cli.verbose, cli.quiet, cli.no_color, cli.root, cli.config_dir, cli.state_dir
        );
    }

    let run = || -> Result<i32> {
        match cli.command {
            CliCommands::Init {
                name,
                out,
                apply,
                dry_run,
            } => handle_init(
                &cli.root,
                &cli.config_dir,
                &cli.state_dir,
                name,
                out,
                apply,
                dry_run,
            ),
            CliCommands::Setup {
                name,
                apply,
                suggest,
                plan_wraps,
            } => commands::setup::run(
                &cli.format,
                &cli.root,
                &cli.config_dir,
                &cli.state_dir,
                name,
                apply,
                suggest,
                plan_wraps,
            ),
            CliCommands::Apply {
                profile,
                dry_run,
                no_oomd,
                no_cpu,
                no_classes,
                force,
                user_daemon_reload,
            } => commands::apply::run(
                &cli.root,
                &cli.config_dir,
                &cli.state_dir,
                profile,
                crate::cli::ApplyOptions {
                    dry_run,
                    no_oomd,
                    no_cpu,
                    no_classes,
                    force,
                    user_daemon_reload,
                },
            ),
            CliCommands::Diff { profile } => handle_diff(&cli.root, &cli.config_dir, &profile),
            CliCommands::Rollback { last, to } => {
                commands::rollback::run(&cli.root, &cli.state_dir, last, to)
            }
            CliCommands::Doctor => commands::doctor::run(&cli.root, &cli.state_dir),
            CliCommands::Metrics => commands::metrics::run(),
            CliCommands::Top { scopes, plain } => commands::top::run(
                &cli.format,
                &cli.config_dir,
                &cli.state_dir,
                scopes,
                plain,
                cli.no_color,
            ),
            #[cfg(feature = "tui")]
            CliCommands::Tui { interval, no_top } => {
                commands::tui::handle_tui(&cli.config_dir, &cli.state_dir, interval, no_top)
            }
            CliCommands::Panic { duration } => commands::panic::run(&cli.root, duration),
            CliCommands::Status => commands::status::run(&cli.root, &cli.state_dir),
            CliCommands::Suggest {
                profile,
                apply,
                dry_run,
                confidence_threshold,
            } => commands::suggest::run(crate::cli::SuggestRequest {
                format: cli.format.clone(),
                root: cli.root.clone(),
                config_dir: cli.config_dir.clone(),
                state_dir: cli.state_dir.clone(),
                profile,
                apply,
                dry_run,
                confidence_threshold,
            }),
            CliCommands::Run {
                class,
                profile,
                slice,
                no_check,
                wait,
                command,
            } => commands::run::run(
                &cli.root,
                &cli.config_dir,
                &cli.state_dir,
                RunRequest {
                    class,
                    profile_override: profile,
                    slice_override: slice,
                    no_check,
                    wait,
                    command,
                },
            ),
            CliCommands::Rescue {
                class,
                command,
                no_ui,
                no_check,
            } => commands::rescue::run(
                &cli.root,
                &cli.config_dir,
                &cli.state_dir,
                class,
                command,
                no_ui,
                no_check,
            ),
            CliCommands::Profile { cmd } => commands::profile::run(&cli.config_dir, cmd),
            CliCommands::Desktop { cmd } => commands::desktop::run(&cli.format, cmd),
            CliCommands::Daemon { cmd } => commands::daemon::run(cmd),
            CliCommands::Completion { shell } => commands::version::completion(shell),
            CliCommands::Version => commands::version::run(),
        }
    };

    let code = match run() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("command failed: {err}");
            1
        }
    };
    process::exit(code);
}
