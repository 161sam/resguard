use anyhow::Result;
use resguard_config::load_profile_from_store;
use resguard_core::parse_size_to_bytes;
use resguard_model::{MetricsSnapshot, Profile};
use resguard_policy::{
    decide_autopilot_actions, AutopilotAction, AutopilotState, AutopilotTransition,
};
use resguard_runtime::{
    apply_class_limit_changes, plan_class_limit_changes, read_system_snapshot,
    revert_class_limit_changes, AdaptiveRevertPlan, ClassLimitRequest, SystemSnapshot,
};
use resguard_state::read_state;
use std::path::Path;

use resguard_runtime::{check_command_success, systemctl_service_action};

pub fn daemon_enable() -> Result<i32> {
    println!("command=daemon enable");
    if systemctl_service_action("enable", "resguardd.service")? {
        println!("result=ok action=enable service=resguardd.service");
        Ok(0)
    } else {
        eprintln!("systemctl enable resguardd.service failed");
        Ok(1)
    }
}

pub fn daemon_disable() -> Result<i32> {
    println!("command=daemon disable");
    if systemctl_service_action("disable", "resguardd.service")? {
        println!("result=ok action=disable service=resguardd.service");
        Ok(0)
    } else {
        eprintln!("systemctl disable resguardd.service failed");
        Ok(1)
    }
}

