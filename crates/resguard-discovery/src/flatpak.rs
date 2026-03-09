pub fn flatpak_app_id_from_desktop_id(desktop_id: &str) -> Option<String> {
    let stem = desktop_id.strip_suffix(".desktop")?;
    if !stem.contains('.') {
        return None;
    }
    // Flatpak desktop IDs are typically reverse-DNS style.
    if stem
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        Some(stem.to_string())
    } else {
        None
    }
}

pub fn flatpak_app_name(app_id: &str) -> Option<String> {
    let tail = app_id.rsplit('.').next().unwrap_or(app_id).trim();
    if tail.is_empty() {
        None
    } else {
        Some(tail.to_ascii_lowercase())
    }
}

pub fn parse_flatpak_run_app(exec: &str) -> Option<String> {
    let mut cleaned = Vec::new();
    for tok in exec.split_whitespace() {
        if tok == "env" {
            continue;
        }
        if tok.contains('=') && tok.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let t = tok.trim_matches('"').trim_matches('\'');
        if !t.is_empty() {
            cleaned.push(t.to_string());
        }
    }

    let mut i = 0usize;
    while i + 2 < cleaned.len() {
        let base = std::path::Path::new(&cleaned[i])
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or(&cleaned[i]);
        if base == "flatpak" && cleaned[i + 1] == "run" {
            for app in &cleaned[(i + 2)..] {
                if app.starts_with('-') || app == "@@u" || app == "@@f" {
                    continue;
                }
                if app.contains('.') && app.chars().any(|c| c.is_ascii_alphabetic()) {
                    return Some(app.to_string());
                }
            }
            return None;
        }
        i += 1;
    }
    None
}

pub fn parse_flatpak_app_from_scope(scope: &str) -> Option<String> {
    let mut s = scope.strip_suffix(".scope").unwrap_or(scope);
    if let Some(rest) = s.strip_prefix("app-") {
        s = rest;
    }
    let rest = s.strip_prefix("flatpak-")?;
    let app = rest
        .split_once('-')
        .map(|(left, _)| left)
        .unwrap_or(rest)
        .trim();
    if app.is_empty() || !app.contains('.') {
        None
    } else {
        Some(app.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        flatpak_app_id_from_desktop_id, flatpak_app_name, parse_flatpak_app_from_scope,
        parse_flatpak_run_app,
    };

    #[test]
    fn parse_flatpak_run_exec_app_id() {
        let exec =
            "/usr/bin/flatpak run --branch=stable --arch=x86_64 org.mozilla.firefox @@u %u @@";
        assert_eq!(
            parse_flatpak_run_app(exec).as_deref(),
            Some("org.mozilla.firefox")
        );
    }

    #[test]
    fn parse_flatpak_scope_app_id() {
        let scope = "app-flatpak-org.mozilla.firefox-1234.scope";
        assert_eq!(
            parse_flatpak_app_from_scope(scope).as_deref(),
            Some("org.mozilla.firefox")
        );
    }

    #[test]
    fn parse_flatpak_desktop_id_and_name() {
        let id = "org.mozilla.firefox.desktop";
        assert_eq!(
            flatpak_app_id_from_desktop_id(id).as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            flatpak_app_name("org.mozilla.firefox").as_deref(),
            Some("firefox")
        );
    }
}
