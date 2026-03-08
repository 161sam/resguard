use resguard_model::{ClassSpec, Profile};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Action {
    EnsureDir {
        path: PathBuf,
    },
    WriteFile {
        path: PathBuf,
        content: String,
    },
    Exec {
        program: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        best_effort: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    pub no_oomd: bool,
    pub no_cpu: bool,
    pub no_classes: bool,
    pub user_daemon_reload: bool,
    pub sudo_user: Option<String>,
    pub sudo_runtime_dir: Option<String>,
}

const MANAGED_HEADER: &str = "# Managed by resguard. DO NOT EDIT.";

pub fn plan_apply(profile: &Profile, root: &Path, options: &PlanOptions) -> Vec<Action> {
    build_apply_plan(profile, root, options)
}

pub fn plan_apply_summary(
    profile: &Profile,
    root: &Path,
    options: &PlanOptions,
) -> resguard_model::ActionPlan {
    let actions = build_apply_plan(profile, root, options);
    let rendered = actions
        .iter()
        .map(|a| match a {
            Action::EnsureDir { path } => format!("ensure_dir {}", path.display()),
            Action::WriteFile { path, .. } => format!("write_file {}", path.display()),
            Action::Exec { program, args, .. } => format!("exec {} {}", program, args.join(" ")),
        })
        .collect();
    resguard_model::ActionPlan { actions: rendered }
}

pub fn build_apply_plan(profile: &Profile, root: &Path, options: &PlanOptions) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut dirs = BTreeSet::new();

    let system_dropin = rooted(root, "/etc/systemd/system/system.slice.d/50-resguard.conf");
    let user_dropin = rooted(root, "/etc/systemd/system/user.slice.d/50-resguard.conf");

    if let Some(parent) = system_dropin.parent() {
        dirs.insert(parent.to_path_buf());
    }
    if let Some(parent) = user_dropin.parent() {
        dirs.insert(parent.to_path_buf());
    }

    for dir in dirs.iter().cloned() {
        actions.push(Action::EnsureDir { path: dir });
    }

    let system_allowed_cpus = profile
        .spec
        .cpu
        .as_ref()
        .and_then(|cpu| cpu.system_allowed_cpus.as_deref());
    let user_allowed_cpus = profile
        .spec
        .cpu
        .as_ref()
        .and_then(|cpu| cpu.user_allowed_cpus.as_deref());

    let memory_low = profile
        .spec
        .memory
        .as_ref()
        .and_then(|memory| memory.system.as_ref())
        .and_then(|system| system.memory_low.as_deref());

    let memory_high = profile
        .spec
        .memory
        .as_ref()
        .and_then(|memory| memory.user.as_ref())
        .and_then(|user| user.memory_high.as_deref());

    let memory_max = profile
        .spec
        .memory
        .as_ref()
        .and_then(|memory| memory.user.as_ref())
        .and_then(|user| user.memory_max.as_deref());

    let oomd_mode = if options.no_oomd {
        None
    } else {
        profile
            .spec
            .oomd
            .as_ref()
            .and_then(|oomd| oomd.memory_pressure.as_deref())
    };

    let oomd_limit = if options.no_oomd {
        None
    } else {
        profile
            .spec
            .oomd
            .as_ref()
            .and_then(|oomd| oomd.memory_pressure_limit.as_deref())
    };

    actions.push(Action::WriteFile {
        path: system_dropin,
        content: render_system_slice_dropin(
            memory_low,
            if options.no_cpu {
                None
            } else {
                system_allowed_cpus
            },
        ),
    });

    actions.push(Action::WriteFile {
        path: user_dropin,
        content: render_user_slice_dropin(
            memory_high,
            memory_max,
            if options.no_cpu {
                None
            } else {
                user_allowed_cpus
            },
            oomd_mode,
            oomd_limit,
        ),
    });

    if !options.no_classes {
        let classes = collect_classes(profile);
        for (name, class) in classes {
            let slice_name = class
                .slice_name
                .clone()
                .unwrap_or_else(|| format!("resguard-{name}.slice"));

            let system_slice_path = rooted(root, &format!("/etc/systemd/system/{slice_name}"));
            let user_slice_path = rooted(root, &format!("/etc/systemd/user/{slice_name}"));

            if let Some(parent) = system_slice_path.parent() {
                actions.push(Action::EnsureDir {
                    path: parent.to_path_buf(),
                });
            }
            if let Some(parent) = user_slice_path.parent() {
                actions.push(Action::EnsureDir {
                    path: parent.to_path_buf(),
                });
            }

            actions.push(Action::WriteFile {
                path: system_slice_path,
                content: render_class_slice(&name, class, false, options.no_oomd),
            });
            actions.push(Action::WriteFile {
                path: user_slice_path,
                content: render_class_slice(&name, class, true, options.no_oomd),
            });
        }
    }

    if root == Path::new("/") {
        actions.push(Action::Exec {
            program: "systemctl".to_string(),
            args: vec!["daemon-reload".to_string()],
            env: BTreeMap::new(),
            best_effort: false,
        });

        if options.user_daemon_reload {
            if let Some(user) = &options.sudo_user {
                let mut args = vec!["-u".to_string(), user.clone()];
                if let Some(runtime_dir) = &options.sudo_runtime_dir {
                    args.push("env".to_string());
                    args.push(format!("XDG_RUNTIME_DIR={runtime_dir}"));
                }
                args.push("systemctl".to_string());
                args.push("--user".to_string());
                args.push("daemon-reload".to_string());
                actions.push(Action::Exec {
                    program: "sudo".to_string(),
                    args,
                    env: BTreeMap::new(),
                    best_effort: true,
                });
            }
        }
    }

    dedupe_ensure_dirs(actions)
}

