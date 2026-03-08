use crate::desktop::discover_desktop_entries;
use crate::exec::{parse_first_exec_token, parse_snap_run_app};
use crate::scope::parse_snap_app_from_scope;
use crate::snap::snap_app_from_desktop_id;
use crate::xdg::DesktopOrigin;
use resguard_model::AppIdentity;
use std::collections::HashMap;

fn index_desktop_exec_key(map: &mut HashMap<String, Vec<String>>, key: String, desktop_id: &str) {
    if key.is_empty() {
        return;
    }
    let ids = map.entry(key).or_default();
    if !ids.iter().any(|v| v == desktop_id) {
        ids.push(desktop_id.to_string());
    }
}

pub fn build_desktop_exec_index() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for item in discover_desktop_entries(DesktopOrigin::All) {
        if let Some(bin) = parse_first_exec_token(&item.exec) {
            index_desktop_exec_key(&mut map, bin, &item.desktop_id);
        }
        if let Some(snap_app) = parse_snap_run_app(&item.exec) {
            index_desktop_exec_key(&mut map, format!("snap:{snap_app}"), &item.desktop_id);
        }
        if let Some(snap_app) = snap_app_from_desktop_id(&item.desktop_id) {
            index_desktop_exec_key(&mut map, format!("snap:{snap_app}"), &item.desktop_id);
        }
    }
    map
}

pub fn unique_desktop_id_for_scope_exec(
    scope: &str,
    exec_start: &str,
    desktop_by_exec: &HashMap<String, Vec<String>>,
) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(bin) = parse_first_exec_token(exec_start) {
        candidates.push(bin);
    }
    if let Some(snap_app) = parse_snap_run_app(exec_start) {
        candidates.push(format!("snap:{snap_app}"));
        candidates.push(snap_app);
    }
    if let Some(snap_app) = parse_snap_app_from_scope(scope) {
        candidates.push(format!("snap:{snap_app}"));
        candidates.push(snap_app);
    }

    let mut matches: Vec<String> = Vec::new();
    for key in candidates {
        if let Some(ids) = desktop_by_exec.get(&key) {
            for id in ids {
                if !matches.iter().any(|v| v == id) {
                    matches.push(id.clone());
                }
            }
        }
    }

    if matches.len() == 1 {
        return matches.first().cloned();
    }
    None
}

pub fn parse_scope_identity(scope_name: &str, exec: &str) -> AppIdentity {
    let executable = parse_first_exec_token(exec);
    let snap_app = parse_snap_app_from_scope(scope_name).or_else(|| parse_snap_run_app(exec));
    AppIdentity {
        executable,
        snap_app,
        desktop_id: None,
    }
}
