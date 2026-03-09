use crate::alias::resolve_alias_candidate;
use crate::exec::parse_first_exec_token;
use crate::snap::snap_app_from_desktop_id;
use crate::xdg::{desktop_scan_dirs, origin_matches, DesktopOrigin};
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub desktop_id: String,
    pub name: String,
    pub exec: String,
    pub path: String,
    pub origin: DesktopOrigin,
    pub fields: BTreeMap<String, String>,
    pub source_content: String,
}

#[derive(Debug, Clone)]
pub enum ResolutionResult {
    Exact(DesktopEntry),
    Alias {
        requested: String,
        resolved: DesktopEntry,
    },
    Ambiguous {
        requested: String,
        candidates: Vec<String>,
    },
    NotFound {
        requested: String,
    },
}

pub fn validate_desktop_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 200 {
        return Err("invalid desktop id length".to_string());
    }
    if !id.ends_with(".desktop") {
        return Err("desktop id must end with .desktop".to_string());
    }
    if id.contains('/') || id.contains('\\') {
        return Err("desktop id must not contain path separators".to_string());
    }
    if id.contains("..") {
        return Err("desktop id must not contain '..'".to_string());
    }
    Ok(())
}

pub fn parse_desktop_entry(s: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut in_entry = false;
    for line in s.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

pub fn discover_desktop_entries(origin_filter: DesktopOrigin) -> Vec<DesktopEntry> {
    let mut items = Vec::new();

    for (dir, origin) in desktop_scan_dirs() {
        if !origin_matches(origin_filter, origin) || !dir.exists() {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(_) => continue,
        };

        for entry in entries {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let map = parse_desktop_entry(&content);
            if map.is_empty() {
                continue;
            }

            if let Some(t) = map.get("Type") {
                if t != "Application" {
                    continue;
                }
            }

            let desktop_id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let name = map.get("Name").cloned().unwrap_or_default();
            let exec = map.get("Exec").cloned().unwrap_or_default();

            items.push(DesktopEntry {
                desktop_id,
                name,
                exec,
                path: path.display().to_string(),
                origin,
                fields: map,
                source_content: content,
            });
        }
    }

    items.sort_by(|a, b| a.desktop_id.cmp(&b.desktop_id).then(a.path.cmp(&b.path)));
    items
}

pub fn scan_desktop_entries() -> Vec<resguard_model::DesktopEntryRef> {
    discover_desktop_entries(DesktopOrigin::All)
        .into_iter()
        .map(|e| resguard_model::DesktopEntryRef {
            desktop_id: e.desktop_id,
            origin: Some(match e.origin {
                DesktopOrigin::User => "user".to_string(),
                DesktopOrigin::System => "system".to_string(),
                DesktopOrigin::All => "all".to_string(),
            }),
            source: Some(e.path),
        })
        .collect()
}

pub fn resolve_desktop_id(id: &str) -> ResolutionResult {
    if validate_desktop_id(id).is_err() {
        return ResolutionResult::NotFound {
            requested: id.to_string(),
        };
    }

    let entries = discover_desktop_entries(DesktopOrigin::All);
    if let Some(mapped) = resolve_unique_snap_canonical(id, &entries) {
        return mapped;
    }
    if let Some(exact) = entries.iter().find(|e| e.desktop_id == id) {
        return ResolutionResult::Exact(exact.clone());
    }

    let requested_stem = match id.strip_suffix(".desktop") {
        Some(v) => v,
        None => {
            return ResolutionResult::NotFound {
                requested: id.to_string(),
            };
        }
    };

    let mut hits_by_id: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for item in entries {
        let Some(stem) = item.desktop_id.strip_suffix(".desktop") else {
            continue;
        };
        let stem_match = stem.ends_with(&format!("_{requested_stem}"))
            || stem.starts_with(&format!("snap.{requested_stem}."));
        let exec_match =
            parse_first_exec_token(&item.exec).is_some_and(|bin| bin == requested_stem);
        let name_match = item.name.eq_ignore_ascii_case(requested_stem);

        if stem_match || exec_match || name_match {
            match hits_by_id.get(&item.desktop_id) {
                Some(existing) if existing.origin == DesktopOrigin::User => {}
                _ => {
                    hits_by_id.insert(item.desktop_id.clone(), item);
                }
            }
        }
    }

    let alias_hits: Vec<DesktopEntry> = hits_by_id.into_values().collect();
    if alias_hits.len() == 1 {
        return ResolutionResult::Alias {
            requested: id.to_string(),
            resolved: alias_hits[0].clone(),
        };
    }

    if !alias_hits.is_empty() {
        return ResolutionResult::Ambiguous {
            requested: id.to_string(),
            candidates: alias_hits
                .iter()
                .map(|v| v.desktop_id.clone())
                .collect::<Vec<_>>(),
        };
    }

    match resolve_alias_candidate(id, &discover_desktop_entries(DesktopOrigin::All)) {
        Some(one) => ResolutionResult::Alias {
            requested: id.to_string(),
            resolved: one,
        },
        None => ResolutionResult::NotFound {
            requested: id.to_string(),
        },
    }
}

fn resolve_unique_snap_canonical(id: &str, entries: &[DesktopEntry]) -> Option<ResolutionResult> {
    let requested_stem = id.strip_suffix(".desktop")?;
    let mut by_id: BTreeMap<String, DesktopEntry> = BTreeMap::new();
    for item in entries {
        if snap_app_from_desktop_id(&item.desktop_id).as_deref() == Some(requested_stem) {
            by_id.insert(item.desktop_id.clone(), item.clone());
        }
    }

    let candidates: Vec<DesktopEntry> = by_id.into_values().collect();
    match candidates.len() {
        0 => None,
        1 => Some(ResolutionResult::Alias {
            requested: id.to_string(),
            resolved: candidates[0].clone(),
        }),
        _ => Some(ResolutionResult::Ambiguous {
            requested: id.to_string(),
            candidates: candidates.iter().map(|v| v.desktop_id.clone()).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_desktop_id, ResolutionResult};
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use tempfile::tempdir;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvGuard {
        old_home: Option<OsString>,
        old_xdg_data_home: Option<OsString>,
        old_xdg_data_dirs: Option<OsString>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(home: &Path, xdg_home: &Path, xdg_dirs: Option<&OsStr>) -> Self {
            let lock = ENV_LOCK.get_or_init(|| Mutex::new(()));
            let guard = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let old_home = std::env::var_os("HOME");
            let old_xdg_data_home = std::env::var_os("XDG_DATA_HOME");
            let old_xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS");
            std::env::set_var("HOME", home);
            std::env::set_var("XDG_DATA_HOME", xdg_home);
            match xdg_dirs {
                Some(v) => std::env::set_var("XDG_DATA_DIRS", v),
                None => std::env::remove_var("XDG_DATA_DIRS"),
            }
            Self {
                old_home,
                old_xdg_data_home,
                old_xdg_data_dirs,
                _lock: guard,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.old_home.take() {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match self.old_xdg_data_home.take() {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
            match self.old_xdg_data_dirs.take() {
                Some(v) => std::env::set_var("XDG_DATA_DIRS", v),
                None => std::env::remove_var("XDG_DATA_DIRS"),
            }
        }
    }

    #[test]
    fn desktop_lookup_exact_id_works() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let xdg_home = temp.path().join("xdg-home");
        let _env = EnvGuard::set(&home, &xdg_home, Some(OsStr::new("")));
        let apps = xdg_home.join("applications");
        std::fs::create_dir_all(&apps).expect("create app dir");
        std::fs::write(
            apps.join("firefox_firefox.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/snap/bin/firefox %u\n",
        )
        .expect("write desktop");

        match resolve_desktop_id("firefox_firefox.desktop") {
            ResolutionResult::Exact(e) => assert_eq!(e.desktop_id, "firefox_firefox.desktop"),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn desktop_lookup_not_found_is_reported() {
        match resolve_desktop_id("resguard-this-should-not-exist.desktop") {
            ResolutionResult::NotFound { .. } => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn desktop_lookup_prefers_snap_canonical_for_firefox_alias() {
        let temp = tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let xdg_home = temp.path().join("xdg-home");
        let _env = EnvGuard::set(&home, &xdg_home, Some(OsStr::new("")));
        let apps = xdg_home.join("applications");
        std::fs::create_dir_all(&apps).expect("create app dir");
        std::fs::write(
            apps.join("firefox_firefox.desktop"),
            "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/snap/bin/firefox %u\n",
        )
        .expect("write desktop");

        match resolve_desktop_id("firefox.desktop") {
            ResolutionResult::Alias { resolved, .. } => {
                assert_eq!(resolved.desktop_id, "firefox_firefox.desktop")
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }
}
