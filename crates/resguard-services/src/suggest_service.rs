use anyhow::{anyhow, Result};
use resguard_discovery::{
    build_desktop_exec_index, parse_first_exec_token, parse_scope_identity,
    unique_desktop_id_for_scope_exec,
};
use resguard_model::{Profile, SuggestRule, Suggestion, SuggestionReason};
use resguard_policy::{
    classify, default_suggest_rules, meets_confidence_threshold, score,
    validate_confidence_threshold, ClassMatch, ClassificationInput, ConfidenceSignals,
};
use resguard_runtime::{systemctl_list_units, systemctl_show_props};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone)]
pub struct SuggestRequest {
    pub format: String,
    pub apply: bool,
    pub dry_run: bool,
    pub confidence_threshold: u8,
}

#[derive(Debug, Clone)]
struct ScopeObservation {
    scope: String,
    slice: String,
    exec_start: String,
    memory_current: u64,
    cpu_usage_nsec: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SuggestPlanItem {
    SkipLowConfidence {
        scope: String,
        class: String,
        confidence: u8,
        threshold: u8,
        confidence_reason: String,
    },
    WrapDesktop {
        desktop_id: String,
        class: String,
        confidence: u8,
        confidence_reason: String,
    },
    ManualWrap {
        scope: String,
        class: String,
        confidence: u8,
        confidence_reason: String,
        filter_hint: String,
        profile_hint: String,
    },
}

pub fn suggest<RP, WD>(
    req: SuggestRequest,
    resolve_profile: RP,
    mut wrap_desktop: WD,
) -> Result<i32>
where
    RP: FnOnce() -> Result<(Option<String>, Option<Profile>)>,
    WD: FnMut(&str, &str, bool) -> Result<()>,
{
    if req.apply && req.dry_run {
        return Err(anyhow!(
            "invalid arguments: --apply and --dry-run cannot be combined"
        ));
    }
    if let Err(msg) = validate_confidence_threshold(req.confidence_threshold) {
        return Err(anyhow!(msg));
    }

    println!("command=suggest");
    println!(
        "apply={} dry_run={} confidence_threshold={}",
        req.apply, req.dry_run, req.confidence_threshold
    );

    let (resolved_profile_name, resolved_profile) = resolve_profile()?;
    if let Some(name) = &resolved_profile_name {
        println!("profile_source={name}");
    } else {
        println!("profile_source=none (using built-in rules only)");
    }

    let mut rules = Vec::new();
    if let Some(p) = &resolved_profile {
        if let Some(cfg) = &p.spec.suggest {
            for r in &cfg.rules {
                rules.push(r.clone());
            }
        }
    }
    rules.extend(default_suggest_rules());

    let observations = match observe_active_scopes() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("warn: could not query user scopes: {err}");
            return Ok(1);
        }
    };
    let desktop_by_exec = build_desktop_exec_index();
    let suggestions = build_suggestions(&observations, &rules, &desktop_by_exec);

    if suggestions.is_empty() {
        println!("result=no-suggestions");
        println!("hint=run workload, then retry: resguard suggest");
        return Ok(0);
    }

    match req.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&suggestions)?),
        "yaml" => println!("{}", serde_yaml::to_string(&suggestions)?),
        _ => print_suggestions_table(&suggestions, req.confidence_threshold),
    }

    if req.apply || req.dry_run {
        let profile_hint = resolved_profile_name.as_deref().unwrap_or("<profile>");
        let plan = build_plan_items(&suggestions, req.confidence_threshold, profile_hint);

        println!();
        if req.dry_run {
            println!("dry_run_preview");
        } else {
            println!("apply_results");
        }

        execute_plan_items(&plan, req.dry_run, &mut wrap_desktop);
    } else {
        println!();
        println!("next_steps");
        println!("1) review suggestions above");
        println!("2) auto-wrap known desktop entries: resguard suggest --apply");
        println!(
            "3) apply profile so user slices exist: sudo resguard apply <profile> --user-daemon-reload"
        );
    }

    Ok(0)
}