pub fn daemon_status() -> Result<i32> {
    println!("command=daemon status");
    let enabled = check_command_success("systemctl", &["is-enabled", "resguardd.service"]);
    let active = check_command_success("systemctl", &["is-active", "resguardd.service"]);
    println!("resguardd.enabled={}", enabled);
    println!("resguardd.active={}", active);
    if !enabled {
        println!("fix: sudo systemctl enable resguardd.service");
    }
    if !active {
        println!("fix: sudo systemctl start resguardd.service");
    }
    Ok(if enabled && active { 0 } else { 1 })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonAutopilotState {
    pub tick: u64,
    pub policy: AutopilotState,
    pub pending_revert: AdaptiveRevertPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonAutopilotTick {
    pub decision_actions: Vec<String>,
    pub transition: Option<String>,
    pub applied: Vec<String>,
    pub reverted: Vec<String>,
    pub skipped_noop: Vec<String>,
    pub warnings: Vec<String>,
    pub in_cooldown: bool,
    pub had_profile: bool,
}

pub fn daemon_autopilot_tick(
    config_dir: &str,
    state_dir: &str,
    state: &mut DaemonAutopilotState,
) -> Result<DaemonAutopilotTick> {
    daemon_autopilot_tick_with(
        || Ok(read_system_snapshot()),
        || load_active_profile(config_dir, state_dir),
        |requests| {
            let plan = plan_class_limit_changes(requests)?;
            apply_class_limit_changes(&plan)
        },
        revert_class_limit_changes,
        state,
    )
}

fn snapshot_to_metrics(s: &SystemSnapshot) -> MetricsSnapshot {
    MetricsSnapshot {
        memory_pressure: s.memory_pressure,
        cpu_pressure: s.cpu_pressure,
        io_pressure: s.io_pressure,
        memory_current_bytes: None,
        memory_available_bytes: s.mem_available_bytes,
        cpu_usage_nsec: None,
    }
}

fn load_active_profile(config_dir: &str, state_dir: &str) -> Result<Option<Profile>> {
    let state_root = Path::new(state_dir);
    let st = match read_state(state_root) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let Some(name) = st.active_profile else {
        return Ok(None);
    };
    Ok(Some(load_profile_from_store(config_dir, &name)?))
}

fn class_slice(profile: &Profile, class: &str) -> Option<String> {
    profile.spec.classes.get(class).map(|c| {
        c.slice_name
            .clone()
            .unwrap_or_else(|| format!("resguard-{class}.slice"))
    })
}

fn format_bytes_binary(bytes: u64) -> String {
    let gb = 1024_u64.pow(3);
    let mb = 1024_u64.pow(2);
    if bytes >= gb {
        format!("{}G", bytes / gb)
    } else if bytes >= mb {
        format!("{}M", bytes / mb)
    } else {
        bytes.to_string()
    }
}

fn scaled_value(base: &str, pct: u8) -> Option<String> {
    let b = parse_size_to_bytes(base).ok()?;
    let scaled = b.saturating_mul(pct as u64) / 100;
    Some(format_bytes_binary(scaled))
}

fn build_requests(
    profile: &Profile,
    actions: &[AutopilotAction],
) -> (Vec<ClassLimitRequest>, Vec<String>) {
    let mut reqs = Vec::new();
    let mut warnings = Vec::new();
    for action in actions {
        match action {
            AutopilotAction::ReduceBrowsersLimits {
                memory_high_percent,
                memory_max_percent,
            } => {
                let Some(slice) = class_slice(profile, "browsers") else {
                    continue;
                };
                let class = profile.spec.classes.get("browsers");
                let high = class
                    .and_then(|c| c.memory_high.as_deref())
                    .and_then(|v| scaled_value(v, *memory_high_percent));
                let max = class
                    .and_then(|c| c.memory_max.as_deref())
                    .and_then(|v| scaled_value(v, *memory_max_percent));
                if high.is_none() && max.is_none() {
                    warnings.push(
                        "browsers memory limits missing; skip adaptive memory reduction"
                            .to_string(),
                    );
                    continue;
                }
                reqs.push(ClassLimitRequest {
                    class: "browsers".to_string(),
                    slice,
                    user: true,
                    memory_high: high,
                    memory_max: max,
                    cpu_weight: None,
                });
            }
            AutopilotAction::ReduceHeavyCpuWeight { cpu_weight } => {
                let Some(slice) = class_slice(profile, "heavy") else {
                    continue;
                };
                reqs.push(ClassLimitRequest {
                    class: "heavy".to_string(),
                    slice,
                    user: true,
                    memory_high: None,
                    memory_max: None,
                    cpu_weight: Some(*cpu_weight),
                });
            }
            AutopilotAction::ReduceHeavyIoWeight { io_weight } => {
                let Some(slice) = class_slice(profile, "heavy") else {
                    continue;
                };
                reqs.push(ClassLimitRequest {
                    class: "heavy".to_string(),
                    slice,
                    user: true,
                    memory_high: None,
                    memory_max: None,
                    cpu_weight: Some(*io_weight),
                });
            }
            AutopilotAction::RevertAdaptiveLimits => {}
            AutopilotAction::PreserveRescueClass => {}
        }
    }
    (reqs, warnings)
}

fn action_name(a: &AutopilotAction) -> String {
    match a {
        AutopilotAction::ReduceBrowsersLimits { .. } => "reduce-browsers-limits".to_string(),
        AutopilotAction::ReduceHeavyCpuWeight { .. } => "reduce-heavy-cpuweight".to_string(),
        AutopilotAction::ReduceHeavyIoWeight { .. } => "reduce-heavy-ioweight".to_string(),
        AutopilotAction::RevertAdaptiveLimits => "revert-adaptive-limits".to_string(),
        AutopilotAction::PreserveRescueClass => "preserve-rescue-class".to_string(),
    }
}

fn daemon_autopilot_tick_with<O, P, A, R>(
    observe: O,
    load_profile: P,
    apply: A,
    revert: R,
    state: &mut DaemonAutopilotState,
) -> Result<DaemonAutopilotTick>
where
    O: FnOnce() -> Result<SystemSnapshot>,
    P: FnOnce() -> Result<Option<Profile>>,
    A: FnOnce(&[ClassLimitRequest]) -> Result<resguard_runtime::AdaptiveApplyResult>,
    R: FnOnce(&AdaptiveRevertPlan) -> Result<resguard_runtime::AdaptiveRevertResult>,
{
    state.tick = state.tick.saturating_add(1);
    let snap = observe()?;
    let metrics = snapshot_to_metrics(&snap);
    let profile = load_profile()?;

    let Some(profile) = profile else {
        return Ok(DaemonAutopilotTick {
            warnings: vec!["no active profile; daemon autopilot idle".to_string()],
            had_profile: false,
            ..DaemonAutopilotTick::default()
        });
    };

    let decision = decide_autopilot_actions(&metrics, &state.policy, &profile, state.tick);
    state.policy = decision.next_state;
    let mut out = DaemonAutopilotTick {
        decision_actions: decision.actions.iter().map(action_name).collect(),
        transition: Some(
            match decision.transition {
                AutopilotTransition::StayIdle => "stay-idle",
                AutopilotTransition::TriggerToCooldown => "trigger-to-cooldown",
                AutopilotTransition::StayCooldown => "stay-cooldown",
                AutopilotTransition::CooldownToRevertWindow => "cooldown-to-revert-window",
                AutopilotTransition::RevertWindowToIdle => "revert-window-to-idle",
            }
            .to_string(),
        ),
        in_cooldown: decision.in_cooldown,
        had_profile: true,
        ..DaemonAutopilotTick::default()
    };

    if decision.actions.is_empty() {
        return Ok(out);
    }

    if decision
        .actions
        .iter()
        .any(|a| matches!(a, AutopilotAction::RevertAdaptiveLimits))
    {
        if state.pending_revert.steps.is_empty() {
            out.warnings
                .push("revert requested but no pending adaptive changes".to_string());
            return Ok(out);
        }
        let revert_out = revert(&state.pending_revert)?;
        out.reverted = revert_out.reverted;
        out.warnings.extend(revert_out.warnings);
        state.pending_revert = AdaptiveRevertPlan::default();
        return Ok(out);
    }

    let (requests, mut warnings) = build_requests(&profile, &decision.actions);
    if requests.is_empty() {
        out.warnings.append(&mut warnings);
        return Ok(out);
    }

    let apply_out = apply(&requests)?;
    out.applied = apply_out.applied;
    out.skipped_noop = apply_out.skipped_noop;
    out.warnings.append(&mut warnings);
    out.warnings.extend(apply_out.warnings);
    state.pending_revert = apply_out.revert_plan;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{daemon_autopilot_tick_with, DaemonAutopilotState};
    use resguard_model::{
        ClassSpec, Metadata, PressureSnapshot, Profile, Spec, SystemMemory, UserMemory,
    };
    use resguard_runtime::adaptive::AdaptiveRevertStep;
    use resguard_runtime::{
        AdaptiveApplyResult, AdaptiveRevertPlan, AdaptiveRevertResult, ClassLimitRequest,
        SystemSnapshot,
    };
    use std::collections::BTreeMap;

    fn profile() -> Profile {
        let mut classes = BTreeMap::new();
        classes.insert(
            "browsers".to_string(),
            ClassSpec {
                slice_name: Some("resguard-browsers.slice".to_string()),
                memory_high: Some("4G".to_string()),
                memory_max: Some("6G".to_string()),
                cpu_weight: Some(80),
                oomd_memory_pressure: None,
                oomd_memory_pressure_limit: None,
            },
        );
        classes.insert(
            "heavy".to_string(),
            ClassSpec {
                slice_name: Some("resguard-heavy.slice".to_string()),
                memory_high: Some("6G".to_string()),
                memory_max: Some("8G".to_string()),
                cpu_weight: Some(90),
                oomd_memory_pressure: None,
                oomd_memory_pressure_limit: None,
            },
        );
        classes.insert(
            "rescue".to_string(),
            ClassSpec {
                slice_name: Some("resguard-rescue.slice".to_string()),
                memory_high: Some("1G".to_string()),
                memory_max: Some("1G".to_string()),
                cpu_weight: Some(100),
                oomd_memory_pressure: None,
                oomd_memory_pressure_limit: None,
            },
        );
        Profile {
            api_version: "resguard.io/v1".to_string(),
            kind: "Profile".to_string(),
            metadata: Metadata {
                name: "auto".to_string(),
            },
            spec: Spec {
                memory: Some(resguard_model::Memory {
                    system: Some(SystemMemory {
                        memory_low: Some("2G".to_string()),
                    }),
                    user: Some(UserMemory {
                        memory_high: Some("10G".to_string()),
                        memory_max: Some("12G".to_string()),
                    }),
                }),
                cpu: None,
                oomd: None,
                classes,
                slices: None,
                suggest: None,
            },
        }
    }

    fn low_snapshot() -> SystemSnapshot {
        SystemSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            ..SystemSnapshot::default()
        }
    }

    fn high_snapshot() -> SystemSnapshot {
        SystemSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 20.0,
                avg60: 35.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 20.0,
                avg60: 40.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 20.0,
                avg60: 30.0,
            }),
            ..SystemSnapshot::default()
        }
    }

    #[test]
    fn once_mode_path_triggers_actions_on_high_pressure() {
        let mut state = DaemonAutopilotState::default();
        let mut seen = Vec::<ClassLimitRequest>::new();
        let out = daemon_autopilot_tick_with(
            || Ok(high_snapshot()),
            || Ok(Some(profile())),
            |reqs| {
                seen = reqs.to_vec();
                Ok(AdaptiveApplyResult {
                    applied: vec!["user:heavy:resguard-heavy.slice".to_string()],
                    skipped_noop: Vec::new(),
                    warnings: Vec::new(),
                    revert_plan: resguard_runtime::AdaptiveRevertPlan::default(),
                })
            },
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("tick");

        assert!(out.had_profile);
        assert!(!out.decision_actions.is_empty());
        assert!(!seen.is_empty());
        assert!(!out.applied.is_empty());
    }

    #[test]
    fn no_action_path_below_threshold() {
        let mut state = DaemonAutopilotState::default();
        let out = daemon_autopilot_tick_with(
            || Ok(low_snapshot()),
            || Ok(Some(profile())),
            |_reqs| Ok(AdaptiveApplyResult::default()),
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("tick");

        assert!(out.had_profile);
        assert!(out.decision_actions.is_empty());
        assert!(out.applied.is_empty());
    }

    #[test]
    fn cooldown_behavior_is_enforced() {
        let mut state = DaemonAutopilotState::default();
        let first = daemon_autopilot_tick_with(
            || Ok(high_snapshot()),
            || Ok(Some(profile())),
            |_reqs| {
                Ok(AdaptiveApplyResult {
                    applied: vec!["x".to_string()],
                    ..AdaptiveApplyResult::default()
                })
            },
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("first");
        assert!(!first.decision_actions.is_empty());

        let second = daemon_autopilot_tick_with(
            || Ok(high_snapshot()),
            || Ok(Some(profile())),
            |_reqs| Ok(AdaptiveApplyResult::default()),
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("second");
        assert!(second.decision_actions.is_empty());
        assert!(second.in_cooldown);
    }

    #[test]
    fn no_profile_is_safe_noop() {
        let mut state = DaemonAutopilotState::default();
        let out = daemon_autopilot_tick_with(
            || Ok(high_snapshot()),
            || Ok(None),
            |_reqs| Ok(AdaptiveApplyResult::default()),
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("tick");
        assert!(!out.had_profile);
        assert!(out.decision_actions.is_empty());
        assert!(out.applied.is_empty());
    }

    #[test]
    fn io_pressure_action_path_is_visible() {
        let mut state = DaemonAutopilotState::default();
        let out = daemon_autopilot_tick_with(
            || {
                Ok(SystemSnapshot {
                    memory_pressure: Some(PressureSnapshot {
                        avg10: 1.0,
                        avg60: 1.0,
                    }),
                    cpu_pressure: Some(PressureSnapshot {
                        avg10: 1.0,
                        avg60: 1.0,
                    }),
                    io_pressure: Some(PressureSnapshot {
                        avg10: 20.0,
                        avg60: 25.0,
                    }),
                    ..SystemSnapshot::default()
                })
            },
            || Ok(Some(profile())),
            |_reqs| {
                Ok(AdaptiveApplyResult {
                    applied: vec!["user:heavy:resguard-heavy.slice".to_string()],
                    ..AdaptiveApplyResult::default()
                })
            },
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("tick");

        assert!(out
            .decision_actions
            .iter()
            .any(|a| a == "reduce-heavy-ioweight"));
    }

    #[test]
    fn revert_visibility_is_explicit() {
        let mut state = DaemonAutopilotState {
            pending_revert: AdaptiveRevertPlan {
                steps: vec![AdaptiveRevertStep {
                    class: "heavy".to_string(),
                    slice: "resguard-heavy.slice".to_string(),
                    user: true,
                    restore_memory_high: None,
                    restore_memory_max: None,
                    restore_cpu_weight: Some(90),
                }],
            },
            policy: resguard_policy::AutopilotState {
                last_action_tick: Some(1),
                phase: resguard_policy::AutopilotPhase::Cooldown,
                ..resguard_policy::AutopilotState::default()
            },
            tick: 4,
        };

        let out = daemon_autopilot_tick_with(
            || Ok(low_snapshot()),
            || Ok(Some(profile())),
            |_reqs| Ok(AdaptiveApplyResult::default()),
            |_plan| {
                Ok(AdaptiveRevertResult {
                    reverted: vec!["user:heavy:resguard-heavy.slice".to_string()],
                    warnings: vec![],
                })
            },
            &mut state,
        )
        .expect("tick");
        assert!(out
            .decision_actions
            .iter()
            .any(|a| a == "revert-adaptive-limits"));
        assert_eq!(out.reverted.len(), 1);
    }

    #[test]
    fn no_op_stability_with_empty_pending_revert_is_safe() {
        let mut state = DaemonAutopilotState {
            policy: resguard_policy::AutopilotState {
                last_action_tick: Some(1),
                phase: resguard_policy::AutopilotPhase::Cooldown,
                ..resguard_policy::AutopilotState::default()
            },
            tick: 4,
            ..DaemonAutopilotState::default()
        };
        let out = daemon_autopilot_tick_with(
            || Ok(low_snapshot()),
            || Ok(Some(profile())),
            |_reqs| Ok(AdaptiveApplyResult::default()),
            |_plan| Ok(AdaptiveRevertResult::default()),
            &mut state,
        )
        .expect("tick");
        assert!(out
            .warnings
            .iter()
            .any(|w| w.contains("no pending adaptive changes")));
        assert!(out.reverted.is_empty());
    }
}
