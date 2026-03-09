use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetupSuggestSummary {
    pub total: usize,
    pub strong_auto_wrap: usize,
    pub strong_manual_review: usize,
    pub low_confidence: usize,
    pub planned_wraps: Vec<String>,
    pub manual_review_hints: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn setup<FI, FA, FS>(
    profile_name: String,
    apply: bool,
    suggest: bool,
    plan_wraps: bool,
    mut init: FI,
    mut apply_fn: FA,
    mut suggest_fn: FS,
) -> Result<i32>
where
    FI: FnMut(&str) -> Result<i32>,
    FA: FnMut(&str) -> Result<i32>,
    FS: FnMut(&str) -> Result<SetupSuggestSummary>,
{
    println!("command=setup");
    println!(
        "profile={} apply={} suggest={} plan_wraps={}",
        profile_name, apply, suggest, plan_wraps
    );

    let init_code = init(&profile_name)?;
    if init_code != 0 {
        return Ok(init_code);
    }

    let mut apply_code = 0;
    if apply {
        apply_code = apply_fn(&profile_name)?;
        if apply_code != 0 {
            println!("setup.result=partial");
            println!("setup.apply_exit={}", apply_code);
            println!("rollback_hint=resguard rollback --last");
            return Ok(apply_code);
        }
    }

    if suggest {
        let summary = suggest_fn(&profile_name)?;
        println!("setup.suggest_preview=ok");
        println!("setup.suggest.total={}", summary.total);
        println!(
            "setup.suggest.strong_auto_wrap={}",
            summary.strong_auto_wrap
        );
        println!(
            "setup.suggest.strong_manual_review={}",
            summary.strong_manual_review
        );
        println!("setup.suggest.low_confidence={}", summary.low_confidence);
        if plan_wraps {
            for line in &summary.planned_wraps {
                println!("setup.auto_wrap.plan={line}");
            }
        }
        for hint in &summary.manual_review_hints {
            println!("setup.manual_review.hint={hint}");
        }
        for warning in &summary.warnings {
            println!("setup.warning={warning}");
        }
    }

    println!("setup.result=ok");
    println!("setup.profile={}", profile_name);
    println!("setup.apply_executed={}", apply);
    println!("setup.zero_config=true");
    println!("changed=profile_initialized_and_apply_executed");
    println!("rollback_hint=resguard rollback --last");
    println!("user_reload_hint=systemctl --user daemon-reload");
    println!("followup=review preview: resguard suggest --dry-run");
    println!("followup=safe auto for strong matches: resguard suggest --auto");
    println!("followup=apply strong matches: resguard suggest --apply");
    println!("followup=manual wrap fallback: resguard desktop list --filter <app> && resguard desktop wrap <desktop_id> --class <class>");
    Ok(if apply { apply_code } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::{setup, SetupSuggestSummary};

    #[test]
    fn setup_reports_safe_zero_config_summary() {
        let mut init_called = 0usize;
        let mut apply_called = 0usize;
        let mut suggest_called = 0usize;
        let code = setup(
            "auto".to_string(),
            true,
            true,
            true,
            |_| {
                init_called += 1;
                Ok(0)
            },
            |_| {
                apply_called += 1;
                Ok(0)
            },
            |_| {
                suggest_called += 1;
                Ok(SetupSuggestSummary {
                    total: 3,
                    strong_auto_wrap: 2,
                    strong_manual_review: 1,
                    low_confidence: 0,
                    planned_wraps: vec!["firefox_firefox.desktop -> browsers".to_string()],
                    manual_review_hints: vec!["jetbrains ambiguous desktop id".to_string()],
                    warnings: vec![],
                })
            },
        )
        .expect("setup");

        assert_eq!(code, 0);
        assert_eq!(init_called, 1);
        assert_eq!(apply_called, 1);
        assert_eq!(suggest_called, 1);
    }

    #[test]
    fn setup_keeps_ambiguous_cases_conservative() {
        let code = setup(
            "auto".to_string(),
            true,
            true,
            true,
            |_| Ok(0),
            |_| Ok(0),
            |_| {
                Ok(SetupSuggestSummary {
                    total: 2,
                    strong_auto_wrap: 0,
                    strong_manual_review: 1,
                    low_confidence: 1,
                    planned_wraps: vec![],
                    manual_review_hints: vec![
                        "scope-x strong but no unique desktop".to_string(),
                        "scope-y confidence 40 below threshold".to_string(),
                    ],
                    warnings: vec![],
                })
            },
        )
        .expect("setup");
        assert_eq!(code, 0);
    }

    #[test]
    fn setup_handles_no_desktop_data_gracefully() {
        let code = setup(
            "auto".to_string(),
            true,
            true,
            false,
            |_| Ok(0),
            |_| Ok(0),
            |_| {
                Ok(SetupSuggestSummary {
                    warnings: vec!["could not query user scopes".to_string()],
                    ..SetupSuggestSummary::default()
                })
            },
        )
        .expect("setup");
        assert_eq!(code, 0);
    }
}
