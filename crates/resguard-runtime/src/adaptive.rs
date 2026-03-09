use crate::cgroup::parse_prop_u64;
use crate::systemd::{systemctl_set_slice_limits, systemctl_show_props};
use anyhow::{anyhow, Result};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassLimitCurrent {
    pub memory_high: Option<String>,
    pub memory_max: Option<String>,
    pub cpu_weight: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassLimitRequest {
    pub class: String,
    pub slice: String,
    pub user: bool,
    pub memory_high: Option<String>,
    pub memory_max: Option<String>,
    pub cpu_weight: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedClassLimitChange {
    pub class: String,
    pub slice: String,
    pub user: bool,
    pub previous: ClassLimitCurrent,
    pub target_memory_high: Option<String>,
    pub target_memory_max: Option<String>,
    pub target_cpu_weight: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdaptiveChangePlan {
    pub changes: Vec<PlannedClassLimitChange>,
    pub skipped_noop: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdaptiveRevertStep {
    pub class: String,
    pub slice: String,
    pub user: bool,
    pub restore_memory_high: Option<String>,
    pub restore_memory_max: Option<String>,
    pub restore_cpu_weight: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdaptiveRevertPlan {
    pub steps: Vec<AdaptiveRevertStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdaptiveApplyResult {
    pub applied: Vec<String>,
    pub skipped_noop: Vec<String>,
    pub warnings: Vec<String>,
    pub revert_plan: AdaptiveRevertPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdaptiveRevertResult {
    pub reverted: Vec<String>,
    pub warnings: Vec<String>,
}

fn request_id(req: &ClassLimitRequest) -> String {
    format!(
        "{}:{}:{}",
        if req.user { "user" } else { "system" },
        req.class,
        req.slice
    )
}

pub fn read_class_limit_current(user: bool, slice: &str) -> Result<ClassLimitCurrent> {
    let props = systemctl_show_props(user, slice, &["MemoryHigh", "MemoryMax", "CPUWeight"])?;
    Ok(ClassLimitCurrent {
        memory_high: props
            .get("MemoryHigh")
            .cloned()
            .filter(|v| !v.is_empty() && v != "[not set]"),
        memory_max: props
            .get("MemoryMax")
            .cloned()
            .filter(|v| !v.is_empty() && v != "[not set]"),
        cpu_weight: parse_prop_u64(&props, "CPUWeight").and_then(|v| u16::try_from(v).ok()),
    })
}

pub fn plan_class_limit_changes(requests: &[ClassLimitRequest]) -> Result<AdaptiveChangePlan> {
    plan_class_limit_changes_with(requests, read_class_limit_current)
}

pub fn plan_class_limit_changes_with<F>(
    requests: &[ClassLimitRequest],
    mut read_current: F,
) -> Result<AdaptiveChangePlan>
where
    F: FnMut(bool, &str) -> Result<ClassLimitCurrent>,
{
    let mut out = AdaptiveChangePlan::default();
    for req in requests {
        if req.memory_high.is_none() && req.memory_max.is_none() && req.cpu_weight.is_none() {
            return Err(anyhow!(
                "class limit request for '{}' has no target values",
                request_id(req)
            ));
        }

        let current = read_current(req.user, &req.slice)?;
        let mut changed = false;

        if let Some(v) = req.memory_high.as_deref() {
            changed |= current.memory_high.as_deref() != Some(v);
        }
        if let Some(v) = req.memory_max.as_deref() {
            changed |= current.memory_max.as_deref() != Some(v);
        }
        if let Some(v) = req.cpu_weight {
            changed |= current.cpu_weight != Some(v);
        }

        if changed {
            out.changes.push(PlannedClassLimitChange {
                class: req.class.clone(),
                slice: req.slice.clone(),
                user: req.user,
                previous: current,
                target_memory_high: req.memory_high.clone(),
                target_memory_max: req.memory_max.clone(),
                target_cpu_weight: req.cpu_weight,
            });
        } else {
            out.skipped_noop.push(request_id(req));
        }
    }
    Ok(out)
}

pub fn apply_class_limit_changes(plan: &AdaptiveChangePlan) -> Result<AdaptiveApplyResult> {
    apply_class_limit_changes_with(plan, |user, slice, memory_high, memory_max, cpu_weight| {
        systemctl_set_slice_limits(user, slice, memory_high, memory_max, cpu_weight)
    })
}

pub fn apply_class_limit_changes_with<F>(
    plan: &AdaptiveChangePlan,
    mut apply_one: F,
) -> Result<AdaptiveApplyResult>
where
    F: FnMut(bool, &str, Option<&str>, Option<&str>, Option<u16>) -> Result<()>,
{
    let mut out = AdaptiveApplyResult {
        skipped_noop: plan.skipped_noop.clone(),
        ..AdaptiveApplyResult::default()
    };

    for change in &plan.changes {
        apply_one(
            change.user,
            &change.slice,
            change.target_memory_high.as_deref(),
            change.target_memory_max.as_deref(),
            change.target_cpu_weight,
        )?;
        out.applied.push(format!(
            "{}:{}:{}",
            if change.user { "user" } else { "system" },
            change.class,
            change.slice
        ));
        out.revert_plan.steps.push(AdaptiveRevertStep {
            class: change.class.clone(),
            slice: change.slice.clone(),
            user: change.user,
            restore_memory_high: if change.target_memory_high.is_some() {
                change.previous.memory_high.clone()
            } else {
                None
            },
            restore_memory_max: if change.target_memory_max.is_some() {
                change.previous.memory_max.clone()
            } else {
                None
            },
            restore_cpu_weight: if change.target_cpu_weight.is_some() {
                change.previous.cpu_weight
            } else {
                None
            },
        });
    }

    Ok(out)
}

pub fn revert_class_limit_changes(plan: &AdaptiveRevertPlan) -> Result<AdaptiveRevertResult> {
    revert_class_limit_changes_with(plan, |user, slice, memory_high, memory_max, cpu_weight| {
        systemctl_set_slice_limits(user, slice, memory_high, memory_max, cpu_weight)
    })
}

pub fn revert_class_limit_changes_with<F>(
    plan: &AdaptiveRevertPlan,
    mut apply_one: F,
) -> Result<AdaptiveRevertResult>
where
    F: FnMut(bool, &str, Option<&str>, Option<&str>, Option<u16>) -> Result<()>,
{
    let mut out = AdaptiveRevertResult::default();

    for step in &plan.steps {
        if step.restore_memory_high.is_none()
            && step.restore_memory_max.is_none()
            && step.restore_cpu_weight.is_none()
        {
            out.warnings.push(format!(
                "no revertable properties for {}:{}:{}",
                if step.user { "user" } else { "system" },
                step.class,
                step.slice
            ));
            continue;
        }

        apply_one(
            step.user,
            &step.slice,
            step.restore_memory_high.as_deref(),
            step.restore_memory_max.as_deref(),
            step.restore_cpu_weight,
        )?;
        out.reverted.push(format!(
            "{}:{}:{}",
            if step.user { "user" } else { "system" },
            step.class,
            step.slice
        ));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_class_limit_changes_with, plan_class_limit_changes_with,
        revert_class_limit_changes_with, AdaptiveChangePlan, AdaptiveRevertPlan, ClassLimitCurrent,
        ClassLimitRequest,
    };
    use std::collections::BTreeMap;

    #[test]
    fn class_limit_change_planning_detects_changes() {
        let req = ClassLimitRequest {
            class: "browsers".to_string(),
            slice: "resguard-browsers.slice".to_string(),
            user: true,
            memory_high: Some("3G".to_string()),
            memory_max: Some("4G".to_string()),
            cpu_weight: None,
        };

        let plan = plan_class_limit_changes_with(&[req], |_user, _slice| {
            Ok(ClassLimitCurrent {
                memory_high: Some("4G".to_string()),
                memory_max: Some("5G".to_string()),
                cpu_weight: Some(80),
            })
        })
        .expect("plan");

        assert_eq!(plan.changes.len(), 1);
        assert!(plan.skipped_noop.is_empty());
        assert_eq!(plan.changes[0].target_memory_high.as_deref(), Some("3G"));
        assert_eq!(plan.changes[0].target_memory_max.as_deref(), Some("4G"));
    }

    #[test]
    fn no_op_when_values_are_unchanged() {
        let req = ClassLimitRequest {
            class: "heavy".to_string(),
            slice: "resguard-heavy.slice".to_string(),
            user: true,
            memory_high: None,
            memory_max: None,
            cpu_weight: Some(70),
        };

        let plan = plan_class_limit_changes_with(&[req], |_user, _slice| {
            Ok(ClassLimitCurrent {
                memory_high: Some("6G".to_string()),
                memory_max: Some("8G".to_string()),
                cpu_weight: Some(70),
            })
        })
        .expect("plan");

        assert!(plan.changes.is_empty());
        assert_eq!(plan.skipped_noop.len(), 1);
    }

    #[test]
    fn apply_and_revert_flow_is_explicit() {
        let plan = AdaptiveChangePlan {
            changes: vec![super::PlannedClassLimitChange {
                class: "heavy".to_string(),
                slice: "resguard-heavy.slice".to_string(),
                user: true,
                previous: ClassLimitCurrent {
                    memory_high: None,
                    memory_max: None,
                    cpu_weight: Some(90),
                },
                target_memory_high: None,
                target_memory_max: None,
                target_cpu_weight: Some(70),
            }],
            skipped_noop: Vec::new(),
        };

        let mut applied_calls = Vec::new();
        let apply = apply_class_limit_changes_with(
            &plan,
            |user, slice, memory_high, memory_max, cpu_weight| {
                applied_calls.push((
                    user,
                    slice.to_string(),
                    memory_high.map(|v| v.to_string()),
                    memory_max.map(|v| v.to_string()),
                    cpu_weight,
                ));
                Ok(())
            },
        )
        .expect("apply");

        assert_eq!(apply.applied.len(), 1);
        assert_eq!(apply.revert_plan.steps.len(), 1);
        assert_eq!(applied_calls.len(), 1);
        assert_eq!(applied_calls[0].0, true);
        assert_eq!(applied_calls[0].1, "resguard-heavy.slice");
        assert_eq!(applied_calls[0].4, Some(70));

        let mut reverted_calls = Vec::new();
        let reverted = revert_class_limit_changes_with(
            &AdaptiveRevertPlan {
                steps: apply.revert_plan.steps.clone(),
            },
            |user, slice, memory_high, memory_max, cpu_weight| {
                reverted_calls.push((
                    user,
                    slice.to_string(),
                    memory_high.map(|v| v.to_string()),
                    memory_max.map(|v| v.to_string()),
                    cpu_weight,
                ));
                Ok(())
            },
        )
        .expect("revert");

        assert_eq!(reverted.reverted.len(), 1);
        assert_eq!(reverted_calls.len(), 1);
        assert_eq!(reverted_calls[0].4, Some(90));
    }

    #[test]
    fn apply_preserves_noop_reporting() {
        let plan = AdaptiveChangePlan {
            changes: Vec::new(),
            skipped_noop: vec!["user:browsers:resguard-browsers.slice".to_string()],
        };
        let apply = apply_class_limit_changes_with(
            &plan,
            |_user, _slice, _memory_high, _memory_max, _cpu_weight| Ok(()),
        )
        .expect("apply");
        assert!(apply.applied.is_empty());
        assert_eq!(
            apply.skipped_noop,
            vec!["user:browsers:resguard-browsers.slice".to_string()]
        );
    }

    #[test]
    fn plan_uses_reader_per_request() {
        let requests = vec![
            ClassLimitRequest {
                class: "browsers".to_string(),
                slice: "resguard-browsers.slice".to_string(),
                user: true,
                memory_high: Some("3G".to_string()),
                memory_max: None,
                cpu_weight: None,
            },
            ClassLimitRequest {
                class: "heavy".to_string(),
                slice: "resguard-heavy.slice".to_string(),
                user: true,
                memory_high: None,
                memory_max: None,
                cpu_weight: Some(70),
            },
        ];

        let mut current = BTreeMap::new();
        current.insert(
            "resguard-browsers.slice".to_string(),
            ClassLimitCurrent {
                memory_high: Some("4G".to_string()),
                memory_max: Some("6G".to_string()),
                cpu_weight: Some(80),
            },
        );
        current.insert(
            "resguard-heavy.slice".to_string(),
            ClassLimitCurrent {
                memory_high: Some("6G".to_string()),
                memory_max: Some("8G".to_string()),
                cpu_weight: Some(70),
            },
        );

        let plan = plan_class_limit_changes_with(&requests, |_user, slice| {
            current
                .get(slice)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing test current"))
        })
        .expect("plan");

        assert_eq!(plan.changes.len(), 1);
        assert_eq!(plan.skipped_noop.len(), 1);
        assert_eq!(plan.changes[0].class, "browsers");
    }
}
