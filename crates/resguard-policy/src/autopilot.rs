use resguard_model::{MetricsSnapshot, PressureSnapshot, Profile};

use crate::thresholds::{
    autopilot_cooldown_ticks, autopilot_revert_window_ticks, cpu_pressure_high_threshold,
    io_pressure_high_threshold, memory_pressure_high_threshold,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AutopilotPhase {
    #[default]
    Idle,
    Cooldown,
    RevertWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AutopilotState {
    pub last_action_tick: Option<u64>,
    pub last_revert_tick: Option<u64>,
    pub phase: AutopilotPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutopilotAction {
    ReduceBrowsersLimits {
        memory_high_percent: u8,
        memory_max_percent: u8,
    },
    ReduceHeavyCpuWeight {
        cpu_weight: u16,
    },
    ReduceHeavyIoWeight {
        io_weight: u16,
    },
    RevertAdaptiveLimits,
    PreserveRescueClass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutopilotTransition {
    StayIdle,
    TriggerToCooldown,
    StayCooldown,
    CooldownToRevertWindow,
    RevertWindowToIdle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutopilotDecision {
    pub actions: Vec<AutopilotAction>,
    pub next_state: AutopilotState,
    pub in_cooldown: bool,
    pub transition: AutopilotTransition,
}

fn pressure_avg60(snapshot: Option<PressureSnapshot>) -> f64 {
    snapshot.map(|p| p.avg60).unwrap_or(0.0)
}

fn profile_has_class(profile: &Profile, class: &str) -> bool {
    profile.spec.classes.contains_key(class)
}

pub fn decide_autopilot_actions(
    metrics: &MetricsSnapshot,
    state: &AutopilotState,
    profile: &Profile,
    now_tick: u64,
) -> AutopilotDecision {
    let cooldown = autopilot_cooldown_ticks();
    let revert_window = autopilot_revert_window_ticks();

    let mem_high = pressure_avg60(metrics.memory_pressure) >= memory_pressure_high_threshold();
    let cpu_high = pressure_avg60(metrics.cpu_pressure) >= cpu_pressure_high_threshold();
    let io_high = pressure_avg60(metrics.io_pressure) >= io_pressure_high_threshold();
    let any_pressure_high = mem_high || cpu_high || io_high;

    if state.phase == AutopilotPhase::RevertWindow {
        if let Some(last_revert) = state.last_revert_tick {
            if now_tick.saturating_sub(last_revert) < revert_window {
                return AutopilotDecision {
                    actions: Vec::new(),
                    next_state: *state,
                    in_cooldown: true,
                    transition: AutopilotTransition::CooldownToRevertWindow,
                };
            }
        }
        return AutopilotDecision {
            actions: Vec::new(),
            next_state: AutopilotState {
                phase: AutopilotPhase::Idle,
                ..*state
            },
            in_cooldown: false,
            transition: AutopilotTransition::RevertWindowToIdle,
        };
    }

    if state.phase == AutopilotPhase::Cooldown {
        if let Some(last) = state.last_action_tick {
            if now_tick.saturating_sub(last) < cooldown {
                return AutopilotDecision {
                    actions: Vec::new(),
                    next_state: *state,
                    in_cooldown: true,
                    transition: AutopilotTransition::StayCooldown,
                };
            }
        }

        if !any_pressure_high {
            return AutopilotDecision {
                actions: vec![AutopilotAction::RevertAdaptiveLimits],
                next_state: AutopilotState {
                    last_revert_tick: Some(now_tick),
                    phase: AutopilotPhase::RevertWindow,
                    ..*state
                },
                in_cooldown: false,
                transition: AutopilotTransition::CooldownToRevertWindow,
            };
        }
    }

    let mut actions = Vec::new();
    if mem_high && profile_has_class(profile, "browsers") {
        actions.push(AutopilotAction::ReduceBrowsersLimits {
            memory_high_percent: 85,
            memory_max_percent: 90,
        });
    }
    if cpu_high && profile_has_class(profile, "heavy") {
        actions.push(AutopilotAction::ReduceHeavyCpuWeight { cpu_weight: 70 });
    }
    if io_high && profile_has_class(profile, "heavy") {
        actions.push(AutopilotAction::ReduceHeavyIoWeight { io_weight: 60 });
    }

    if !actions.is_empty() && profile_has_class(profile, "rescue") {
        actions.push(AutopilotAction::PreserveRescueClass);
    }

    let next_state = if actions.is_empty() {
        AutopilotState {
            phase: AutopilotPhase::Idle,
            ..*state
        }
    } else {
        AutopilotState {
            last_action_tick: Some(now_tick),
            phase: AutopilotPhase::Cooldown,
            ..*state
        }
    };

    AutopilotDecision {
        actions,
        next_state,
        in_cooldown: false,
        transition: if next_state.phase == AutopilotPhase::Cooldown {
            AutopilotTransition::TriggerToCooldown
        } else {
            AutopilotTransition::StayIdle
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decide_autopilot_actions, AutopilotAction, AutopilotPhase, AutopilotState,
        AutopilotTransition,
    };
    use resguard_model::{
        ClassSpec, Cpu, Memory, Metadata, MetricsSnapshot, Oomd, PressureSnapshot, Profile, Spec,
    };
    use std::collections::BTreeMap;

    fn profile_with_classes(classes: &[&str]) -> Profile {
        let mut map = BTreeMap::new();
        for class in classes {
            map.insert((*class).to_string(), ClassSpec::default());
        }
        Profile {
            api_version: "resguard.io/v1".to_string(),
            kind: "Profile".to_string(),
            metadata: Metadata {
                name: "auto".to_string(),
            },
            spec: Spec {
                memory: Some(Memory::default()),
                cpu: Some(Cpu::default()),
                oomd: Some(Oomd::default()),
                classes: map,
                slices: None,
                suggest: None,
            },
        }
    }

    #[test]
    fn high_memory_pressure_reduces_browsers_limits() {
        let profile = profile_with_classes(&["browsers", "rescue"]);
        let metrics = MetricsSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 10.0,
                avg60: 25.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            ..MetricsSnapshot::default()
        };
        let decision = decide_autopilot_actions(&metrics, &AutopilotState::default(), &profile, 10);
        assert!(decision
            .actions
            .iter()
            .any(|a| matches!(a, AutopilotAction::ReduceBrowsersLimits { .. })));
        assert!(decision
            .actions
            .iter()
            .any(|a| matches!(a, AutopilotAction::PreserveRescueClass)));
        assert_eq!(decision.next_state.last_action_tick, Some(10));
        assert!(!decision.in_cooldown);
        assert_eq!(decision.transition, AutopilotTransition::TriggerToCooldown);
    }

    #[test]
    fn high_cpu_pressure_reduces_heavy_cpu_weight() {
        let profile = profile_with_classes(&["heavy", "rescue"]);
        let metrics = MetricsSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 15.0,
                avg60: 40.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            ..MetricsSnapshot::default()
        };
        let decision = decide_autopilot_actions(&metrics, &AutopilotState::default(), &profile, 11);
        assert!(decision
            .actions
            .iter()
            .any(|a| matches!(a, AutopilotAction::ReduceHeavyCpuWeight { cpu_weight: 70 })));
        assert!(decision
            .actions
            .iter()
            .any(|a| matches!(a, AutopilotAction::PreserveRescueClass)));
    }

    #[test]
    fn high_io_pressure_reduces_heavy_io_weight() {
        let profile = profile_with_classes(&["heavy", "rescue"]);
        let metrics = MetricsSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 1.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 10.0,
                avg60: 25.0,
            }),
            ..MetricsSnapshot::default()
        };
        let decision = decide_autopilot_actions(&metrics, &AutopilotState::default(), &profile, 12);
        assert!(decision
            .actions
            .iter()
            .any(|a| matches!(a, AutopilotAction::ReduceHeavyIoWeight { io_weight: 60 })));
    }

    #[test]
    fn no_action_below_threshold() {
        let profile = profile_with_classes(&["browsers", "heavy", "rescue"]);
        let metrics = MetricsSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 5.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 2.0,
                avg60: 10.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 2.0,
                avg60: 5.0,
            }),
            ..MetricsSnapshot::default()
        };
        let decision = decide_autopilot_actions(&metrics, &AutopilotState::default(), &profile, 13);
        assert!(decision.actions.is_empty());
        assert_eq!(decision.next_state.last_action_tick, None);
    }

    #[test]
    fn cooldown_prevents_thrashing() {
        let profile = profile_with_classes(&["browsers", "heavy", "rescue"]);
        let metrics = MetricsSnapshot {
            memory_pressure: Some(PressureSnapshot {
                avg10: 10.0,
                avg60: 30.0,
            }),
            cpu_pressure: Some(PressureSnapshot {
                avg10: 10.0,
                avg60: 35.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 10.0,
                avg60: 25.0,
            }),
            ..MetricsSnapshot::default()
        };
        let state = AutopilotState {
            last_action_tick: Some(100),
            phase: AutopilotPhase::Cooldown,
            ..AutopilotState::default()
        };

        let decision = decide_autopilot_actions(&metrics, &state, &profile, 101);
        assert!(decision.actions.is_empty());
        assert!(decision.in_cooldown);
        assert_eq!(decision.next_state.last_action_tick, Some(100));
        assert_eq!(decision.transition, AutopilotTransition::StayCooldown);
    }

    #[test]
    fn cooldown_elapsed_and_pressure_low_triggers_revert() {
        let profile = profile_with_classes(&["browsers", "heavy", "rescue"]);
        let metrics = MetricsSnapshot {
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
            ..MetricsSnapshot::default()
        };
        let state = AutopilotState {
            last_action_tick: Some(1),
            phase: AutopilotPhase::Cooldown,
            ..AutopilotState::default()
        };
        let decision = decide_autopilot_actions(&metrics, &state, &profile, 5);
        assert!(decision
            .actions
            .iter()
            .any(|a| matches!(a, AutopilotAction::RevertAdaptiveLimits)));
        assert_eq!(decision.next_state.phase, AutopilotPhase::RevertWindow);
        assert_eq!(
            decision.transition,
            AutopilotTransition::CooldownToRevertWindow
        );
    }
}
