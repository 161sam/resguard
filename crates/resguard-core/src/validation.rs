use crate::profile::Profile;
use regex::Regex;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl ValidationError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

pub fn parse_size_to_bytes(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty size".into());
    }

    let split_idx = s
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_alphabetic())
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());

    let (n_str, u_str) = s.split_at(split_idx);
    let n: u64 = n_str
        .trim()
        .parse()
        .map_err(|_| format!("invalid number: {}", n_str.trim()))?;

    let mult: u64 = match u_str.trim().to_ascii_uppercase().as_str() {
        "" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024_u64.pow(2),
        "G" | "GB" => 1024_u64.pow(3),
        "T" | "TB" => 1024_u64.pow(4),
        _ => return Err(format!("invalid unit: {}", u_str.trim())),
    };

    n.checked_mul(mult).ok_or_else(|| "size overflow".into())
}

pub fn parse_cpuset(s: &str) -> Result<Vec<u32>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty cpuset".into());
    }

    let mut values = BTreeSet::new();
    for raw_part in s.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err("empty cpuset segment".into());
        }

        if let Some((start_s, end_s)) = part.split_once('-') {
            let start: u32 = start_s
                .trim()
                .parse()
                .map_err(|_| format!("invalid cpu index: {}", start_s.trim()))?;
            let end: u32 = end_s
                .trim()
                .parse()
                .map_err(|_| format!("invalid cpu index: {}", end_s.trim()))?;
            if end < start {
                return Err(format!("invalid cpu range: {}", part));
            }
            for n in start..=end {
                values.insert(n);
            }
        } else {
            let idx: u32 = part
                .parse()
                .map_err(|_| format!("invalid cpu index: {}", part))?;
            values.insert(idx);
        }
    }

    Ok(values.into_iter().collect())
}

pub fn validate_memory(high: Option<&str>, max: Option<&str>) -> Result<(), String> {
    if let (Some(h), Some(m)) = (high, max) {
        let hb = parse_size_to_bytes(h)?;
        let mb = parse_size_to_bytes(m)?;
        if mb < hb {
            return Err(format!("MemoryMax ({m}) must be >= MemoryHigh ({h})"));
        }
    }
    Ok(())
}

pub fn validate_profile(profile: &Profile) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if profile.api_version != "resguard.io/v1" {
        errors.push(ValidationError::new(
            "apiVersion",
            format!(
                "unsupported apiVersion: {} (expected resguard.io/v1)",
                profile.api_version
            ),
        ));
    }

    if profile.kind != "Profile" {
        errors.push(ValidationError::new(
            "kind",
            format!("unsupported kind: {} (expected Profile)", profile.kind),
        ));
    }

    if profile.metadata.name.trim().is_empty() {
        errors.push(ValidationError::new("metadata.name", "must not be empty"));
    }

    if let Some(memory) = &profile.spec.memory {
        let system_low = memory
            .system
            .as_ref()
            .and_then(|system| system.memory_low.as_deref());
        if let Some(low) = system_low {
            if let Err(err) = parse_size_to_bytes(low) {
                errors.push(ValidationError::new(
                    "spec.memory.system.memoryLow",
                    format!("invalid size: {err}"),
                ));
            }
        }

        let user_high = memory
            .user
            .as_ref()
            .and_then(|user| user.memory_high.as_deref());
        let user_max = memory
            .user
            .as_ref()
            .and_then(|user| user.memory_max.as_deref());

        if let Some(high) = user_high {
            if let Err(err) = parse_size_to_bytes(high) {
                errors.push(ValidationError::new(
                    "spec.memory.user.memoryHigh",
                    format!("invalid size: {err}"),
                ));
            }
        }

        if let Some(max) = user_max {
            if let Err(err) = parse_size_to_bytes(max) {
                errors.push(ValidationError::new(
                    "spec.memory.user.memoryMax",
                    format!("invalid size: {err}"),
                ));
            }
        }

        if let Err(err) = validate_memory(user_high, user_max) {
            errors.push(ValidationError::new("spec.memory.user", err));
        }
    }

    if let Some(cpu) = &profile.spec.cpu {
        if let Some(system_allowed) = cpu.system_allowed_cpus.as_deref() {
            if let Err(err) = parse_cpuset(system_allowed) {
                errors.push(ValidationError::new("spec.cpu.systemAllowedCpus", err));
            }
        }

        if let Some(user_allowed) = cpu.user_allowed_cpus.as_deref() {
            if let Err(err) = parse_cpuset(user_allowed) {
                errors.push(ValidationError::new("spec.cpu.userAllowedCpus", err));
            }
        }
    }

    if let Some(suggest) = &profile.spec.suggest {
        for (idx, rule) in suggest.rules.iter().enumerate() {
            let base = format!("spec.suggest.rules[{idx}]");
            if rule.pattern.trim().is_empty() {
                errors.push(ValidationError::new(
                    format!("{base}.pattern"),
                    "must not be empty",
                ));
            } else if let Err(err) = Regex::new(&rule.pattern) {
                errors.push(ValidationError::new(
                    format!("{base}.pattern"),
                    format!("invalid regex: {err}"),
                ));
            }

            if rule.class.trim().is_empty() {
                errors.push(ValidationError::new(
                    format!("{base}.class"),
                    "must not be empty",
                ));
            }
        }
    }

    validate_classes(&profile.spec.classes, "spec.classes", &mut errors);
    if let Some(slices) = &profile.spec.slices {
        validate_classes(&slices.classes, "spec.slices.classes", &mut errors);
    }

    errors
}

