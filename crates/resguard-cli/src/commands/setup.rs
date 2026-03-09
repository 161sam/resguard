use crate::cli::{ApplyOptions, SuggestRequest};
use crate::*;
use resguard_services::setup_service::SetupSuggestSummary;
use resguard_services::suggest_service::suggest_preview_summary;

pub(crate) fn handle_setup(
    format: &str,
    root: &str,
    config_dir: &str,
    state_dir: &str,
    name: Option<String>,
    apply: bool,
    suggest: bool,
    plan_wraps: bool,
) -> Result<i32> {
    let profile_name = name.unwrap_or_else(|| "auto".to_string());

    resguard_services::setup_service::setup(
        profile_name.clone(),
        apply,
        suggest,
        plan_wraps,
        |profile_name| {
            let setup_out = profile_path(config_dir, profile_name)?
                .to_string_lossy()
                .to_string();
            handle_init(
                root,
                config_dir,
                state_dir,
                Some(profile_name.to_string()),
                Some(setup_out),
                false,
                false,
            )
        },
        |profile_name| {
            handle_apply(
                root,
                config_dir,
                state_dir,
                profile_name,
                &ApplyOptions {
                    dry_run: false,
                    no_oomd: false,
                    no_cpu: false,
                    no_classes: false,
                    force: false,
                    user_daemon_reload: false,
                },
            )
        },
        |profile_name| {
            let req = SuggestRequest {
                format: format.to_string(),
                root: root.to_string(),
                config_dir: config_dir.to_string(),
                state_dir: state_dir.to_string(),
                profile: Some(profile_name.to_string()),
                apply: false,
                auto: false,
                dry_run: true,
                confidence_threshold: 70,
            };
            let summary = suggest_preview_summary(
                &resguard_services::suggest_service::SuggestRequest {
                    format: req.format.clone(),
                    apply: req.apply,
                    auto: req.auto,
                    dry_run: req.dry_run,
                    confidence_threshold: req.confidence_threshold,
                },
                || {
                    resolve_suggest_profile(
                        &req.root,
                        &req.config_dir,
                        &req.state_dir,
                        req.profile.as_deref(),
                    )
                },
            )?;
            Ok(SetupSuggestSummary {
                total: summary.total,
                strong_auto_wrap: summary.strong_auto_wrap,
                strong_manual_review: summary.strong_manual_review,
                low_confidence: summary.low_confidence,
                planned_wraps: summary.planned_wraps,
                manual_review_hints: summary.manual_review_hints,
                warnings: summary.warnings,
            })
        },
    )
}

pub(crate) fn run(
    format: &str,
    root: &str,
    config_dir: &str,
    state_dir: &str,
    name: Option<String>,
    apply: bool,
    suggest: bool,
    plan_wraps: bool,
) -> Result<i32> {
    handle_setup(
        format, root, config_dir, state_dir, name, apply, suggest, plan_wraps,
    )
}
