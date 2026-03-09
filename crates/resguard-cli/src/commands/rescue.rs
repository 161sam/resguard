use crate::*;

pub(crate) fn default_shell_path() -> String {
    resguard_services::rescue_service::default_shell_path()
}

pub(crate) fn build_rescue_command(
    shell: &str,
    custom_command: Option<&str>,
    no_ui: bool,
) -> Vec<String> {
    resguard_services::rescue_service::build_rescue_command(shell, custom_command, no_ui)
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
    resguard_services::rescue_service::rescue(
        class,
        custom_command,
        no_ui,
        no_check,
        |slice_or_class, bypass_check, command| {
            if bypass_check {
                handle_run(
                    root,
                    config_dir,
                    state_dir,
                    RunRequest {
                        class: Some("rescue".to_string()),
                        profile_override: None,
                        slice_override: Some(slice_or_class),
                        no_check: true,
                        wait: false,
                        command,
                    },
                )
            } else {
                handle_run(
                    root,
                    config_dir,
                    state_dir,
                    RunRequest {
                        class: Some(slice_or_class),
                        profile_override: None,
                        slice_override: None,
                        no_check: false,
                        wait: false,
                        command,
                    },
                )
            }
        },
    )
}

pub(crate) fn run(
    root: &str,
    config_dir: &str,
    state_dir: &str,
    class: String,
    command: Option<String>,
    no_ui: bool,
    no_check: bool,
) -> Result<i32> {
    handle_rescue(root, config_dir, state_dir, class, command, no_ui, no_check)
}
