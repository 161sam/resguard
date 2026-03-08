pub fn desktop_id_stem(desktop_id: &str) -> Option<&str> {
    desktop_id.strip_suffix(".desktop")
}

pub fn snap_app_from_desktop_id(desktop_id: &str) -> Option<String> {
    let stem = desktop_id_stem(desktop_id)?;
    if let Some((_, app)) = stem.split_once('_') {
        if !app.is_empty() {
            return Some(app.to_string());
        }
    }
    if let Some(rest) = stem.strip_prefix("snap.") {
        let mut parts = rest.split('.');
        let _snap_name = parts.next()?;
        let app = parts.next()?;
        if !app.is_empty() {
            return Some(app.to_string());
        }
    }
    None
}
