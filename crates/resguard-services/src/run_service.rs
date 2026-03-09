use anyhow::{anyhow, Result};
use resguard_discovery::parse_scope_identity;
use resguard_model::Profile;
use resguard_policy::{
    classify, default_suggest_rules, score, ClassificationInput, ConfidenceSignals,
};

const AUTO_DETECT_THRESHOLD: u8 = 70;

#[derive(Debug, Clone)]
pub struct RunServiceRequest {
    pub class: Option<String>,
    pub profile_override: Option<String>,
    pub slice_override: Option<String>,
    pub no_check: bool,
    pub wait: bool,
    pub command: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RunPlan {
    pub class: String,
    pub slice: String,
    pub resolution_source: String,
    pub profile_hint: Option<String>,
    pub user_mode: bool,
    pub wait: bool,
    pub no_check: bool,
    pub command: Vec<String>,
}

fn resolve_class_slice(profile: &Profile, class_name: &str) -> Option<String> {
    if let Some(class) = profile.spec.classes.get(class_name) {
        return Some(
            class
                .slice_name
                .clone()
                .unwrap_or_else(|| format!("resguard-{class_name}.slice")),
        );
    }

    if let Some(slices) = &profile.spec.slices {
        if let Some(class) = slices.classes.get(class_name) {
            return Some(
                class
                    .slice_name
                    .clone()
                    .unwrap_or_else(|| format!("resguard-{class_name}.slice")),
            );
        }
    }

    None
}

fn autodetect_class(command: &[String]) -> Result<(String, String)> {
    let exec = command.join(" ");
    let identity = parse_scope_identity("resguard.run.scope", &exec);
    let rules = default_suggest_rules();

    let classified = classify(
        &ClassificationInput {
            scope: "resguard.run.scope".to_string(),
            slice: "app.slice".to_string(),
            exec_start: exec.clone(),
            memory_current: 0,
        },
        &rules,
    )
    .ok_or_else(|| {
        anyhow!(
            "auto-detect could not classify command '{exec}' safely\nnext steps:\n  1) run with explicit class: resguard run --class <class> -- {exec}\n  2) inspect known classes in your profile and retry"
        )
    })?;

    let confidence = score(
        &identity,
        &ConfidenceSignals {
            pattern_match: classified.pattern_match,
            memory_threshold_match: classified.memory_threshold_match,
            known_desktop_id: false,
            class: classified.class.clone(),
        },
    );

    if confidence.score < AUTO_DETECT_THRESHOLD {
        return Err(anyhow!(
            "auto-detect rejected command '{exec}' (class='{}', confidence={} < {})\nreason={}\nnext steps:\n  1) run with explicit class: resguard run --class <class> -- {exec}\n  2) for repeated usage, apply/setup profile first",
            classified.class,
            confidence.score,
            AUTO_DETECT_THRESHOLD,
            confidence.reason
        ));
    }

    Ok((
        classified.class,
        format!(
            "auto-detect from command (confidence={} reason={})",
            confidence.score, confidence.reason
        ),
    ))
}

fn class_and_source(req: &RunServiceRequest) -> Result<(String, String)> {
    if let Some(class) = &req.class {
        return Ok((class.clone(), "explicit class via --class".to_string()));
    }
    autodetect_class(&req.command)
}

pub fn resolve_run_plan<IR, AP, LP>(
    req: RunServiceRequest,
    is_root_user: IR,
    active_profile: AP,
    load_profile: LP,
) -> Result<RunPlan>
where
    IR: FnOnce() -> Result<bool>,
    AP: FnOnce() -> Result<Option<String>>,
    LP: Fn(&str) -> Result<Profile>,
{
    if req.command.is_empty() {
        return Err(anyhow!("command is required"));
    }

    let user_mode = !is_root_user()?;

    if let Some(slice) = req.slice_override.clone() {
        let (class, class_source) = class_and_source(&req)?;
        return Ok(RunPlan {
            class,
            slice,
            resolution_source: format!("slice override via --slice ({class_source})"),
            profile_hint: req.profile_override,
            user_mode,
            wait: req.wait,
            no_check: req.no_check,
            command: req.command,
        });
    }

    let (class, class_source) = class_and_source(&req)?;

    let (profile_name, profile_source) = if let Some(name) = req.profile_override.clone() {
        (Some(name), "explicit --profile".to_string())
    } else if let Some(active) = active_profile()? {
        (
            Some(active),
            "active profile from state (apply/setup)".to_string(),
        )
    } else {
        (None, "no profile available".to_string())
    };

    if let Some(profile_name) = profile_name.clone() {
        let profile = load_profile(&profile_name)?;
        if let Some(slice) = resolve_class_slice(&profile, &class) {
            return Ok(RunPlan {
                class,
                slice,
                resolution_source: format!(
                    "{class_source}; class mapped in profile '{}' ({})",
                    profile_name, profile_source
                ),
                profile_hint: Some(profile_name),
                user_mode,
                wait: req.wait,
                no_check: req.no_check,
                command: req.command,
            });
        }

        return Ok(RunPlan {
            class: class.clone(),
            slice: format!("resguard-{class}.slice"),
            resolution_source: format!(
                "{class_source}; class not defined in profile '{}' -> fallback default slice name",
                profile_name
            ),
            profile_hint: Some(profile_name),
            user_mode,
            wait: req.wait,
            no_check: req.no_check,
            command: req.command,
        });
    }

    Ok(RunPlan {
        class: class.clone(),
        slice: format!("resguard-{class}.slice"),
        resolution_source: format!(
            "{class_source}; no profile/state available -> fallback default slice name"
        ),
        profile_hint: None,
        user_mode,
        wait: req.wait,
        no_check: req.no_check,
        command: req.command,
    })
}

pub fn execute_run_plan<SC, SR>(
    plan: &RunPlan,
    check_slice_exists: SC,
    run_in_slice: SR,
) -> Result<i32>
where
    SC: Fn(bool, &str) -> Result<bool>,
    SR: Fn(bool, &str, bool, &[String]) -> Result<i32>,
{
    if !plan.no_check {
        let exists = check_slice_exists(plan.user_mode, &plan.slice).map_err(|err| {
            let profile_hint = plan.profile_hint.as_deref().unwrap_or("<profile>");
            anyhow!(
                "slice check failed\nselected class: {}\nselected slice: {}\nresolution source: {}\nmode: {}\ncheck error: {}\nnext steps:\n  1) sudo resguard setup --apply\n  2) sudo resguard apply {} --user-daemon-reload\n  3) retry your run command",
                plan.class,
                plan.slice,
                plan.resolution_source,
                if plan.user_mode { "user" } else { "system" },
                err,
                profile_hint,
            )
        })?;

        if !exists {
            let profile_hint = plan.profile_hint.as_deref().unwrap_or("<profile>");
            return Err(anyhow!(
                "slice not found\nselected class: {}\nselected slice: {}\nresolution source: {}\nmode: {}\nnext steps:\n  1) sudo resguard setup --apply\n  2) sudo resguard apply {} --user-daemon-reload\n  3) retry your run command",
                plan.class,
                plan.slice,
                plan.resolution_source,
                if plan.user_mode { "user" } else { "system" },
                profile_hint,
            ));
        }
    }

    let code = run_in_slice(plan.user_mode, &plan.slice, plan.wait, &plan.command)?;
    if plan.wait {
        return Ok(code);
    }
    if code == 0 {
        Ok(0)
    } else {
        Ok(6)
    }
}

#[cfg(test)]
mod tests {
    use super::{execute_run_plan, resolve_run_plan, RunPlan, RunServiceRequest};
    use resguard_model::{ClassSpec, Metadata, Profile, Spec};
    use std::collections::BTreeMap;

