use crate::*;

pub(crate) fn handle_suggest(req: SuggestRequest) -> Result<i32> {
    let SuggestRequest {
        format,
        root,
        config_dir,
        state_dir,
        profile,
        apply,
        dry_run,
        confidence_threshold,
    } = req;

    if apply && dry_run {
        return Err(anyhow!(
            "invalid arguments: --apply and --dry-run cannot be combined"
        ));
    }
    if confidence_threshold > 100 {
        return Err(anyhow!("invalid --confidence-threshold: must be 0..=100"));
    }

    println!("command=suggest");
    println!(
        "apply={} dry_run={} confidence_threshold={} profile={:?}",
        apply, dry_run, confidence_threshold, profile
    );

    let (resolved_profile_name, resolved_profile) =
        resolve_suggest_profile(&root, &config_dir, &state_dir, profile.as_deref())?;
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
    let scopes = match systemctl_user_scope_units() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("warn: could not query user scopes: {err}");
            return Ok(1);
        }
    };

    let mut suggestions = Vec::new();
    for scope in scopes {
        let props = match systemctl_user_show_scope(&scope) {
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

        let desktop_id = unique_desktop_id_for_scope_exec(&scope, &exec_start, &desktop_by_exec);
        let strong_identity = strong_app_identity_match(&scope, &exec_start, &classified.class);
        let (confidence, confidence_reason) = confidence_score(
            classified.pattern_match,
            classified.memory_threshold_match,
            desktop_id.is_some(),
            strong_identity,
        );

        suggestions.push(Suggestion {
            scope,
            class: classified.class,
            reason: classified.reason,
            slice,
            exec_start,
            memory_current,
            cpu_usage_nsec,
            desktop_id,
            confidence,
            confidence_reason,
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

    match format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&suggestions)?),
        "yaml" => println!("{}", serde_yaml::to_string(&suggestions)?),
        _ => print_suggestions_table(&suggestions),
    }

    if apply || dry_run {
        println!();
        if dry_run {
            println!("dry_run_preview");
        } else {
            println!("apply_results");
        }
        for s in &suggestions {
            if s.confidence < confidence_threshold {
                println!(
                    "skip\t{}\t{}\tconfidence {} below threshold {} ({})",
                    s.scope, s.class, s.confidence, confidence_threshold, s.confidence_reason
                );
                continue;
            }
            if let Some(desktop_id) = &s.desktop_id {
                let wrapper_path = wrapper_path_for(desktop_id, &s.class)?;
                if wrapper_path.exists() {
                    println!(
                        "skip\t{}\t{}\talready wrapped ({})",
                        desktop_id,
                        s.class,
                        wrapper_path.display()
                    );
                    continue;
                }

                if dry_run {
                    println!(
                        "would-wrap\t{}\t{}\tconfidence={}\tpath={}",
                        desktop_id,
                        s.class,
                        s.confidence,
                        wrapper_path.display()
                    );
                } else {
                    match handle_desktop_wrap(
                        desktop_id,
                        &s.class,
                        DesktopWrapOptions {
                            force: false,
                            dry_run: false,
                            print_only: false,
                            override_mode: false,
                        },
                    ) {
                        Ok(0) => println!("ok\t{}\t{}\twrapped", desktop_id, s.class),
                        Ok(code) => {
                            println!("warn\t{}\t{}\twrap returned {}", desktop_id, s.class, code)
                        }
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
        println!("3) apply profile so user slices exist: sudo resguard apply <profile> --user-daemon-reload");
    }

    Ok(0)
}
