use crate::desktop::{discover_desktop_entries, DesktopEntry};
use crate::exec::{parse_first_exec_token, parse_snap_run_app};
use crate::flatpak::{
    flatpak_app_id_from_desktop_id, flatpak_app_name, parse_flatpak_app_from_scope,
    parse_flatpak_run_app,
};
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

fn normalize_exec_token(token: &str) -> String {
    let mut out = token.to_ascii_lowercase();
    if let Some(stripped) = out.strip_suffix(".sh") {
        out = stripped.to_string();
    }
    if let Some(stripped) = out.strip_suffix("64") {
        out = stripped.to_string();
    }
    out
}

fn known_aliases(token: &str) -> Vec<String> {
    let t = normalize_exec_token(token);
    let mut out = vec![t.clone()];
    match t.as_str() {
        "google-chrome-stable" | "google-chrome" | "chrome" => {
            out.push("chromium".to_string());
        }
        "chromium-browser" => {
            out.push("chromium".to_string());
            out.push("chrome".to_string());
        }
        "vscodium" => {
            out.push("codium".to_string());
        }
        _ => {}
    }
    out
}

fn is_secondary_desktop_id(id: &str) -> bool {
    let v = id.to_ascii_lowercase();
    v.contains("url-handler") || v.contains("x-scheme-handler")
}

fn resolve_unique_or_preferred(matches: Vec<String>) -> Option<String> {
    if matches.len() == 1 {
        return matches.first().cloned();
    }
    let primary: Vec<String> = matches
        .into_iter()
        .filter(|id| !is_secondary_desktop_id(id))
        .collect();
    if primary.len() == 1 {
        return primary.first().cloned();
    }
    None
}

fn index_entry(map: &mut HashMap<String, Vec<String>>, item: &DesktopEntry) {
    if let Some(bin) = parse_first_exec_token(&item.exec) {
        for key in known_aliases(&bin) {
            index_desktop_exec_key(map, key, &item.desktop_id);
        }
    }
    if let Some(snap_app) = parse_snap_run_app(&item.exec) {
        index_desktop_exec_key(map, format!("snap:{snap_app}"), &item.desktop_id);
    }
    if let Some(snap_app) = snap_app_from_desktop_id(&item.desktop_id) {
        index_desktop_exec_key(map, format!("snap:{snap_app}"), &item.desktop_id);
    }
    if let Some(flatpak_id) = flatpak_app_id_from_desktop_id(&item.desktop_id) {
        index_desktop_exec_key(map, format!("flatpak:{flatpak_id}"), &item.desktop_id);
        if let Some(name) = flatpak_app_name(&flatpak_id) {
            index_desktop_exec_key(map, name, &item.desktop_id);
        }
    }
}

pub fn build_desktop_exec_index() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for item in discover_desktop_entries(DesktopOrigin::All) {
        index_entry(&mut map, &item);
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
        for alias in known_aliases(&bin) {
            candidates.push(alias);
        }
    }
    if let Some(snap_app) = parse_snap_run_app(exec_start) {
        candidates.push(format!("snap:{snap_app}"));
        for alias in known_aliases(&snap_app) {
            candidates.push(alias);
        }
    }
    if let Some(snap_app) = parse_snap_app_from_scope(scope) {
        candidates.push(format!("snap:{snap_app}"));
        for alias in known_aliases(&snap_app) {
            candidates.push(alias);
        }
    }
    if let Some(flatpak_id) = parse_flatpak_run_app(exec_start) {
        candidates.push(format!("flatpak:{flatpak_id}"));
        if let Some(name) = flatpak_app_name(&flatpak_id) {
            for alias in known_aliases(&name) {
                candidates.push(alias);
            }
        }
    }
    if let Some(flatpak_id) = parse_flatpak_app_from_scope(scope) {
        candidates.push(format!("flatpak:{flatpak_id}"));
        if let Some(name) = flatpak_app_name(&flatpak_id) {
            for alias in known_aliases(&name) {
                candidates.push(alias);
            }
        }
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

    resolve_unique_or_preferred(matches)
}