    fn profile_with(class: &str, slice: &str) -> Profile {
        let mut classes = BTreeMap::new();
        classes.insert(
            class.to_string(),
            ClassSpec {
                slice_name: Some(slice.to_string()),
                ..ClassSpec::default()
            },
        );
        Profile {
            api_version: "resguard.io/v1".to_string(),
            kind: "Profile".to_string(),
            metadata: Metadata {
                name: "dev".to_string(),
            },
            spec: Spec {
                classes,
                ..Spec::default()
            },
        }
    }

    #[test]
    fn explicit_class_run_uses_profile_slice() {
        let req = RunServiceRequest {
            class: Some("heavy".to_string()),
            profile_override: Some("dev".to_string()),
            slice_override: None,
            no_check: false,
            wait: false,
            command: vec!["cargo".to_string(), "build".to_string()],
        };

        let plan = resolve_run_plan(
            req,
            || Ok(false),
            || Ok(None),
            |_name| Ok(profile_with("heavy", "resguard-heavy.slice")),
        )
        .expect("plan");

        assert_eq!(plan.class, "heavy");
        assert_eq!(plan.slice, "resguard-heavy.slice");
        assert!(plan
            .resolution_source
            .contains("explicit class via --class"));
    }

    #[test]
    fn profile_backed_resolution_uses_active_profile() {
        let req = RunServiceRequest {
            class: Some("browsers".to_string()),
            profile_override: None,
            slice_override: None,
            no_check: false,
            wait: false,
            command: vec!["firefox".to_string()],
        };

        let plan = resolve_run_plan(
            req,
            || Ok(false),
            || Ok(Some("auto".to_string())),
            |_name| Ok(profile_with("browsers", "resguard-browsers.slice")),
        )
        .expect("plan");

        assert_eq!(plan.slice, "resguard-browsers.slice");
        assert_eq!(plan.profile_hint.as_deref(), Some("auto"));
    }