fn observe_active_scopes() -> Result<Vec<ScopeObservation>> {
    let scopes = systemctl_list_units(true, "scope")?;
    let mut observations = Vec::new();

    for scope in scopes.into_iter().filter(|u| u.ends_with(".scope")) {
        let props = match systemctl_show_props(
            true,
            &scope,
            &["MemoryCurrent", "CPUUsageNSec", "Slice", "ExecStart", "Id"],
        ) {
            Ok(v) => v,
            Err(_) => continue,
        };
        observations.push(scope_observation_from_props(&scope, &props));
    }

    Ok(observations)
}

fn scope_observation_from_props(scope: &str, props: &BTreeMap<String, String>) -> ScopeObservation {
    ScopeObservation {
        scope: scope.to_string(),
        exec_start: props.get("ExecStart").cloned().unwrap_or_default(),
        slice: props
            .get("Slice")
            .cloned()
            .unwrap_or_else(|| "-".to_string()),
        memory_current: props
            .get("MemoryCurrent")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0),
        cpu_usage_nsec: props
            .get("CPUUsageNSec")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0),
    }
}

fn build_suggestions(
    observations: &[ScopeObservation],
    rules: &[SuggestRule],
    desktop_by_exec: &HashMap<String, Vec<String>>,
) -> Vec<Suggestion> {
    let mut suggestions = Vec::new();

    for obs in observations {
        let Some(classified) = classify_scope(
            &obs.scope,
            &obs.slice,
            &obs.exec_start,
            obs.memory_current,
            rules,
        ) else {
            continue;
        };

        let identity = parse_scope_identity(&obs.scope, &obs.exec_start);
        let desktop_id =
            unique_desktop_id_for_scope_exec(&obs.scope, &obs.exec_start, desktop_by_exec);
        let scored = score(
            &identity,
            &ConfidenceSignals {
                pattern_match: classified.pattern_match,
                memory_threshold_match: classified.memory_threshold_match,
                known_desktop_id: desktop_id.is_some(),
                class: classified.class.clone(),
            },
        );

        suggestions.push(Suggestion {
            scope: obs.scope.clone(),
            class: classified.class,
            reason: SuggestionReason::Manual {
                message: classified.reason,
            },
            slice: obs.slice.clone(),
            exec_start: obs.exec_start.clone(),
            memory_current: obs.memory_current,
            cpu_usage_nsec: obs.cpu_usage_nsec,
            desktop_id,
            confidence: scored.score,
            confidence_reason: scored.reason,
        });
    }

    suggestions.sort_by(|a, b| {
        b.memory_current
            .cmp(&a.memory_current)
            .then(a.scope.cmp(&b.scope))
    });
    suggestions.dedup_by(|a, b| a.scope == b.scope && a.class == b.class);
    suggestions
}

fn build_plan_items(
    suggestions: &[Suggestion],
    threshold: u8,
    profile_hint: &str,
) -> Vec<SuggestPlanItem> {
    let mut out = Vec::new();

    for s in suggestions {
        if !meets_confidence_threshold(s.confidence, threshold) {
            out.push(SuggestPlanItem::SkipLowConfidence {
                scope: s.scope.clone(),
                class: s.class.clone(),
                confidence: s.confidence,
                threshold,
                confidence_reason: s.confidence_reason.clone(),
            });
            continue;
        }

        if let Some(desktop_id) = &s.desktop_id {
            out.push(SuggestPlanItem::WrapDesktop {
                desktop_id: desktop_id.clone(),
                class: s.class.clone(),
                confidence: s.confidence,
                confidence_reason: s.confidence_reason.clone(),
            });
            continue;
        }

        out.push(SuggestPlanItem::ManualWrap {
            scope: s.scope.clone(),
            class: s.class.clone(),
            confidence: s.confidence,
            confidence_reason: s.confidence_reason.clone(),
            filter_hint: parse_first_exec_token(&s.exec_start).unwrap_or_else(|| s.scope.clone()),
            profile_hint: profile_hint.to_string(),
        });
    }

    out
}

