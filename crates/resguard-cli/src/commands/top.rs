use crate::output::{render_json, render_yaml};
use crate::util::system::{format_bytes_human, partial_exit_code};
use anyhow::Result;
use resguard_services::top_service::{collect_top_snapshot, TopClassRow, TopSnapshot};
use std::io;
use std::io::IsTerminal;

fn opt_bytes(v: Option<u64>) -> String {
    v.map(format_bytes_human).unwrap_or_else(|| "-".to_string())
}

fn opt_text(v: Option<&str>) -> String {
    v.unwrap_or("-").to_string()
}

fn opt_u16(v: Option<u16>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| "-".to_string())
}

fn scopes_text(class: &TopClassRow) -> String {
    if class.scopes.is_empty() {
        "-".to_string()
    } else {
        class
            .scopes
            .iter()
            .map(|s| format!("{}({})", s.unit, opt_bytes(s.memory_current)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn c(text: &str, ansi: &str, color_enabled: bool) -> String {
    if color_enabled {
        format!("{ansi}{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn top_table_lines(snapshot: &TopSnapshot, plain: bool, color_enabled: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if !plain {
        lines.push(c("== resguard top ==", "\x1b[1;36m", color_enabled));
    }
    if snapshot.classes.is_empty() {
        lines.push("no class slice data (active profile not found)".to_string());
    } else {
        for row in &snapshot.classes {
            let class = if plain {
                row.class.clone()
            } else {
                c(&row.class, "\x1b[1;33m", color_enabled)
            };
            lines.push(format!(
                "class={class} slice={} src={} current={} high={} max={} cpuw={}",
                row.slice,
                row.source,
                opt_bytes(row.memory_current),
                opt_text(row.live_memory_high.as_deref()),
                opt_text(row.live_memory_max.as_deref()),
                opt_u16(row.live_cpu_weight),
            ));
            lines.push(format!(
                "configured high={} max={} cpuw={}",
                opt_text(row.configured_memory_high.as_deref()),
                opt_text(row.configured_memory_max.as_deref()),
                opt_u16(row.configured_cpu_weight),
            ));
            lines.push(format!("scopes {}", scopes_text(row)));
        }
    }
    for warn in &snapshot.warnings {
        lines.push(format!("warn {warn}"));
    }
    lines
}

pub(crate) fn run(
    format: &str,
    config_dir: &str,
    state_dir: &str,
    scopes: usize,
    plain: bool,
    no_color: bool,
) -> Result<i32> {
    println!("command=top");
    println!("scopes={scopes} plain={plain}");
    let snapshot = collect_top_snapshot(config_dir, state_dir, scopes)?;

    match format {
        "json" => render_json(&snapshot)?,
        "yaml" => render_yaml(&snapshot)?,
        _ => {
            let color_enabled =
                !plain && !no_color && env_no_color_disabled() && io::stdout().is_terminal();
            for line in top_table_lines(&snapshot, plain, color_enabled) {
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
    use super::top_table_lines;
    use resguard_services::top_service::{TopClassRow, TopScopeRow, TopSnapshot};

    #[test]
    fn table_lines_include_scopes_and_limits() {
        let snap = TopSnapshot {
            classes: vec![TopClassRow {
                class: "browsers".to_string(),
                slice: "resguard-browsers.slice".to_string(),
                source: "user".to_string(),
                configured_memory_high: Some("4G".to_string()),
                configured_memory_max: Some("6G".to_string()),
                configured_cpu_weight: Some(100),
                memory_current: Some(1_073_741_824),
                live_memory_high: Some("4G".to_string()),
                live_memory_max: Some("6G".to_string()),
                live_cpu_weight: Some(100),
                scopes: vec![TopScopeRow {
                    unit: "app-firefox.scope".to_string(),
                    memory_current: Some(734_003_200),
                }],
            }],
            partial: false,
            warnings: Vec::new(),
        };

        let lines = top_table_lines(&snap, true, false);
        let joined = lines.join("\n");
        assert!(joined.contains("class=browsers"));
        assert!(joined.contains("current=1G"));
        assert!(joined.contains("configured high=4G"));
        assert!(joined.contains("app-firefox.scope(700M)"));
    }

    #[test]
    fn table_lines_show_missing_profile_hint() {
        let snap = TopSnapshot::default();
        let lines = top_table_lines(&snap, true, false);
        assert!(lines.iter().any(|l| l.contains("active profile not found")));
    }
}
