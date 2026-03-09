use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopOrigin {
    User,
    System,
    All,
}

fn push_scan_dir(
    out: &mut Vec<(PathBuf, DesktopOrigin)>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    dir: PathBuf,
    origin: DesktopOrigin,
) {
    if seen.insert(dir.clone()) {
        out.push((dir, origin));
    }
}

pub fn desktop_scan_dirs() -> Vec<(PathBuf, DesktopOrigin)> {
    let mut dirs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    let user_data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(data_home) = user_data_home {
        push_scan_dir(
            &mut dirs,
            &mut seen,
            data_home.join("applications"),
            DesktopOrigin::User,
        );
        push_scan_dir(
            &mut dirs,
            &mut seen,
            data_home.join("flatpak/exports/share/applications"),
            DesktopOrigin::User,
        );
    }

    if let Some(raw) = std::env::var_os("XDG_DATA_DIRS") {
        for dir in std::env::split_paths(&raw) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            push_scan_dir(
                &mut dirs,
                &mut seen,
                dir.join("applications"),
                DesktopOrigin::System,
            );
        }
    }

    for path in [
        "/usr/local/share/applications",
        "/usr/share/applications",
        "/var/lib/flatpak/exports/share/applications",
        "/var/lib/snapd/desktop/applications",
    ] {
        push_scan_dir(
            &mut dirs,
            &mut seen,
            PathBuf::from(path),
            DesktopOrigin::System,
        );
    }

    dirs
}

pub fn origin_matches(filter: DesktopOrigin, item_origin: DesktopOrigin) -> bool {
    match filter {
        DesktopOrigin::All => true,
        DesktopOrigin::User => item_origin == DesktopOrigin::User,
        DesktopOrigin::System => item_origin == DesktopOrigin::System,
    }
}