fn execute_plan_items<WD>(items: &[SuggestPlanItem], dry_run: bool, wrap_desktop: &mut WD)
where
    WD: FnMut(&str, &str, bool) -> Result<()>,
{
    for item in items {
        match item {
            SuggestPlanItem::SkipLowConfidence {
                scope,
                class,
                confidence,
                threshold,
                confidence_reason,
            } => println!(
                "skip\t{scope}\t{class}\tconfidence {confidence} below threshold {threshold} ({confidence_reason})"
            ),
            SuggestPlanItem::WrapDesktop {
                desktop_id,
                class,
                confidence,
                confidence_reason,
            } => {
                if dry_run {
                    println!(
                        "would-wrap\t{desktop_id}\t{class}\tconfidence={confidence}\treason={confidence_reason}\tauto-wrap=yes"
                    );
                } else {
                    match wrap_desktop(desktop_id, class, false) {
                        Ok(()) => println!(
                            "ok\t{desktop_id}\t{class}\twrapped\tconfidence={confidence}\treason={confidence_reason}"
                        ),
                        Err(err) => println!("warn\t{desktop_id}\t{class}\t{err}"),
                    }
                }
            }
            SuggestPlanItem::ManualWrap {
                scope,
                class,
                confidence,
                confidence_reason,
                filter_hint,
                profile_hint,
            } => println!(
                "hint\t{scope}\t{class}\tno unique desktop_id match (confidence={confidence} {confidence_reason}); auto-wrap=no; wrap manually: resguard desktop list --filter '{filter_hint}' && resguard desktop wrap <desktop_id> --class {class} (then sudo resguard apply {profile_hint} --user-daemon-reload)"
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct SuggestClassification {
    class: String,
    reason: String,
    pattern_match: bool,
    memory_threshold_match: bool,
}

fn classify_scope(
    unit: &str,
    slice: &str,
    exec_start: &str,
    memory_current: u64,
    rules: &[SuggestRule],
) -> Option<SuggestClassification> {
    classify(
        &ClassificationInput {
            scope: unit.to_string(),
            slice: slice.to_string(),
            exec_start: exec_start.to_string(),
            memory_current,
        },
        rules,
    )
    .map(|m: ClassMatch| SuggestClassification {
        class: m.class,
        reason: m.reason,
        pattern_match: m.pattern_match,
        memory_threshold_match: m.memory_threshold_match,
    })
}

fn suggestion_reason_text(reason: &SuggestionReason) -> String {
    match reason {
        SuggestionReason::PatternRule => "pattern rule".to_string(),
        SuggestionReason::MemoryThreshold => "memory threshold".to_string(),
        SuggestionReason::StrongIdentity => "strong identity".to_string(),
        SuggestionReason::DesktopIdMatch => "desktop-id match".to_string(),
        SuggestionReason::Manual { message } => message.clone(),
    }
}

fn print_suggestions_table(items: &[Suggestion], threshold: u8) {
    println!(
        "scope\tclass\tdesktop_id\tmemory\tconfidence\tconfidence_reason\tclass_reason\tauto_wrap\tnext_action"
    );
    for s in items {
        let auto_wrap = s.desktop_id.is_some();
        let next_action = if meets_confidence_threshold(s.confidence, threshold) {
            if auto_wrap {
                "ready: suggest --apply"
            } else {
                "manual wrap required"
            }
        } else {
            "below threshold: review"
        };

        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            s.scope,
            s.class,
            s.desktop_id.clone().unwrap_or_else(|| "-".to_string()),
            s.memory_current,
            s.confidence,
            s.confidence_reason,
            suggestion_reason_text(&s.reason),
            if auto_wrap { "yes" } else { "no" },
            next_action
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_plan_items, build_suggestions, execute_plan_items, ScopeObservation, SuggestPlanItem,
    };
    use resguard_model::SuggestRule;
    use std::collections::HashMap;

    #[test]
    fn suggest_dry_run_plans_wrap_for_strong_snap_matches() {
        let observations = vec![
            ScopeObservation {
                scope: "app-snap.firefox.firefox-1234.scope".to_string(),
                slice: "app.slice".to_string(),
                exec_start: "/usr/bin/snap run firefox".to_string(),
                memory_current: 512 * 1024 * 1024,
                cpu_usage_nsec: 0,
            },
            ScopeObservation {
                scope: "app-snap.code.code-5678.scope".to_string(),
                slice: "app.slice".to_string(),
                exec_start: "/usr/bin/snap run code".to_string(),
                memory_current: 768 * 1024 * 1024,
                cpu_usage_nsec: 0,
            },
        ];

        let rules = default_rules();
        let desktop_by_exec = HashMap::from([
            (
                "snap:firefox".to_string(),
                vec!["firefox_firefox.desktop".to_string()],
            ),
            (
                "snap:code".to_string(),
                vec!["code_code.desktop".to_string()],
            ),
        ]);

        let suggestions = build_suggestions(&observations, &rules, &desktop_by_exec);
        let plan = build_plan_items(&suggestions, 70, "ubuntu-desktop");

        assert!(plan.iter().any(|p| {
            matches!(
                p,
                SuggestPlanItem::WrapDesktop {
                    desktop_id,
                    class,
                    ..
                } if desktop_id == "firefox_firefox.desktop" && class == "browsers"
            )
        }));
        assert!(plan.iter().any(|p| {
            matches!(
                p,
                SuggestPlanItem::WrapDesktop {
                    desktop_id,
                    class,
                    ..
                } if desktop_id == "code_code.desktop" && class == "ide"
            )
        }));
    }

    #[test]
    fn suggest_apply_executes_wrap_for_strong_snap_matches() {
        let observations = vec![ScopeObservation {
            scope: "app-snap.firefox.firefox-1234.scope".to_string(),
            slice: "app.slice".to_string(),
            exec_start: "/usr/bin/snap run firefox".to_string(),
            memory_current: 600 * 1024 * 1024,
            cpu_usage_nsec: 0,
        }];

        let rules = default_rules();
        let desktop_by_exec = HashMap::from([(
            "snap:firefox".to_string(),
            vec!["firefox_firefox.desktop".to_string()],
        )]);

        let suggestions = build_suggestions(&observations, &rules, &desktop_by_exec);
        let plan = build_plan_items(&suggestions, 70, "ubuntu-desktop");

        let mut wrapped: Vec<(String, String)> = Vec::new();
        execute_plan_items(&plan, false, &mut |desktop_id, class, _force| {
            wrapped.push((desktop_id.to_string(), class.to_string()));
            Ok(())
        });

        assert_eq!(
            wrapped,
            vec![(
                "firefox_firefox.desktop".to_string(),
                "browsers".to_string()
            )]
        );
    }

    #[test]
    fn weak_or_ambiguous_match_stays_conservative() {
        let observations = vec![ScopeObservation {
            scope: "app-random.scope".to_string(),
            slice: "app.slice".to_string(),
            exec_start: "/usr/bin/unknown-firefox".to_string(),
            memory_current: 128 * 1024 * 1024,
            cpu_usage_nsec: 0,
        }];

        let rules = default_rules();
        let desktop_by_exec: HashMap<String, Vec<String>> = HashMap::new();

        let suggestions = build_suggestions(&observations, &rules, &desktop_by_exec);
        let plan = build_plan_items(&suggestions, 70, "ubuntu-desktop");

        assert!(plan.iter().any(|p| {
            matches!(
                p,
                SuggestPlanItem::SkipLowConfidence {
                    confidence,
                    threshold,
                    ..
                } if *confidence < *threshold
            )
        }));
    }

    fn default_rules() -> Vec<SuggestRule> {
        vec![
            SuggestRule {
                pattern: "(?i)firefox|chrome|chromium|brave|vivaldi|opera".to_string(),
                class: "browsers".to_string(),
            },
            SuggestRule {
                pattern: "(?i)code|codium|idea|pycharm|clion|goland".to_string(),
                class: "ide".to_string(),
            },
        ]
    }
}
