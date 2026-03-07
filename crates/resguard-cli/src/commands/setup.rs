use crate::*;

pub(crate) fn handle_setup(
    format: &str,
    root: &str,
    config_dir: &str,
    state_dir: &str,
    name: Option<String>,
    apply: bool,
    suggest: bool,
) -> Result<i32> {
    println!("command=setup");
    let profile_name = name.unwrap_or_else(|| "auto".to_string());
    println!(
        "profile={} apply={} suggest={}",
        profile_name, apply, suggest
    );

    let setup_out = profile_path(config_dir, &profile_name)?
        .to_string_lossy()
        .to_string();
    let init_code = handle_init(
        root,
        config_dir,
        state_dir,
        Some(profile_name.clone()),
        Some(setup_out),
        false,
        false,
    )?;
    if init_code != 0 {
        return Ok(init_code);
    }

    let mut apply_code = 0;
    if apply {
        apply_code = handle_apply(
            root,
            config_dir,
            state_dir,
            &profile_name,
            &ApplyOptions {
                dry_run: false,
                no_oomd: false,
                no_cpu: false,
                no_classes: false,
                force: false,
                user_daemon_reload: false,
            },
        )?;
        if apply_code != 0 {
            println!("setup.result=partial");
            println!("setup.apply_exit={}", apply_code);
            println!("rollback_hint=resguard rollback --last");
            return Ok(apply_code);
        }
    }

    if suggest {
        println!("setup.suggest_preview=begin");
        let _ = commands::suggest::handle_suggest(SuggestRequest {
            format: format.to_string(),
            root: root.to_string(),
            config_dir: config_dir.to_string(),
            state_dir: state_dir.to_string(),
            profile: Some(profile_name.clone()),
            apply: false,
            dry_run: true,
            confidence_threshold: 70,
        })?;
        println!("setup.suggest_preview=end");
    }

    println!("setup.result=ok");
    println!("setup.profile={}", profile_name);
    println!("setup.apply_executed={}", apply);
    println!("changed=profile_initialized_and_apply_executed");
    println!("rollback_hint=resguard rollback --last");
    println!("user_reload_hint=systemctl --user daemon-reload");
    println!("followup=for desktop wraps run: resguard suggest --dry-run");
    println!("followup=apply suggestions with confidence gate: resguard suggest --apply");
    Ok(if apply { apply_code } else { 0 })
}
