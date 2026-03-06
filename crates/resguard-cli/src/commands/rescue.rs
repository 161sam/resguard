use crate::*;

pub(crate) fn default_shell_path() -> String {
    if let Ok(shell) = env::var("SHELL") {
        let trimmed = shell.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if Path::new("/bin/bash").exists() {
        "/bin/bash".to_string()
    } else {
        "/bin/sh".to_string()
    }
}

pub(crate) fn build_rescue_command(
    shell: &str,
    custom_command: Option<&str>,
    no_ui: bool,
) -> Vec<String> {
    if let Some(cmd) = custom_command {
        return vec![shell.to_string(), "-lc".to_string(), cmd.to_string()];
    }
    if no_ui {
        return vec![shell.to_string()];
    }
    vec![
        shell.to_string(),
        "-lc".to_string(),
        "htop || top".to_string(),
    ]
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
    println!("command=rescue");
    let shell = default_shell_path();
    let command = build_rescue_command(&shell, custom_command.as_deref(), no_ui);
    println!(
        "class={} shell={} command={:?} no_ui={} no_check={}",
        class, shell, command, no_ui, no_check
    );

    let base_req = RunRequest {
        class: class.clone(),
        profile_override: None,
        slice_override: None,
        no_check: false,
        wait: false,
        command: command.clone(),
    };

    match handle_run(root, config_dir, state_dir, base_req) {
        Ok(code) => Ok(code),
        Err(err) => {
            let msg = err.to_string();
            let rescue_target_missing = msg.contains("class '")
                || msg.contains("slice not found")
                || msg.contains("slice check failed");
            if no_check && rescue_target_missing {
                eprintln!(
                    "warn: rescue class/slice unavailable; falling back to system.slice due to --no-check"
                );
                return handle_run(
                    root,
                    config_dir,
                    state_dir,
                    RunRequest {
                        class,
                        profile_override: None,
                        slice_override: Some("system.slice".to_string()),
                        no_check: true,
                        wait: false,
                        command,
                    },
                );
            }
            if rescue_target_missing {
                return Err(anyhow!(
                    "{err}\nrescue fix:\n  1) apply/create a profile that defines class '{}' (auto profiles include it)\n  2) sudo resguard apply <profile> --user-daemon-reload\n  3) retry: resguard rescue\noptional fallback (unsafe): resguard rescue --no-check",
                    class
                ));
            }
            Err(err)
        }
    }
}