fn validate_classes(
    classes: &std::collections::BTreeMap<String, crate::profile::Class>,
    root: &str,
    errors: &mut Vec<ValidationError>,
) {
    for (class_name, class) in classes {
        if class_name.trim().is_empty() {
            errors.push(ValidationError::new(root, "class name must not be empty"));
        }

        let base = format!("{root}.{class_name}");

        if let Some(slice_name) = &class.slice_name {
            if !slice_name.ends_with(".slice") {
                errors.push(ValidationError::new(
                    format!("{base}.sliceName"),
                    "sliceName must end with .slice",
                ));
            }
            if slice_name.contains('/') || slice_name.contains("..") {
                errors.push(ValidationError::new(
                    format!("{base}.sliceName"),
                    "sliceName must not contain path components",
                ));
            }
        }

        if let Some(memory_high) = class.memory_high.as_deref() {
            if let Err(err) = parse_size_to_bytes(memory_high) {
                errors.push(ValidationError::new(
                    format!("{base}.memoryHigh"),
                    format!("invalid size: {err}"),
                ));
            }
        }

        if let Some(memory_max) = class.memory_max.as_deref() {
            if let Err(err) = parse_size_to_bytes(memory_max) {
                errors.push(ValidationError::new(
                    format!("{base}.memoryMax"),
                    format!("invalid size: {err}"),
                ));
            }
        }

        if let Err(err) = validate_memory(class.memory_high.as_deref(), class.memory_max.as_deref())
        {
            errors.push(ValidationError::new(base, err));
        }

        if let Some(weight) = class.cpu_weight {
            if weight == 0 || weight > 10_000 {
                errors.push(ValidationError::new(
                    format!("{root}.{class_name}.cpuWeight"),
                    "cpuWeight must be in range 1..=10000",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::profile::{Metadata, Profile, Spec, Suggest, SuggestRule};

    use super::{parse_cpuset, parse_size_to_bytes, validate_memory};

    #[test]
    fn parse_size_bytes_plain_number() {
        assert_eq!(parse_size_to_bytes("42").unwrap(), 42);
    }

    #[test]
    fn parse_size_bytes_with_units() {
        assert_eq!(parse_size_to_bytes("1K").unwrap(), 1024);
        assert_eq!(parse_size_to_bytes("2m").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size_to_bytes("3GB").unwrap(), 3 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_rejects_invalid_values() {
        assert!(parse_size_to_bytes(" ").is_err());
        assert!(parse_size_to_bytes("xG").is_err());
        assert!(parse_size_to_bytes("1P").is_err());
    }

    #[test]
    fn parse_cpuset_expands_ranges() {
        assert_eq!(parse_cpuset("1-3,5").unwrap(), vec![1, 2, 3, 5]);
    }

    #[test]
    fn parse_cpuset_rejects_invalid_values() {
        assert!(parse_cpuset("").is_err());
        assert!(parse_cpuset("3-1").is_err());
        assert!(parse_cpuset("a").is_err());
    }

    #[test]
    fn validate_memory_requires_max_ge_high() {
        assert!(validate_memory(Some("12G"), Some("14G")).is_ok());
        assert!(validate_memory(Some("12G"), Some("10G")).is_err());
    }

    #[test]
    fn validate_profile_rejects_invalid_suggest_rules() {
        let profile = Profile {
            api_version: "resguard.io/v1".to_string(),
            kind: "Profile".to_string(),
            metadata: Metadata {
                name: "demo".to_string(),
            },
            spec: Spec {
                suggest: Some(Suggest {
                    rules: vec![SuggestRule {
                        pattern: "(".to_string(),
                        class: "".to_string(),
                    }],
                }),
                ..Spec::default()
            },
        };
        let errors = super::validate_profile(&profile);
        assert!(
            errors
                .iter()
                .any(|e| e.path == "spec.suggest.rules[0].pattern")
        );
        assert!(
            errors
                .iter()
                .any(|e| e.path == "spec.suggest.rules[0].class")
        );
    }
}
