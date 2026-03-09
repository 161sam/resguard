use anyhow::Result;
use resguard_config::load_profile_from_store;
use resguard_model::{ClassSpec, Profile};
use resguard_runtime::{parse_prop_u64, systemctl_list_units, systemctl_show_props};
use resguard_state::read_state;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TopScopeRow {
    pub unit: String,
    pub memory_current: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TopClassRow {
    pub class: String,
    pub slice: String,
    pub source: String,
    pub configured_memory_high: Option<String>,
    pub configured_memory_max: Option<String>,
    pub configured_cpu_weight: Option<u16>,
    pub memory_current: Option<u64>,
    pub live_memory_high: Option<String>,
    pub live_memory_max: Option<String>,
    pub live_cpu_weight: Option<u16>,
    pub scopes: Vec<TopScopeRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct TopSnapshot {
    pub classes: Vec<TopClassRow>,
    pub partial: bool,
    pub warnings: Vec<String>,
}

fn class_slice_name(class: &str, spec: &ClassSpec) -> String {
    spec.slice_name
        .clone()
        .unwrap_or_else(|| format!("resguard-{class}.slice"))
}

fn first_nonempty(props: &BTreeMap<String, String>, key: &str) -> Option<String> {
    props
        .get(key)
        .cloned()
        .filter(|v| !v.is_empty() && v != "[not set]")
}

fn load_active_profile(config_dir: &str, state_dir: &str) -> Result<Option<Profile>> {
    let state = match read_state(Path::new(state_dir)) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let Some(name) = state.active_profile else {
        return Ok(None);
    };
    Ok(Some(load_profile_from_store(config_dir, &name)?))
}

fn collect_top_snapshot_with<FP, FL, FS>(
    profile_loader: FP,
    max_scopes: usize,
    list_units: FL,
    show_props: FS,
) -> Result<TopSnapshot>
where
    FP: Fn() -> Result<Option<Profile>>,
    FL: Fn(bool, &str) -> Result<Vec<String>>,
    FS: Fn(bool, &str, &[&str]) -> Result<BTreeMap<String, String>>,
{
    let mut out = TopSnapshot::default();
    let Some(profile) = profile_loader()? else {
        out.warnings
            .push("no active profile in state; run apply/setup first".to_string());
        return Ok(out);
    };

    let scope_limit = max_scopes.min(10);
    let mut class_specs = profile.spec.classes.into_iter().collect::<Vec<_>>();
    class_specs.sort_by(|a, b| a.0.cmp(&b.0));

    for (class, spec) in class_specs {
        let slice = class_slice_name(&class, &spec);

        let (source, props) = if let Ok(p) = show_props(
            true,
            &slice,
            &["MemoryCurrent", "MemoryHigh", "MemoryMax", "CPUWeight"],
        ) {
            ("user".to_string(), Some(p))
        } else if let Ok(p) = show_props(
            false,
            &slice,
            &["MemoryCurrent", "MemoryHigh", "MemoryMax", "CPUWeight"],
        ) {
            ("system".to_string(), Some(p))
        } else {
            out.partial = true;
            out.warnings
                .push(format!("slice {slice} is not visible via systemctl show"));
            ("-".to_string(), None)
        };

        let scopes = if source == "-" || scope_limit == 0 {
            Vec::new()
        } else {
            let user_mode = source == "user";
            match list_units(user_mode, "scope") {
                Ok(units) => {
                    let mut rows = Vec::new();
                    for unit in units {
                        let scope_props =
                            match show_props(user_mode, &unit, &["Slice", "MemoryCurrent", "Id"]) {
                                Ok(v) => v,
                                Err(_) => {
                                    out.partial = true;
                                    continue;
                                }
                            };
                        if scope_props.get("Slice").is_some_and(|v| v == &slice) {
                            rows.push(TopScopeRow {
                                unit: scope_props.get("Id").cloned().unwrap_or(unit),
                                memory_current: parse_prop_u64(&scope_props, "MemoryCurrent"),
                            });
                        }
                    }
                    rows.sort_by(|a, b| {
                        b.memory_current
                            .unwrap_or(0)
                            .cmp(&a.memory_current.unwrap_or(0))
                            .then_with(|| a.unit.cmp(&b.unit))
                    });
                    rows.truncate(scope_limit);
                    rows
                }
                Err(_) => {
                    out.partial = true;
                    out.warnings.push(format!(
                        "cannot list active scopes for {} manager",
                        if user_mode { "user" } else { "system" }
                    ));
                    Vec::new()
                }
            }
        };

        out.classes.push(TopClassRow {
            class,
            slice,
            source,
            configured_memory_high: spec.memory_high,
            configured_memory_max: spec.memory_max,
            configured_cpu_weight: spec.cpu_weight,
            memory_current: props
                .as_ref()
                .and_then(|p| parse_prop_u64(p, "MemoryCurrent")),
            live_memory_high: props.as_ref().and_then(|p| first_nonempty(p, "MemoryHigh")),
            live_memory_max: props.as_ref().and_then(|p| first_nonempty(p, "MemoryMax")),
            live_cpu_weight: props
                .as_ref()
                .and_then(|p| parse_prop_u64(p, "CPUWeight"))
                .and_then(|v| u16::try_from(v).ok()),
            scopes,
        });
    }

    Ok(out)
}

pub fn collect_top_snapshot(
    config_dir: &str,
    state_dir: &str,
    max_scopes: usize,
) -> Result<TopSnapshot> {
    collect_top_snapshot_with(
        || load_active_profile(config_dir, state_dir),
        max_scopes,
        systemctl_list_units,
        systemctl_show_props,
    )
}

#[cfg(test)]
mod tests {
    use super::collect_top_snapshot_with;
    use resguard_model::{ClassSpec, Metadata, Profile, Spec};
    use std::collections::BTreeMap;

    fn profile() -> Profile {
        let mut classes = BTreeMap::new();
        classes.insert(
            "browsers".to_string(),
            ClassSpec {
                slice_name: Some("resguard-browsers.slice".to_string()),
                memory_high: Some("4G".to_string()),
                memory_max: Some("6G".to_string()),
                cpu_weight: Some(100),
                ..ClassSpec::default()
            },
        );
        Profile {
            api_version: "v1".to_string(),
            kind: "Profile".to_string(),
            metadata: Metadata {
                name: "demo".to_string(),
            },
            spec: Spec {
                classes,
                ..Spec::default()
            },
        }
    }

    #[test]
    fn includes_live_limits_and_notable_scopes() {
        let out = collect_top_snapshot_with(
            || Ok(Some(profile())),
            2,
            |_user, ty| {
                assert_eq!(ty, "scope");
                Ok(vec![
                    "app-firefox.scope".to_string(),
                    "app-chromium.scope".to_string(),
                ])
            },
            |_user, unit, _keys| {
                let mut p = BTreeMap::new();
                match unit {
                    "resguard-browsers.slice" => {
                        p.insert("MemoryCurrent".to_string(), "1073741824".to_string());
                        p.insert("MemoryHigh".to_string(), "4G".to_string());
                        p.insert("MemoryMax".to_string(), "6G".to_string());
                        p.insert("CPUWeight".to_string(), "100".to_string());
                    }
                    "app-firefox.scope" => {
                        p.insert("Slice".to_string(), "resguard-browsers.slice".to_string());
                        p.insert("MemoryCurrent".to_string(), "900000000".to_string());
                        p.insert("Id".to_string(), "app-firefox.scope".to_string());
                    }
                    "app-chromium.scope" => {
                        p.insert("Slice".to_string(), "resguard-browsers.slice".to_string());
                        p.insert("MemoryCurrent".to_string(), "300000000".to_string());
                        p.insert("Id".to_string(), "app-chromium.scope".to_string());
                    }
                    _ => {}
                }
                Ok(p)
            },
        )
        .expect("snapshot");

        assert!(!out.partial);
        assert_eq!(out.classes.len(), 1);
        let row = &out.classes[0];
        assert_eq!(row.class, "browsers");
        assert_eq!(row.source, "user");
        assert_eq!(row.memory_current, Some(1_073_741_824));
        assert_eq!(row.live_memory_high.as_deref(), Some("4G"));
        assert_eq!(row.live_memory_max.as_deref(), Some("6G"));
        assert_eq!(row.live_cpu_weight, Some(100));
        assert_eq!(row.scopes.len(), 2);
        assert_eq!(row.scopes[0].unit, "app-firefox.scope");
    }

    #[test]
    fn no_profile_returns_empty_snapshot_with_hint() {
        let out = collect_top_snapshot_with(
            || Ok(None),
            3,
            |_user, _ty| Ok(Vec::new()),
            |_user, _unit, _keys| Ok(BTreeMap::new()),
        )
        .expect("snapshot");

        assert!(out.classes.is_empty());
        assert!(!out.partial);
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("no active profile"));
    }

    #[test]
    fn missing_slice_marks_partial() {
        let out = collect_top_snapshot_with(
            || Ok(Some(profile())),
            3,
            |_user, _ty| Ok(Vec::new()),
            |_user, _unit, _keys| Err(anyhow::anyhow!("not found")),
        )
        .expect("snapshot");

        assert!(out.partial);
        assert_eq!(out.classes.len(), 1);
        assert_eq!(out.classes[0].source, "-");
        assert!(out.warnings.iter().any(|w| w.contains("not visible")));
    }
}
