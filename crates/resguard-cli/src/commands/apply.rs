use crate::cli::ApplyOptions;
use crate::*;
use resguard_services::apply_service::{apply, ApplyRequest};

pub(crate) fn handle_apply(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    profile_name: &str,
    opts: &ApplyOptions,
) -> Result<i32> {
    apply(&ApplyRequest {
        root: root.to_string(),
        config_dir: config_dir.to_string(),
        state_dir: state_dir.to_string(),
        profile_name: profile_name.to_string(),
        dry_run: opts.dry_run,
        no_oomd: opts.no_oomd,
        no_cpu: opts.no_cpu,
        no_classes: opts.no_classes,
        force: opts.force,
        user_daemon_reload: opts.user_daemon_reload,
    })
}

pub(crate) fn run(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    profile: String,
    opts: ApplyOptions,
) -> Result<i32> {
    handle_apply(root, config_dir, state_dir, &profile, &opts)
}
