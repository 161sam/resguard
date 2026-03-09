use resguard_model::AppIdentity;

#[derive(Debug, Clone)]
pub struct ConfidenceSignals {
    pub pattern_match: bool,
    pub memory_threshold_match: bool,
    pub known_desktop_id: bool,
    pub class: String,
}

#[derive(Debug, Clone)]
pub struct ConfidenceScore {
    pub score: u8,
    pub reason: String,
}

fn expected_class_for_known_app(app: &str) -> Option<&'static str> {
    let app = app.to_ascii_lowercase();
    if [
        "firefox",
        "chrome",
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "brave",
        "opera",
        "vivaldi",
    ]
    .contains(&app.as_str())
    {
        return Some("browsers");
    }
    if [
        "code", "codium", "vscodium", "idea", "pycharm", "clion", "goland", "webstorm", "rubymine",
        "phpstorm", "datagrip", "rider",
    ]
    .contains(&app.as_str())
        || app.starts_with("jetbrains")
    {
        return Some("ide");
    }
    if ["docker", "podman", "containerd"].contains(&app.as_str()) {
        return Some("heavy");
    }
    None
}

pub fn strong_identity_match(identity: &AppIdentity, class: &str) -> bool {
    let app = identity
        .snap_app
        .as_deref()
        .or(identity.executable.as_deref())
        .map(|v| v.to_ascii_lowercase());
    let Some(app) = app else {
        return false;
    };
    expected_class_for_known_app(&app).is_some_and(|expected| expected == class)
}

pub fn score(identity: &AppIdentity, signals: &ConfidenceSignals) -> ConfidenceScore {
    let mut score = 0u8;
    let mut reasons = Vec::new();

    if signals.pattern_match {
        score = score.saturating_add(40);
        reasons.push("pattern");
    }
    if signals.memory_threshold_match {
        score = score.saturating_add(30);
        reasons.push("memory");
    }
    if signals.known_desktop_id {
        score = score.saturating_add(30);
        reasons.push("desktop-id");
    }
    if strong_identity_match(identity, &signals.class) {
        score = score.saturating_add(30);
        reasons.push("identity");
    }

    if reasons.is_empty() {
        reasons.push("none");
    }

    ConfidenceScore {
        score: score.min(100),
        reason: reasons.join("+"),
    }
}

#[cfg(test)]
mod tests {
    use super::{score, ConfidenceSignals};
    use resguard_model::AppIdentity;

    #[test]
    fn firefox_snap_scoring_reaches_default_threshold() {
        let identity = AppIdentity {
            executable: Some("firefox".to_string()),
            snap_app: Some("firefox".to_string()),
            desktop_id: None,
        };
        let got = score(
            &identity,
            &ConfidenceSignals {
                pattern_match: true,
                memory_threshold_match: false,
                known_desktop_id: false,
                class: "browsers".to_string(),
            },
        );
        assert_eq!(got.score, 70);
        assert!(got.reason.contains("identity"));
    }

    #[test]
    fn code_ide_scoring_reaches_default_threshold() {
        let identity = AppIdentity {
            executable: Some("code".to_string()),
            snap_app: Some("code".to_string()),
            desktop_id: None,
        };
        let got = score(
            &identity,
            &ConfidenceSignals {
                pattern_match: true,
                memory_threshold_match: false,
                known_desktop_id: false,
                class: "ide".to_string(),
            },
        );
        assert_eq!(got.score, 70);
    }

    #[test]
    fn ambiguous_identity_keeps_score_lower() {
        let identity = AppIdentity {
            executable: Some("unknown-browser".to_string()),
            snap_app: None,
            desktop_id: None,
        };
        let got = score(
            &identity,
            &ConfidenceSignals {
                pattern_match: true,
                memory_threshold_match: false,
                known_desktop_id: false,
                class: "browsers".to_string(),
            },
        );
        assert_eq!(got.score, 40);
        assert!(!got.reason.contains("identity"));
    }
}
