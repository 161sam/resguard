use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_check_interval_secs")]
    pub check_interval_secs: u64,
    #[serde(default = "default_memory_pressure_avg60_threshold")]
    pub memory_pressure_avg60_threshold: f64,
    #[serde(default)]
    pub actions: Vec<DaemonAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DaemonAction {
    SetProperty {
        #[serde(default = "default_unit")]
        unit: String,
        #[serde(default = "default_memory_high_percent")]
        memory_high_percent: u8,
        #[serde(default = "default_memory_max_percent")]
        memory_max_percent: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonValidationError {
    pub path: String,
    pub message: String,
}

impl DaemonValidationError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

pub fn default_daemon_config() -> DaemonConfig {
    DaemonConfig {
        enabled: default_enabled(),
        check_interval_secs: default_check_interval_secs(),
        memory_pressure_avg60_threshold: default_memory_pressure_avg60_threshold(),
        actions: vec![DaemonAction::SetProperty {
            unit: default_unit(),
            memory_high_percent: default_memory_high_percent(),
            memory_max_percent: default_memory_max_percent(),
        }],
    }
}

pub fn validate_daemon_config(cfg: &DaemonConfig) -> Vec<DaemonValidationError> {
    let mut errors = Vec::new();

    if cfg.check_interval_secs == 0 {
        errors.push(DaemonValidationError::new(
            "check_interval_secs",
            "must be > 0",
        ));
    }

    if !cfg.memory_pressure_avg60_threshold.is_finite()
        || cfg.memory_pressure_avg60_threshold <= 0.0
        || cfg.memory_pressure_avg60_threshold > 100.0
    {
        errors.push(DaemonValidationError::new(
            "memory_pressure_avg60_threshold",
            "must be finite and in range (0, 100]",
        ));
    }

    if cfg.actions.is_empty() {
        errors.push(DaemonValidationError::new(
            "actions",
            "must contain at least one action",
        ));
    }

    for (idx, action) in cfg.actions.iter().enumerate() {
        match action {
            DaemonAction::SetProperty {
                unit,
                memory_high_percent,
                memory_max_percent,
            } => {
                let base = format!("actions[{idx}]");
                if unit != "user.slice" {
                    errors.push(DaemonValidationError::new(
                        format!("{base}.unit"),
                        "only user.slice is allowed",
                    ));
                }
                if *memory_high_percent == 0 || *memory_high_percent > 100 {
                    errors.push(DaemonValidationError::new(
                        format!("{base}.memory_high_percent"),
                        "must be in range 1..=100",
                    ));
                }
                if *memory_max_percent == 0 || *memory_max_percent > 100 {
                    errors.push(DaemonValidationError::new(
                        format!("{base}.memory_max_percent"),
                        "must be in range 1..=100",
                    ));
                }
                if *memory_max_percent < *memory_high_percent {
                    errors.push(DaemonValidationError::new(
                        base,
                        "memory_max_percent must be >= memory_high_percent",
                    ));
                }
            }
        }
    }

    errors
}

const fn default_enabled() -> bool {
    false
}

const fn default_check_interval_secs() -> u64 {
    30
}

const fn default_memory_pressure_avg60_threshold() -> f64 {
    20.0
}

fn default_unit() -> String {
    "user.slice".to_string()
}

const fn default_memory_high_percent() -> u8 {
    50
}

const fn default_memory_max_percent() -> u8 {
    60
}

#[cfg(test)]
mod tests {
    use super::{
        default_daemon_config, validate_daemon_config, DaemonAction, DaemonConfig,
        DaemonValidationError,
    };

    fn has_error(errors: &[DaemonValidationError], path: &str) -> bool {
        errors.iter().any(|e| e.path == path)
    }

    #[test]
    fn defaults_are_safe_and_valid() {
        let cfg = default_daemon_config();
        let errors = validate_daemon_config(&cfg);
        assert!(errors.is_empty());
        assert!(!cfg.enabled);
    }

    #[test]
    fn invalid_threshold_and_interval_are_rejected() {
        let mut cfg = default_daemon_config();
        cfg.check_interval_secs = 0;
        cfg.memory_pressure_avg60_threshold = 0.0;
        let errors = validate_daemon_config(&cfg);
        assert!(has_error(&errors, "check_interval_secs"));
        assert!(has_error(&errors, "memory_pressure_avg60_threshold"));
    }

    #[test]
    fn action_validation_rejects_unsafe_units_and_ranges() {
        let cfg = DaemonConfig {
            enabled: true,
            check_interval_secs: 10,
            memory_pressure_avg60_threshold: 10.0,
            actions: vec![DaemonAction::SetProperty {
                unit: "system.slice".to_string(),
                memory_high_percent: 70,
                memory_max_percent: 60,
            }],
        };
        let errors = validate_daemon_config(&cfg);
        assert!(has_error(&errors, "actions[0].unit"));
        assert!(has_error(&errors, "actions[0]"));
    }
}