    #[test]
    fn missing_slice_guidance_is_actionable() {
        let plan = RunPlan {
            class: "heavy".to_string(),
            slice: "resguard-heavy.slice".to_string(),
            resolution_source: "explicit class via --class".to_string(),
            profile_hint: Some("auto".to_string()),
            user_mode: true,
            wait: false,
            no_check: false,
            command: vec!["cargo".to_string(), "build".to_string()],
        };

        let err = execute_run_plan(
            &plan,
            |_user, _slice| Ok(false),
            |_user, _slice, _wait, _cmd| Ok(0),
        )
        .expect_err("must fail");

        let txt = err.to_string();
        assert!(txt.contains("slice not found"));
        assert!(txt.contains("sudo resguard setup --apply"));
        assert!(txt.contains("sudo resguard apply auto --user-daemon-reload"));
    }

    #[test]
    fn autodetect_path_picks_strong_browser() {
        let req = RunServiceRequest {
            class: None,
            profile_override: None,
            slice_override: None,
            no_check: true,
            wait: false,
            command: vec!["firefox".to_string()],
        };

        let plan = resolve_run_plan(
            req,
            || Ok(false),
            || Ok(None),
            |_name| Ok(profile_with("browsers", "resguard-browsers.slice")),
        )
        .expect("plan");

        assert_eq!(plan.class, "browsers");
        assert!(plan.resolution_source.contains("auto-detect"));
    }

    #[test]
    fn autodetect_ambiguous_or_weak_match_is_rejected() {
        let req = RunServiceRequest {
            class: None,
            profile_override: None,
            slice_override: None,
            no_check: true,
            wait: false,
            command: vec!["unknown-firefox-wrapper".to_string()],
        };

        let err = resolve_run_plan(
            req,
            || Ok(false),
            || Ok(None),
            |_name| Ok(profile_with("browsers", "resguard-browsers.slice")),
        )
        .expect_err("must reject weak");

        let txt = err.to_string();
        assert!(txt.contains("auto-detect rejected"));
        assert!(txt.contains("resguard run --class"));
    }
}
