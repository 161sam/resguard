use resguard_model::SuggestRule;

#[derive(Debug, Clone)]
pub struct ClassificationInput {
    pub scope: String,
    pub slice: String,
    pub exec_start: String,
    pub memory_current: u64,
}

#[derive(Debug, Clone)]
pub struct ClassMatch {
    pub class: String,
    pub reason: String,
    pub pattern_match: bool,
    pub memory_threshold_match: bool,
}

fn simple_pattern_match(pattern: &str, hay: &str) -> bool {
    let (case_insensitive, p) = if let Some(rest) = pattern.strip_prefix("(?i)") {
        (true, rest)
    } else {
        (false, pattern)
    };

    if case_insensitive {
        let hay_lc = hay.to_ascii_lowercase();
        return p
            .split('|')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .any(|token| hay_lc.contains(&token.to_ascii_lowercase()));
    }

    p.split('|')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .any(|token| hay.contains(token))
}

pub fn classify(input: &ClassificationInput, rules: &[SuggestRule]) -> Option<ClassMatch> {
    let hay = format!("{} {} {}", input.scope, input.slice, input.exec_start);
    for rule in rules {
        if simple_pattern_match(&rule.pattern, &hay) {
            return Some(ClassMatch {
                class: rule.class.clone(),
                reason: format!("matched profile rule /{}/", rule.pattern),
                pattern_match: true,
                memory_threshold_match: false,
            });
        }
    }

    let h = hay.to_ascii_lowercase();
    if h.contains("docker") || h.contains("podman") {
        return Some(ClassMatch {
            class: "heavy".to_string(),
            reason: "container workload detected".to_string(),
            pattern_match: true,
            memory_threshold_match: false,
        });
    }
    if h.contains("code")
        || h.contains("codium")
        || h.contains("vscodium")
        || h.contains("jetbrains")
        || h.contains("idea")
        || h.contains("pycharm")
        || h.contains("clion")
        || h.contains("goland")
        || h.contains("webstorm")
        || h.contains("rubymine")
        || h.contains("phpstorm")
        || h.contains("datagrip")
        || h.contains("rider")
    {
        return Some(ClassMatch {
            class: "ide".to_string(),
            reason: "IDE workload detected".to_string(),
            pattern_match: true,
            memory_threshold_match: false,
        });
    }

    let gib = 1024_u64.pow(3);
    if input.slice == "app.slice" && input.memory_current >= 2 * gib {
        if h.contains("firefox")
            || h.contains("chrome")
            || h.contains("google-chrome")
            || h.contains("chromium")
            || h.contains("chromium-browser")
            || h.contains("brave")
        {
            return Some(ClassMatch {
                class: "browsers".to_string(),
                reason: "high-memory app.slice browser process".to_string(),
                pattern_match: true,
                memory_threshold_match: true,
            });
        }
        return Some(ClassMatch {
            class: "heavy".to_string(),
            reason: "high-memory app.slice process".to_string(),
            pattern_match: false,
            memory_threshold_match: true,
        });
    }

    None
}
