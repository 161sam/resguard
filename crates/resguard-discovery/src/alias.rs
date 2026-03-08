use crate::desktop::DesktopEntry;

pub fn resolve_alias_candidate(
    requested_id: &str,
    entries: &[DesktopEntry],
) -> Option<DesktopEntry> {
    let requested_stem = requested_id.strip_suffix(".desktop")?;
    let mut matches = entries
        .iter()
        .filter(|e| e.desktop_id.contains(requested_stem))
        .cloned()
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        return matches.pop();
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::desktop::DesktopEntry;
    use crate::xdg::DesktopOrigin;

    #[test]
    fn snap_alias_resolution_prefers_exact_unique_candidate_shape() {
        let entry = DesktopEntry {
            desktop_id: "firefox_firefox.desktop".to_string(),
            name: "Firefox".to_string(),
            exec: "/snap/bin/firefox %u".to_string(),
            path: "/var/lib/snapd/desktop/applications/firefox_firefox.desktop".to_string(),
            origin: DesktopOrigin::System,
            fields: std::collections::BTreeMap::new(),
            source_content: String::new(),
        };
        let out = super::resolve_alias_candidate("firefox.desktop", std::slice::from_ref(&entry));
        assert_eq!(out.map(|v| v.desktop_id), Some(entry.desktop_id));
    }

    #[test]
    fn ambiguous_shape_does_not_resolve() {
        let a = DesktopEntry {
            desktop_id: "foo_firefox.desktop".to_string(),
            name: "Firefox A".to_string(),
            exec: "firefox %u".to_string(),
            path: "/tmp/a.desktop".to_string(),
            origin: DesktopOrigin::System,
            fields: std::collections::BTreeMap::new(),
            source_content: String::new(),
        };
        let b = DesktopEntry {
            desktop_id: "bar_firefox.desktop".to_string(),
            ..a.clone()
        };
        let out = super::resolve_alias_candidate("firefox.desktop", &[a, b]);
        assert!(out.is_none());
    }
}
