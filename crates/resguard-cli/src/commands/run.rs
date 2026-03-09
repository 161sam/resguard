use crate::*;
use resguard_services::run_service::{
    execute_run_plan, resolve_run_plan, RunPlan, RunServiceRequest,
};

pub(crate) fn run(root: &str, config_dir: &str, state_dir: &str, req: RunRequest) -> Result<i32> {
    println!("command=run");
    println!(
        "class={:?} profile={:?} slice={:?} no_check={} wait={} command={:?}",
        req.class, req.profile_override, req.slice_override, req.no_check, req.wait, req.command
    );

    let rooted_config_dir = resolve_with_root(root, PathBuf::from(config_dir))?;
    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;

    let plan: RunPlan = resolve_run_plan(
        RunServiceRequest {
            class: req.class,
            profile_override: req.profile_override,
            slice_override: req.slice_override,
            no_check: req.no_check,
            wait: req.wait,
            command: req.command,
        },
        is_root_user,
        || {
            let state = read_state(&rooted_state_dir)?;
            Ok(state.active_profile)
        },
        |profile_name| {
            load_profile_from_store(&rooted_config_dir, profile_name).map_err(|err| {
                anyhow!(
                    "failed to load profile '{}' from {}: {err}",
                    profile_name,
                    rooted_config_dir.display()
                )
            })
        },
    )?;

    println!("selected.class={}", plan.class);
    println!("selected.slice={}", plan.slice);
    println!("resolution.source={}", plan.resolution_source);
    println!("mode={}", if plan.user_mode { "user" } else { "system" });

    if plan.no_check {
        eprintln!(
            "warn: skipping slice existence check due to --no-check (unsafe, poweruser mode)"
        );
    }

    execute_run_plan(
        &plan,
        |user_mode, slice| systemctl_cat_unit(user_mode, slice),
        |user_mode, slice, wait, command| systemd_run(user_mode, slice, wait, command),
    )
}
