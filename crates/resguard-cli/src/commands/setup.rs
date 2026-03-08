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
    let profile_name = name.unwrap_or_else(|| "auto".to_string());

    resguard_services::setup_service::setup(
        profile_name.clone(),
        apply,
        suggest,
        |profile_name| {
            let setup_out = profile_path(config_dir, profile_name)?
                .to_string_lossy()
                .to_string();
            handle_init(
                root,
                config_dir,
                state_dir,
                Some(profile_name.to_string()),
                Some(setup_out),
                false,
                false,
            )
        },
        |profile_name| {
            handle_apply(
                root,
                config_dir,
                state_dir,
                profile_name,
                &ApplyOptions {
                    dry_run: false,
                    no_oomd: false,
                    no_cpu: false,
                    no_classes: false,
                    force: false,
                    user_daemon_reload: false,
                },
            )
        },
        |profile_name| {
            commands::suggest::handle_suggest(SuggestRequest {
                format: format.to_string(),
                root: root.to_string(),
                config_dir: config_dir.to_string(),
                state_dir: state_dir.to_string(),
                profile: Some(profile_name.to_string()),
                apply: false,
                dry_run: true,
                confidence_threshold: 70,
            })
        },
    )
}
