use resguard_model::{MetricsSnapshot, PressureSnapshot, Profile};

use crate::thresholds::{
    autopilot_cooldown_ticks, cpu_pressure_high_threshold, memory_pressure_high_threshold,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AutopilotState {
    pub last_action_tick: Option<u64>,
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
    PreserveRescueClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutopilotDecision {
    pub actions: Vec<AutopilotAction>,
    pub next_state: AutopilotState,
    pub in_cooldown: bool,
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
    if let Some(last) = state.last_action_tick {
        if now_tick.saturating_sub(last) < cooldown {
            return AutopilotDecision {
                actions: Vec::new(),
                next_state: *state,
                in_cooldown: true,
            };
        }
    }

    let mem_high = pressure_avg60(metrics.memory_pressure) >= memory_pressure_high_threshold();
    let cpu_high = pressure_avg60(metrics.cpu_pressure) >= cpu_pressure_high_threshold();

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

    if !actions.is_empty() && profile_has_class(profile, "rescue") {
        actions.push(AutopilotAction::PreserveRescueClass);
    }

    let next_state = if actions.is_empty() {
        *state
    } else {
        AutopilotState {
            last_action_tick: Some(now_tick),
        }
    };

    AutopilotDecision {
        actions,
        next_state,
        in_cooldown: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{decide_autopilot_actions, AutopilotAction, AutopilotState};
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
            ..MetricsSnapshot::default()
        };
        let decision = decide_autopilot_actions(&metrics, &AutopilotState::default(), &profile, 12);
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
            ..MetricsSnapshot::default()
        };
        let state = AutopilotState {
            last_action_tick: Some(100),
        };

        let decision = decide_autopilot_actions(&metrics, &state, &profile, 101);
        assert!(decision.actions.is_empty());
        assert!(decision.in_cooldown);
        assert_eq!(decision.next_state.last_action_tick, Some(100));
    }
}
