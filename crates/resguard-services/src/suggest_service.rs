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

#[derive(Debug, Clone)]
pub struct SuggestRequest {
    pub format: String,
    pub apply: bool,
    pub dry_run: bool,
    pub confidence_threshold: u8,
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

    let desktop_by_exec = build_desktop_exec_index();
    let scopes = match systemctl_list_units(true, "scope") {
        Ok(v) => v,
        Err(err) => {
            eprintln!("warn: could not query user scopes: {err}");
            return Ok(1);
        }
    };

    let mut suggestions = Vec::new();
    for scope in scopes.into_iter().filter(|u| u.ends_with(".scope")) {
        let props = match systemctl_show_props(
            true,
            &scope,
            &["MemoryCurrent", "CPUUsageNSec", "Slice", "ExecStart", "Id"],
        ) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let exec_start = props.get("ExecStart").cloned().unwrap_or_default();
        let slice = props
            .get("Slice")
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        let memory_current = props
            .get("MemoryCurrent")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let cpu_usage_nsec = props
            .get("CPUUsageNSec")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let Some(classified) = classify_scope(&scope, &slice, &exec_start, memory_current, &rules)
        else {
            continue;
        };

        let identity = parse_scope_identity(&scope, &exec_start);
        let desktop_id = unique_desktop_id_for_scope_exec(&scope, &exec_start, &desktop_by_exec);
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
            scope,
            class: classified.class,
            reason: SuggestionReason::Manual {
                message: classified.reason,
            },
            slice,
            exec_start,
            memory_current,
            cpu_usage_nsec,
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

    if suggestions.is_empty() {
        println!("result=no-suggestions");
        println!("hint=run workload, then retry: resguard suggest");
        return Ok(0);
    }

    match req.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&suggestions)?),
        "yaml" => println!("{}", serde_yaml::to_string(&suggestions)?),
        _ => print_suggestions_table(&suggestions),
    }

    if req.apply || req.dry_run {
        println!();
        if req.dry_run {
            println!("dry_run_preview");
        } else {
            println!("apply_results");
        }
        for s in &suggestions {
            if !meets_confidence_threshold(s.confidence, req.confidence_threshold) {
                println!(
                    "skip\t{}\t{}\tconfidence {} below threshold {} ({})",
                    s.scope, s.class, s.confidence, req.confidence_threshold, s.confidence_reason
                );
                continue;
            }
            if let Some(desktop_id) = &s.desktop_id {
                if req.dry_run {
                    println!(
                        "would-wrap\t{}\t{}\tconfidence={}",
                        desktop_id, s.class, s.confidence
                    );
                } else {
                    match wrap_desktop(desktop_id, &s.class, false) {
                        Ok(()) => println!("ok\t{}\t{}\twrapped", desktop_id, s.class),
                        Err(err) => println!("warn\t{}\t{}\t{}", desktop_id, s.class, err),
                    }
                }
            } else {
                let profile_hint = resolved_profile_name.as_deref().unwrap_or("<profile>");
                println!(
                    "hint\t{}\t{}\tno unique desktop_id match (confidence={} {}); wrap manually: resguard desktop list --filter '{}' && resguard desktop wrap <desktop_id> --class {} (then sudo resguard apply {} --user-daemon-reload)",
                    s.scope,
                    s.class,
                    s.confidence,
                    s.confidence_reason,
                    parse_first_exec_token(&s.exec_start).unwrap_or_else(|| s.scope.clone()),
                    s.class,
                    profile_hint
                );
            }
        }
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

fn print_suggestions_table(items: &[Suggestion]) {
    println!("scope\tclass\tdesktop_id\tmemory\tconfidence\treason");
    for s in items {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            s.scope,
            s.class,
            s.desktop_id.clone().unwrap_or_else(|| "-".to_string()),
            s.memory_current,
            s.confidence,
            s.confidence_reason
        );
    }
}
