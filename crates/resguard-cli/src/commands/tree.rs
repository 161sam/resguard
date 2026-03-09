use crate::output::{render_json, render_yaml};
use crate::util::system::{format_bytes_human, partial_exit_code};
use anyhow::Result;
use resguard_services::tree_service::{collect_tree_snapshot, TreeClassSlice, TreeSnapshot};
use std::io;
use std::io::IsTerminal;

fn opt_bytes(v: Option<u64>) -> String {
    v.map(format_bytes_human).unwrap_or_else(|| "-".to_string())
}

fn c(text: &str, ansi: &str, enabled: bool) -> String {
    if enabled {
        format!("{ansi}{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn class_line(class_row: &TreeClassSlice) -> String {
    format!(
        "{} (class={} src={} current={})",
        class_row.slice,
        class_row.class,
        class_row.source,
        opt_bytes(class_row.memory_current)
    )
}

fn tree_lines(snapshot: &TreeSnapshot, plain: bool, color_enabled: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if plain {
        for root in &snapshot.roots {
            lines.push(format!(
                "root={} source={} current={} high={} max={}",
                root.name,
                root.source,
                opt_bytes(root.memory_current),
                root.memory_high.as_deref().unwrap_or("-"),
                root.memory_max.as_deref().unwrap_or("-")
            ));
            for class_row in snapshot.classes.iter().filter(|c| c.source == root.source) {
                lines.push(format!(
                    "class_slice={} class={} source={} current={}",
                    class_row.slice,
                    class_row.class,
                    class_row.source,
                    opt_bytes(class_row.memory_current)
                ));
                for scope in &class_row.scopes {
                    lines.push(format!(
                        "scope={} parent={} current={}",
                        scope.unit,
                        class_row.slice,
                        opt_bytes(scope.memory_current)
                    ));
                }
            }
        }
    } else {
        lines.push(c("== resguard tree ==", "\x1b[1;36m", color_enabled));
        for root in &snapshot.roots {
            lines.push(format!(
                "{} (src={} current={} high={} max={})",
                c(&root.name, "\x1b[1;33m", color_enabled),
                root.source,
                opt_bytes(root.memory_current),
                root.memory_high.as_deref().unwrap_or("-"),
                root.memory_max.as_deref().unwrap_or("-")
            ));
            let classes = snapshot
                .classes
                .iter()
                .filter(|c| c.source == root.source)
                .collect::<Vec<_>>();
            if classes.is_empty() {
                lines.push("  +- no resguard class slices".to_string());
                continue;
            }
            for class_row in classes {
                lines.push(format!("  +- {}", class_line(class_row)));
                if class_row.scopes.is_empty() {
                    lines.push("  |  \\- no notable scopes".to_string());
                } else {
                    for scope in &class_row.scopes {
                        lines.push(format!(
                            "  |  \\- {} (current={})",
                            scope.unit,
                            opt_bytes(scope.memory_current)
                        ));
                    }
                }
            }
        }
    }
    for warning in &snapshot.warnings {
        lines.push(format!("warn {warning}"));
    }
    lines
}

pub(crate) fn run(format: &str, scopes: usize, plain: bool, no_color: bool) -> Result<i32> {
    println!("command=tree");
    println!("scopes={scopes} plain={plain}");
    let snapshot = collect_tree_snapshot(scopes);
    match format {
        "json" => render_json(&snapshot)?,
        "yaml" => render_yaml(&snapshot)?,
        _ => {
            let color_enabled =
                !plain && !no_color && env_no_color_disabled() && io::stdout().is_terminal();
            for line in tree_lines(&snapshot, plain, color_enabled) {
                println!("{line}");
            }
        }
    }
    Ok(partial_exit_code(snapshot.partial))
}

fn env_no_color_disabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

#[cfg(test)]
mod tests {
    use super::tree_lines;
    use resguard_services::tree_service::{TreeClassSlice, TreeRoot, TreeScope, TreeSnapshot};

    #[test]
    fn human_tree_includes_roots_classes_and_scopes() {
        let snap = TreeSnapshot {
            roots: vec![TreeRoot {
                name: "user.slice".to_string(),
                source: "user".to_string(),
                memory_current: Some(2 * 1024 * 1024 * 1024),
                memory_high: Some("12G".to_string()),
                memory_max: Some("14G".to_string()),
            }],
            classes: vec![TreeClassSlice {
                class: "browsers".to_string(),
                slice: "resguard-browsers.slice".to_string(),
                source: "user".to_string(),
                memory_current: Some(1024 * 1024 * 1024),
                scopes: vec![TreeScope {
                    unit: "app-firefox.scope".to_string(),
                    memory_current: Some(700 * 1024 * 1024),
                }],
            }],
            partial: false,
            warnings: vec![],
        };
        let txt = tree_lines(&snap, false, false).join("\n");
        assert!(txt.contains("resguard tree"));
        assert!(txt.contains("user.slice"));
        assert!(txt.contains("resguard-browsers.slice"));
        assert!(txt.contains("app-firefox.scope"));
    }

    #[test]
    fn plain_tree_is_script_friendly() {
        let snap = TreeSnapshot {
            roots: vec![TreeRoot {
                name: "system.slice".to_string(),
                source: "system".to_string(),
                memory_current: None,
                memory_high: None,
                memory_max: None,
            }],
            classes: vec![],
            partial: true,
            warnings: vec!["cannot list slice units".to_string()],
        };
        let txt = tree_lines(&snap, true, false).join("\n");
        assert!(txt.contains("root=system.slice"));
        assert!(txt.contains("warn cannot list slice units"));
    }
}
