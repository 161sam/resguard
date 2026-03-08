pub fn parse_snap_app_from_scope(scope: &str) -> Option<String> {
    let mut s = scope.strip_suffix(".scope").unwrap_or(scope);
    if let Some(rest) = s.strip_prefix("app-") {
        s = rest;
    }
    let rest = s.strip_prefix("snap.")?;
    let mut parts = rest.split('.');
    let _snap_name = parts.next()?;
    let app_raw = parts.next()?;
    let app = app_raw
        .split_once('-')
        .map(|(left, _)| left)
        .unwrap_or(app_raw);
    if app.is_empty() {
        None
    } else {
        Some(app.to_string())
    }
}