fn collect_classes(profile: &Profile) -> BTreeMap<String, &ClassSpec> {
    let mut classes = BTreeMap::new();
    for (name, class) in &profile.spec.classes {
        classes.insert(name.clone(), class);
    }
    if let Some(slices) = &profile.spec.slices {
        for (name, class) in &slices.classes {
            classes.entry(name.clone()).or_insert(class);
        }
    }
    classes
}

fn dedupe_ensure_dirs(actions: Vec<Action>) -> Vec<Action> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for action in actions {
        match &action {
            Action::EnsureDir { path } => {
                if seen.insert(path.clone()) {
                    out.push(action);
                }
            }
            _ => out.push(action),
        }
    }

    out
}

fn rooted(root: &Path, abs_path: &str) -> PathBuf {
    if root == Path::new("/") {
        return PathBuf::from(abs_path);
    }
    root.join(abs_path.trim_start_matches('/'))
}

pub fn render_user_slice_dropin(
    memory_high: Option<&str>,
    memory_max: Option<&str>,
    allowed_cpus: Option<&str>,
    oomd_mode: Option<&str>,
    oomd_limit: Option<&str>,
) -> String {
    let mut s = String::new();
    s.push_str(MANAGED_HEADER);
    s.push_str("\n[Slice]\n");
    if let Some(v) = memory_high {
        s.push_str(&format!("MemoryHigh={v}\n"));
    }
    if let Some(v) = memory_max {
        s.push_str(&format!("MemoryMax={v}\n"));
    }
    if let Some(v) = allowed_cpus {
        s.push_str(&format!("AllowedCPUs={v}\n"));
    }
    if let Some(v) = oomd_mode {
        s.push_str(&format!("ManagedOOMMemoryPressure={v}\n"));
    }
    if let Some(v) = oomd_limit {
        s.push_str(&format!("ManagedOOMMemoryPressureLimit={v}\n"));
    }
    s
}

pub fn render_system_slice_dropin(memory_low: Option<&str>, allowed_cpus: Option<&str>) -> String {
    let mut s = String::new();
    s.push_str(MANAGED_HEADER);
    s.push_str("\n[Slice]\n");
    if let Some(v) = memory_low {
        s.push_str(&format!("MemoryLow={v}\n"));
    }
    if let Some(v) = allowed_cpus {
        s.push_str(&format!("AllowedCPUs={v}\n"));
    }
    s
}

pub fn render_class_slice(name: &str, class: &ClassSpec, user: bool, no_oomd: bool) -> String {
    let mut s = String::new();
    s.push_str(MANAGED_HEADER);
    s.push_str("\n[Unit]\n");
    if user {
        s.push_str(&format!("Description=Resguard {name} slice (user)\n\n"));
    } else {
        s.push_str(&format!("Description=Resguard {name} slice (system)\n\n"));
    }

    s.push_str("[Slice]\n");
    if let Some(v) = class.memory_high.as_deref() {
        s.push_str(&format!("MemoryHigh={v}\n"));
    }
    if let Some(v) = class.memory_max.as_deref() {
        s.push_str(&format!("MemoryMax={v}\n"));
    }
    if let Some(v) = class.cpu_weight {
        s.push_str(&format!("CPUWeight={v}\n"));
    }
    if !no_oomd {
        if let Some(v) = class.oomd_memory_pressure.as_deref() {
            s.push_str(&format!("ManagedOOMMemoryPressure={v}\n"));
        }
        if let Some(v) = class.oomd_memory_pressure_limit.as_deref() {
            s.push_str(&format!("ManagedOOMMemoryPressureLimit={v}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::{render_system_slice_dropin, render_user_slice_dropin};

    #[test]
    fn render_user_dropin_contains_expected_keys() {
        let out = render_user_slice_dropin(Some("12G"), Some("14G"), Some("1-7"), None, None);
        assert!(out.contains("MemoryHigh=12G"));
        assert!(out.contains("MemoryMax=14G"));
        assert!(out.contains("AllowedCPUs=1-7"));
    }

    #[test]
    fn render_system_dropin_contains_expected_keys() {
        let out = render_system_slice_dropin(Some("2G"), Some("0"));
        assert!(out.contains("MemoryLow=2G"));
        assert!(out.contains("AllowedCPUs=0"));
    }
}
