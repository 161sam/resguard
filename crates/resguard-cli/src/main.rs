use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
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
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

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

    print_plan(&plan);
    if opts.dry_run {
        println!("result=dry-run");
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

fn main() {
    let cli = Cli::parse();
    print_global_context(&cli);
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
        Commands::Diff { profile } => {
            println!("command=diff");
            println!("profile={profile}");
            0
        }
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
}
