use anyhow::{anyhow, Result};
use resguard_config::load_profile_from_store;
use resguard_core::validate_profile;
use resguard_runtime::{
    build_apply_plan, daemon_reload_if_root, execute_action, is_root_user, planned_write_changes,
    resolve_user_runtime_dir, Action, PlanOptions,
};
use resguard_state::{
    begin_transaction, manifest_from_transaction, rollback_from_manifest, snapshot_before_write,
    state_from_manifest, write_backup_manifest, write_state,
};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ApplyRequest {
    pub root: String,
    pub config_dir: String,
    pub state_dir: String,
    pub profile_name: String,
    pub dry_run: bool,
    pub no_oomd: bool,
    pub no_cpu: bool,
    pub no_classes: bool,
    pub force: bool,
    pub user_daemon_reload: bool,
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
            .map_err(|_| anyhow!("failed to strip leading slash"))?;
        Ok(root_path.join(rel))
    } else {
        Ok(path)
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

pub fn apply(req: &ApplyRequest) -> Result<i32> {
    println!("command=apply");
    println!(
        "profile={} dry_run={} no_oomd={} no_cpu={} no_classes={} force={} user_daemon_reload={}",
        req.profile_name,
        req.dry_run,
        req.no_oomd,
        req.no_cpu,
        req.no_classes,
        req.force,
        req.user_daemon_reload
    );

    if !req.dry_run && req.root == "/" && !is_root_user()? {
        return Ok(3);
    }

    let rooted_config_dir = resolve_with_root(&req.root, PathBuf::from(&req.config_dir))?;
    let rooted_state_dir = resolve_with_root(&req.root, PathBuf::from(&req.state_dir))?;
    let profile =
        load_profile_from_store(&rooted_config_dir, &req.profile_name).map_err(|err| {
            anyhow!(
                "failed to load profile '{}' from {}: {err}",
                req.profile_name,
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

    if req.user_daemon_reload && req.root == "/" {
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
    } else if req.user_daemon_reload && req.root != "/" {
        println!("hint=--user-daemon-reload skipped because --root is not '/' (test root mode)");
    }

    let plan = build_apply_plan(
        &profile,
        Path::new(&req.root),
        &PlanOptions {
            no_oomd: req.no_oomd,
            no_cpu: req.no_cpu,
            no_classes: req.no_classes,
            user_daemon_reload: req.user_daemon_reload,
            sudo_user,
            sudo_runtime_dir,
        },
    );
    let changed_writes = planned_write_changes(&plan)?;

    print_plan(&plan);
    println!("plan_write_changes={}", changed_writes.len());
    if req.dry_run {
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
            Action::WriteFile { path, .. } => {
                snapshot_before_write(&mut tx, path, Path::new(&req.root))
                    .and_then(|_| execute_action(action))
            }
            _ => execute_action(action),
        };

        if let Err(err) = step {
            eprintln!("apply failed: {err}");
            let failure_manifest = manifest_from_transaction(&tx, Some(req.profile_name.clone()));
            let rollback_result =
                rollback_from_manifest(Path::new(&req.root), &rooted_state_dir, &failure_manifest)
                    .and_then(|_| daemon_reload_if_root(&req.root));
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

    let manifest = manifest_from_transaction(&tx, Some(req.profile_name.clone()));
    write_backup_manifest(&rooted_state_dir, &manifest)?;
    write_state(&rooted_state_dir, &state_from_manifest(&manifest))?;

    println!("result=ok");
    Ok(0)
}
