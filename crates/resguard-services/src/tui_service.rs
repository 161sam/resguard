use anyhow::Result;
use resguard_config::load_profile_from_store;
use resguard_model::{ClassSpec, PressureSnapshot, Profile};
use resguard_runtime::{parse_prop_u64, read_system_snapshot, systemctl_show_props};
use resguard_state::read_state;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TuiClassSlice {
    pub class: String,
    pub slice: String,
    pub source: String,
    pub configured_memory_high: Option<String>,
    pub configured_memory_max: Option<String>,
    pub configured_cpu_weight: Option<u16>,
    pub live_memory_high: Option<String>,
    pub live_memory_max: Option<String>,
    pub live_cpu_weight: Option<u16>,
    pub memory_current: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TuiLedgerAction {
    pub timestamp: Option<u64>,
    pub tick: Option<u64>,
    pub decision: String,
    pub actions: Vec<String>,
    pub applied: Vec<String>,
    pub reverted: Vec<String>,
    pub warnings: Vec<String>,
    pub in_cooldown: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TuiSnapshot {
    pub cpu_pressure: Option<PressureSnapshot>,
    pub memory_pressure: Option<PressureSnapshot>,
    pub io_pressure: Option<PressureSnapshot>,
    pub mem_total_bytes: Option<u64>,
    pub mem_available_bytes: Option<u64>,
    pub classes: Vec<TuiClassSlice>,
    pub recent_actions: Vec<TuiLedgerAction>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct LedgerRecordWire {
    timestamp: Option<u64>,
    tick: Option<u64>,
    decision: Option<String>,
    #[serde(alias = "decision_actions")]
    decision_actions: Vec<String>,
    applied: Vec<String>,
    reverted: Vec<String>,
    warnings: Vec<String>,
    #[serde(alias = "in_cooldown")]
    in_cooldown: Option<bool>,
    action: Option<String>,
}

fn class_slice_name(class: &str, spec: &ClassSpec) -> String {
    spec.slice_name
        .clone()
        .unwrap_or_else(|| format!("resguard-{class}.slice"))
}

fn load_active_profile(config_dir: &str, state_dir: &str) -> Result<Option<Profile>> {
    let state_root = Path::new(state_dir);
    let state = match read_state(state_root) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let Some(name) = state.active_profile else {
        return Ok(None);
    };
    Ok(Some(load_profile_from_store(config_dir, &name)?))
}

fn first_nonempty(props: &BTreeMap<String, String>, key: &str) -> Option<String> {
    props
        .get(key)
        .cloned()
        .filter(|v| !v.is_empty() && v != "[not set]")
}

fn read_slice_props(user: bool, slice: &str) -> Option<BTreeMap<String, String>> {
    systemctl_show_props(
        user,
        slice,
        &["MemoryCurrent", "MemoryHigh", "MemoryMax", "CPUWeight"],
    )
    .ok()
}

fn build_class_rows(profile: Option<Profile>) -> Vec<TuiClassSlice> {
    let Some(profile) = profile else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (class, spec) in &profile.spec.classes {
        let slice = class_slice_name(class, spec);

        let (source, props) = if let Some(p) = read_slice_props(true, &slice) {
            ("user".to_string(), Some(p))
        } else if let Some(p) = read_slice_props(false, &slice) {
            ("system".to_string(), Some(p))
        } else {
            ("-".to_string(), None)
        };

        out.push(TuiClassSlice {
            class: class.clone(),
            slice,
            source,
            configured_memory_high: spec.memory_high.clone(),
            configured_memory_max: spec.memory_max.clone(),
            configured_cpu_weight: spec.cpu_weight,
            live_memory_high: props.as_ref().and_then(|p| first_nonempty(p, "MemoryHigh")),
            live_memory_max: props.as_ref().and_then(|p| first_nonempty(p, "MemoryMax")),
            live_cpu_weight: props
                .as_ref()
                .and_then(|p| parse_prop_u64(p, "CPUWeight"))
                .and_then(|v| u16::try_from(v).ok()),
            memory_current: props
                .as_ref()
                .and_then(|p| parse_prop_u64(p, "MemoryCurrent")),
        });
    }

    out.sort_by(|a, b| a.class.cmp(&b.class));
    out
}

fn parse_ledger_line(line: &str) -> Option<TuiLedgerAction> {
    let wire: LedgerRecordWire = serde_json::from_str(line).ok()?;

    let decision = wire.decision.unwrap_or_else(|| {
        if wire.action.is_some() {
            "trigger".to_string()
        } else {
            "idle".to_string()
        }
    });

    let mut actions = wire.decision_actions;
    if actions.is_empty() {
        if let Some(action) = wire.action {
            actions.push(action);
        }
    }

    Some(TuiLedgerAction {
        timestamp: wire.timestamp,
        tick: wire.tick,
        decision,
        actions,
        applied: wire.applied,
        reverted: wire.reverted,
        warnings: wire.warnings,
        in_cooldown: wire.in_cooldown.unwrap_or(false),
    })
}

fn read_recent_actions(state_dir: &str, limit: usize) -> Vec<TuiLedgerAction> {
    let ledger = Path::new(state_dir).join("daemon-ledger.jsonl");
    let content = match fs::read_to_string(ledger) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .rev()
        .filter_map(parse_ledger_line)
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

pub fn collect_tui_snapshot(config_dir: &str, state_dir: &str) -> TuiSnapshot {
    let sys = read_system_snapshot();
    let profile = load_active_profile(config_dir, state_dir).ok().flatten();

    TuiSnapshot {
        cpu_pressure: sys.cpu_pressure,
        memory_pressure: sys.memory_pressure,
        io_pressure: sys.io_pressure,
        mem_total_bytes: sys.mem_total_bytes,
        mem_available_bytes: sys.mem_available_bytes,
        classes: build_class_rows(profile),
        recent_actions: read_recent_actions(state_dir, 5),
    }
}

#[cfg(test)]
mod tests {
    use super::{class_slice_name, parse_ledger_line};
    use resguard_model::ClassSpec;

    #[test]
    fn class_slice_name_defaults_to_resguard_class() {
        let spec = ClassSpec::default();
        assert_eq!(
            class_slice_name("browsers", &spec),
            "resguard-browsers.slice"
        );
    }

    #[test]
    fn class_slice_name_uses_override_when_present() {
        let spec = ClassSpec {
            slice_name: Some("custom.slice".to_string()),
            ..ClassSpec::default()
        };
        assert_eq!(class_slice_name("browsers", &spec), "custom.slice");
    }

    #[test]
    fn parse_ledger_line_supports_new_daemon_format() {
        let line = r#"{"timestamp":1,"tick":7,"decision":"trigger","decisionActions":["reduce-heavy-cpuweight"],"applied":["user:heavy:resguard-heavy.slice"],"reverted":[],"warnings":[],"inCooldown":false}"#;
        let row = parse_ledger_line(line).expect("row");
        assert_eq!(row.tick, Some(7));
        assert_eq!(row.decision, "trigger");
        assert_eq!(row.actions, vec!["reduce-heavy-cpuweight".to_string()]);
        assert_eq!(row.applied.len(), 1);
        assert_eq!(row.reverted.len(), 0);
    }

    #[test]
    fn parse_ledger_line_supports_legacy_action_field() {
        let line = r#"{"timestamp":1,"decision":"trigger","action":"panic"}"#;
        let row = parse_ledger_line(line).expect("row");
        assert_eq!(row.decision, "trigger");
        assert_eq!(row.actions, vec!["panic".to_string()]);
    }

    #[test]
    fn parse_ledger_line_rejects_invalid_json() {
        assert!(parse_ledger_line("nope").is_none());
    }
}