pub fn parse_scope_identity(scope_name: &str, exec: &str) -> AppIdentity {
    let executable = parse_flatpak_run_app(exec)
        .and_then(|app_id| flatpak_app_name(&app_id))
        .or_else(|| {
            parse_flatpak_app_from_scope(scope_name).and_then(|app_id| flatpak_app_name(&app_id))
        })
        .or_else(|| parse_first_exec_token(exec));
    let snap_app = parse_snap_app_from_scope(scope_name).or_else(|| parse_snap_run_app(exec));
    AppIdentity {
        executable,
        snap_app,
        desktop_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::unique_desktop_id_for_scope_exec;
    use std::collections::HashMap;

    #[test]
    fn prefers_primary_desktop_entry_over_url_handler() {
        let idx = HashMap::from([(
            "code".to_string(),
            vec![
                "code.desktop".to_string(),
                "code-url-handler.desktop".to_string(),
            ],
        )]);

        let out =
            unique_desktop_id_for_scope_exec("app-code.scope", "/usr/bin/code --new-window", &idx);
        assert_eq!(out.as_deref(), Some("code.desktop"));
    }

    #[test]
    fn resolves_chromium_aliases_from_google_chrome_binary() {
        let idx = HashMap::from([("chromium".to_string(), vec!["chromium.desktop".to_string()])]);

        let out = unique_desktop_id_for_scope_exec(
            "app-chrome.scope",
            "/usr/bin/google-chrome-stable --new-window",
            &idx,
        );
        assert_eq!(out.as_deref(), Some("chromium.desktop"));
    }

    #[test]
    fn keeps_ambiguous_primary_entries_unresolved() {
        let idx = HashMap::from([(
            "idea".to_string(),
            vec![
                "jetbrains-idea.desktop".to_string(),
                "intellij-idea-ultimate.desktop".to_string(),
            ],
        )]);

        let out = unique_desktop_id_for_scope_exec("app-idea.scope", "/opt/idea/bin/idea.sh", &idx);
        assert!(out.is_none());
    }

    #[test]
    fn flatpak_browser_detection_uses_app_id_and_name_alias() {
        let idx = HashMap::from([(
            "flatpak:org.mozilla.firefox".to_string(),
            vec!["org.mozilla.firefox.desktop".to_string()],
        )]);

        let out = unique_desktop_id_for_scope_exec(
            "app-flatpak-org.mozilla.firefox-1234.scope",
            "/usr/bin/flatpak run org.mozilla.firefox @@u %u @@",
            &idx,
        );
        assert_eq!(out.as_deref(), Some("org.mozilla.firefox.desktop"));
    }

    #[test]
    fn flatpak_ide_detection_prefers_unique_primary_entry() {
        let idx = HashMap::from([(
            "flatpak:com.visualstudio.code".to_string(),
            vec![
                "com.visualstudio.code.desktop".to_string(),
                "com.visualstudio.code-url-handler.desktop".to_string(),
            ],
        )]);

        let out = unique_desktop_id_for_scope_exec(
            "app-flatpak-com.visualstudio.code-23.scope",
            "/usr/bin/flatpak run com.visualstudio.code --new-window",
            &idx,
        );
        assert_eq!(out.as_deref(), Some("com.visualstudio.code.desktop"));
    }

    #[test]
    fn flatpak_ambiguous_identity_stays_conservative() {
        let idx = HashMap::from([(
            "flatpak:com.jetbrains.IntelliJ-IDEA-Ultimate".to_string(),
            vec![
                "com.jetbrains.IntelliJ-IDEA-Ultimate.desktop".to_string(),
                "com.jetbrains.IntelliJ-IDEA-Community.desktop".to_string(),
            ],
        )]);

        let out = unique_desktop_id_for_scope_exec(
            "app-flatpak-com.jetbrains.IntelliJ-IDEA-Ultimate-42.scope",
            "/usr/bin/flatpak run com.jetbrains.IntelliJ-IDEA-Ultimate",
            &idx,
        );
        assert!(out.is_none());
    }
}
