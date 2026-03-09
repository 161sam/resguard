use anyhow::Result;
use resguard_runtime::{parse_prop_u64, systemctl_list_units, systemctl_show_props};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TreeRoot {
    pub name: String,
    pub source: String,
    pub memory_current: Option<u64>,
    pub memory_high: Option<String>,
    pub memory_max: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TreeScope {
    pub unit: String,
    pub memory_current: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TreeClassSlice {
    pub class: String,
    pub slice: String,
    pub source: String,
    pub memory_current: Option<u64>,
    pub scopes: Vec<TreeScope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TreeSnapshot {
    pub roots: Vec<TreeRoot>,
    pub classes: Vec<TreeClassSlice>,
    pub partial: bool,
    pub warnings: Vec<String>,
}

fn nonempty(props: &BTreeMap<String, String>, key: &str) -> Option<String> {
    props
        .get(key)
        .cloned()
        .filter(|v| !v.is_empty() && v != "[not set]")
}

fn class_name_from_slice(slice: &str) -> String {
    slice
        .strip_prefix("resguard-")
        .and_then(|v| v.strip_suffix(".slice"))
        .unwrap_or(slice)
        .to_string()
}

fn collect_tree_snapshot_with<FL, FS>(
    max_scopes: usize,
    list_units: FL,
    show_props: FS,
) -> TreeSnapshot
where
    FL: Fn(bool, &str) -> Result<Vec<String>>,
    FS: Fn(bool, &str, &[&str]) -> Result<BTreeMap<String, String>>,
{
    let mut out = TreeSnapshot::default();
    let scope_limit = max_scopes.min(10);

    for (root, user) in [("system.slice", false), ("user.slice", true)] {
        match show_props(user, root, &["MemoryCurrent", "MemoryHigh", "MemoryMax"]) {
            Ok(props) => out.roots.push(TreeRoot {
                name: root.to_string(),
                source: if user { "user".into() } else { "system".into() },
                memory_current: parse_prop_u64(&props, "MemoryCurrent"),
                memory_high: nonempty(&props, "MemoryHigh"),
                memory_max: nonempty(&props, "MemoryMax"),
            }),
            Err(_) => {
                out.partial = true;
                out.warnings.push(format!("cannot read {root} properties"));
            }
        }
    }

    let mut seen = BTreeMap::<(String, String), TreeClassSlice>::new();
    for user in [true, false] {
        let manager = if user { "user" } else { "system" };
        let slices = match list_units(user, "slice") {
            Ok(v) => v,
            Err(_) => {
                out.partial = true;
                out.warnings
                    .push(format!("cannot list slice units for {manager} manager"));
                continue;
            }
        };
        let scopes = list_units(user, "scope").unwrap_or_default();

        for slice in slices
            .into_iter()
            .filter(|u| u.starts_with("resguard-") && u.ends_with(".slice"))
        {
            let props = match show_props(user, &slice, &["MemoryCurrent"]) {
                Ok(v) => v,
                Err(_) => {
                    out.partial = true;
                    continue;
                }
            };

            let mut class_scopes = Vec::new();
            for unit in &scopes {
                let scope_props = match show_props(user, unit, &["Slice", "MemoryCurrent", "Id"]) {
                    Ok(v) => v,
                    Err(_) => {
                        out.partial = true;
                        continue;
                    }
                };
                if scope_props.get("Slice").is_some_and(|v| v == &slice) {
                    class_scopes.push(TreeScope {
                        unit: scope_props
                            .get("Id")
                            .cloned()
                            .unwrap_or_else(|| unit.clone()),
                        memory_current: parse_prop_u64(&scope_props, "MemoryCurrent"),
                    });
                }
            }
            class_scopes.sort_by(|a, b| {
                b.memory_current
                    .unwrap_or(0)
                    .cmp(&a.memory_current.unwrap_or(0))
                    .then_with(|| a.unit.cmp(&b.unit))
            });
            class_scopes.truncate(scope_limit);

            seen.insert(
                (slice.clone(), manager.to_string()),
                TreeClassSlice {
                    class: class_name_from_slice(&slice),
                    slice,
                    source: manager.to_string(),
                    memory_current: parse_prop_u64(&props, "MemoryCurrent"),
                    scopes: class_scopes,
                },
            );
        }
    }

    out.roots.sort_by(|a, b| a.name.cmp(&b.name));
    out.classes = seen.into_values().collect();
    out.classes
        .sort_by(|a, b| a.slice.cmp(&b.slice).then(a.source.cmp(&b.source)));
    out
}

pub fn collect_tree_snapshot(max_scopes: usize) -> TreeSnapshot {
    collect_tree_snapshot_with(max_scopes, systemctl_list_units, systemctl_show_props)
}

#[cfg(test)]
mod tests {
    use super::collect_tree_snapshot_with;
    use std::collections::BTreeMap;

    #[test]
    fn tree_contains_roots_classes_and_scopes() {
        let snap = collect_tree_snapshot_with(
            2,
            |user, ty| match (user, ty) {
                (false, "slice") => Ok(vec!["resguard-heavy.slice".into()]),
                (true, "slice") => Ok(vec!["resguard-browsers.slice".into()]),
                (false, "scope") => Ok(vec!["app-docker.scope".into()]),
                (true, "scope") => Ok(vec!["app-firefox.scope".into(), "app-chrome.scope".into()]),
                _ => Ok(Vec::new()),
            },
            |user, unit, _keys| {
                let mut m = BTreeMap::new();
                match (user, unit) {
                    (false, "system.slice") => {
                        m.insert("MemoryCurrent".into(), "1000".into());
                    }
                    (true, "user.slice") => {
                        m.insert("MemoryCurrent".into(), "2000".into());
                    }
                    (false, "resguard-heavy.slice") => {
                        m.insert("MemoryCurrent".into(), "3000".into());
                    }
                    (true, "resguard-browsers.slice") => {
                        m.insert("MemoryCurrent".into(), "4000".into());
                    }
                    (false, "app-docker.scope") => {
                        m.insert("Slice".into(), "resguard-heavy.slice".into());
                        m.insert("MemoryCurrent".into(), "2500".into());
                        m.insert("Id".into(), "app-docker.scope".into());
                    }
                    (true, "app-firefox.scope") => {
                        m.insert("Slice".into(), "resguard-browsers.slice".into());
                        m.insert("MemoryCurrent".into(), "2100".into());
                        m.insert("Id".into(), "app-firefox.scope".into());
                    }
                    (true, "app-chrome.scope") => {
                        m.insert("Slice".into(), "resguard-browsers.slice".into());
                        m.insert("MemoryCurrent".into(), "1900".into());
                        m.insert("Id".into(), "app-chrome.scope".into());
                    }
                    _ => {}
                }
                Ok(m)
            },
        );

        assert!(!snap.roots.is_empty());
        assert!(snap
            .classes
            .iter()
            .any(|c| c.slice == "resguard-heavy.slice"));
        assert!(snap
            .classes
            .iter()
            .any(|c| c.slice == "resguard-browsers.slice" && !c.scopes.is_empty()));
    }

    #[test]
    fn tree_marks_partial_when_system_queries_fail() {
        let snap = collect_tree_snapshot_with(
            3,
            |_user, _ty| Err(anyhow::anyhow!("no bus")),
            |_user, _unit, _keys| Err(anyhow::anyhow!("no bus")),
        );
        assert!(snap.partial);
        assert!(!snap.warnings.is_empty());
    }
}
