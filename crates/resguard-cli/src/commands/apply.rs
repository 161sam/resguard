use crate::*;

pub(crate) fn handle_apply(
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
                    .and_then(|_| daemon_reload_if_root(root));
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
