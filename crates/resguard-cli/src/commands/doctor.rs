use crate::*;

pub(crate) fn handle_doctor(root: &str, state_dir: &str) -> Result<i32> {
    println!("command=doctor");
    let mut partial = false;

    println!("System checks");
    let systemd_ok = check_command_success("systemctl", &["--version"]);
    if systemd_ok {
        println!("OK  systemd detected");
    } else {
        println!("ERR systemd missing or unavailable (systemctl --version failed)");
        partial = true;
    }

    let cgroup_v2_path = if root == "/" {
        "/sys/fs/cgroup/cgroup.controllers".to_string()
    } else {
        format!(
            "{}/sys/fs/cgroup/cgroup.controllers",
            root.trim_end_matches('/')
        )
    };
    if Path::new(&cgroup_v2_path).exists() {
        println!("OK  cgroups v2 active");
    } else {
        println!("ERR cgroups v2 not detected ({})", cgroup_v2_path);
        partial = true;
    }

    let oomd_enabled = check_command_success("systemctl", &["is-enabled", "systemd-oomd"]);
    if oomd_enabled {
        println!("OK  systemd-oomd enabled");
    } else {
        println!("WARN systemd-oomd not enabled");
        partial = true;
    }

    println!();
    println!("Resguard checks");
    let rooted_state_dir = resolve_with_root(root, PathBuf::from(state_dir))?;
    let state_path = rooted_state_dir.join("state.json");
    let state_present = state_path.exists();
    if state_present {
        println!("OK  state.json present ({})", state_path.display());
    } else {
        println!("WARN state.json missing ({})", state_path.display());
        partial = true;
    }

    let mut slice_paths = Vec::new();
    if let Ok(state) = read_state(&rooted_state_dir) {
        for p in state.managed_paths {
            if p.ends_with(".slice") {
                slice_paths.push(p);
            }
        }
    }
    if slice_paths.is_empty() {
        println!("WARN class slices not found in state");
        partial = true;
    } else {
        let missing = slice_paths
            .iter()
            .filter(|p| !Path::new(p).exists())
            .count();
        if missing == 0 {
            println!("OK  class slices installed");
        } else {
            println!("WARN class slices partially missing (missing {})", missing);
            partial = true;
        }
    }

    let has_desktop_mappings = read_desktop_mapping_store()
        .map(|s| !s.mappings.is_empty())
        .unwrap_or(false);
    if has_desktop_mappings {
        println!();
        let (desktop_partial, _) = run_desktop_doctor_checks(false, false)?;
        if desktop_partial {
            partial = true;
        }
    }

    println!();
    println!("Hints");
    if env::var("SUDO_USER").is_ok() {
        println!("OK  sudo session detected");
    } else {
        println!("WARN user daemon reload may be required in active session");
        println!("fix: systemctl --user daemon-reload");
        println!("fix: logout/login");
        partial = true;
    }

    Ok(partial_exit_code(partial))
}
