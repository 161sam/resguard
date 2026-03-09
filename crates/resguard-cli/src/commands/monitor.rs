use crate::output::{render_json, render_yaml};
use crate::util::system::{format_bytes_human, partial_exit_code};
use anyhow::Result;
use resguard_services::tui_service::{collect_tui_snapshot, TuiLedgerAction, TuiSnapshot};
use serde::Serialize;
use std::io;
use std::io::IsTerminal;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn pressure_pair(v: Option<resguard_model::PressureSnapshot>) -> (String, String) {
    match v {
        Some(p) => (format!("{:.2}", p.avg10), format!("{:.2}", p.avg60)),
        None => ("-".to_string(), "-".to_string()),
    }
}

fn opt_bytes(v: Option<u64>) -> String {
    v.map(format_bytes_human).unwrap_or_else(|| "-".to_string())
}

fn action_text(action: &TuiLedgerAction) -> String {
    if action.actions.is_empty() {
        "-".to_string()
    } else {
        action.actions.join(",")
    }
}

fn snapshot_partial(snapshot: &TuiSnapshot) -> bool {
    snapshot.cpu_pressure.is_none()
        || snapshot.memory_pressure.is_none()
        || snapshot.io_pressure.is_none()
        || snapshot.classes.is_empty()
}

fn c(text: &str, ansi: &str, enabled: bool) -> String {
    if enabled {
        format!("{ansi}{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn render_snapshot_lines(snapshot: &TuiSnapshot, plain: bool, color_enabled: bool) -> Vec<String> {
    let mut out = Vec::new();
    let (cpu10, cpu60) = pressure_pair(snapshot.cpu_pressure);
    let (mem10, mem60) = pressure_pair(snapshot.memory_pressure);
    let (io10, io60) = pressure_pair(snapshot.io_pressure);

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if plain {
        out.push(format!("monitor.ts={ts}"));
        out.push(format!(
            "pressure.cpu.avg10={cpu10} pressure.cpu.avg60={cpu60}"
        ));
        out.push(format!(
            "pressure.mem.avg10={mem10} pressure.mem.avg60={mem60}"
        ));
        out.push(format!("pressure.io.avg10={io10} pressure.io.avg60={io60}"));
        out.push(format!(
            "memory.total={} memory.available={}",
            opt_bytes(snapshot.mem_total_bytes),
            opt_bytes(snapshot.mem_available_bytes)
        ));
    } else {
        out.push(c("== resguard monitor ==", "\x1b[1;36m", color_enabled));
        out.push(format!(
            "pressure cpu(avg10/avg60)={cpu10}/{cpu60} mem(avg10/avg60)={mem10}/{mem60} io(avg10/avg60)={io10}/{io60}"
        ));
        out.push(format!(
            "memory total={} available={}",
            opt_bytes(snapshot.mem_total_bytes),
            opt_bytes(snapshot.mem_available_bytes)
        ));
    }

    if snapshot.classes.is_empty() {
        out.push("class_slices=unavailable (apply/setup profile first)".to_string());
    } else {
        for class_row in snapshot.classes.iter().take(8) {
            out.push(format!(
                "class={} slice={} src={} current={} high={} max={} cpu={}",
                class_row.class,
                class_row.slice,
                class_row.source,
                opt_bytes(class_row.memory_current),
                class_row.live_memory_high.as_deref().unwrap_or("-"),
                class_row.live_memory_max.as_deref().unwrap_or("-"),
                class_row
                    .live_cpu_weight
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string())
            ));
        }
    }

    out.push("recent_actions".to_string());
    if snapshot.recent_actions.is_empty() {
        out.push("none".to_string());
    } else {
        for row in &snapshot.recent_actions {
            out.push(format!(
                "tick={} decision={} cooldown={} actions={} applied={} reverted={} warnings={}",
                row.tick
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                row.decision,
                row.in_cooldown,
                action_text(row),
                row.applied.len(),
                row.reverted.len(),
                row.warnings.len()
            ));
        }
    }

    out
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorClassRow {
    class: String,
    slice: String,
    source: String,
    memory_current: Option<u64>,
    memory_high: Option<String>,
    memory_max: Option<String>,
    cpu_weight: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorActionRow {
    tick: Option<u64>,
    decision: String,
    actions: Vec<String>,
    applied_count: usize,
    reverted_count: usize,
    warning_count: usize,
    in_cooldown: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorSnapshotWire {
    cpu_pressure: Option<resguard_model::PressureSnapshot>,
    memory_pressure: Option<resguard_model::PressureSnapshot>,
    io_pressure: Option<resguard_model::PressureSnapshot>,
    mem_total_bytes: Option<u64>,
    mem_available_bytes: Option<u64>,
    classes: Vec<MonitorClassRow>,
    recent_actions: Vec<MonitorActionRow>,
}

fn to_wire(snapshot: &TuiSnapshot) -> MonitorSnapshotWire {
    MonitorSnapshotWire {
        cpu_pressure: snapshot.cpu_pressure,
        memory_pressure: snapshot.memory_pressure,
        io_pressure: snapshot.io_pressure,
        mem_total_bytes: snapshot.mem_total_bytes,
        mem_available_bytes: snapshot.mem_available_bytes,
        classes: snapshot
            .classes
            .iter()
            .map(|row| MonitorClassRow {
                class: row.class.clone(),
                slice: row.slice.clone(),
                source: row.source.clone(),
                memory_current: row.memory_current,
                memory_high: row.live_memory_high.clone(),
                memory_max: row.live_memory_max.clone(),
                cpu_weight: row.live_cpu_weight,
            })
            .collect(),
        recent_actions: snapshot
            .recent_actions
            .iter()
            .map(|row| MonitorActionRow {
                tick: row.tick,
                decision: row.decision.clone(),
                actions: row.actions.clone(),
                applied_count: row.applied.len(),
                reverted_count: row.reverted.len(),
                warning_count: row.warnings.len(),
                in_cooldown: row.in_cooldown,
            })
            .collect(),
    }
}

pub(crate) fn run(
    format: &str,
    config_dir: &str,
    state_dir: &str,
    watch: bool,
    interval_ms: u64,
    plain: bool,
    no_color: bool,
) -> Result<i32> {
    println!("command=monitor");
    println!("watch={watch} interval_ms={interval_ms} plain={plain}");
    if interval_ms == 0 {
        return Ok(2);
    }

    if !watch {
        let snapshot = collect_tui_snapshot(config_dir, state_dir);
        match format {
            "json" => render_json(&to_wire(&snapshot))?,
            "yaml" => render_yaml(&to_wire(&snapshot))?,
            _ => {
                let color_enabled =
                    !plain && !no_color && env_no_color_disabled() && io::stdout().is_terminal();
                for line in render_snapshot_lines(&snapshot, plain, color_enabled) {
                    println!("{line}");
                }
            }
        }
        return Ok(partial_exit_code(snapshot_partial(&snapshot)));
    }

    let tty = io::stdout().is_terminal();
    let color_enabled = !plain && !no_color && env_no_color_disabled() && tty;
    loop {
        let snapshot = collect_tui_snapshot(config_dir, state_dir);
        if tty && !plain && format == "table" {
            print!("\x1b[2J\x1b[H");
        } else {
            println!("---");
        }

        for line in render_snapshot_lines(&snapshot, plain || format != "table", color_enabled) {
            println!("{line}");
        }
        thread::sleep(Duration::from_millis(interval_ms));
    }
}

fn env_no_color_disabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

#[cfg(test)]
mod tests {
    use super::render_snapshot_lines;
    use resguard_model::PressureSnapshot;
    use resguard_services::tui_service::{TuiClassSlice, TuiLedgerAction, TuiSnapshot};

    #[test]
    fn plain_output_contains_pressure_class_and_actions() {
        let snap = TuiSnapshot {
            cpu_pressure: Some(PressureSnapshot {
                avg10: 1.0,
                avg60: 2.0,
            }),
            memory_pressure: Some(PressureSnapshot {
                avg10: 3.0,
                avg60: 4.0,
            }),
            io_pressure: Some(PressureSnapshot {
                avg10: 5.0,
                avg60: 6.0,
            }),
            mem_total_bytes: Some(8 * 1024 * 1024 * 1024),
            mem_available_bytes: Some(4 * 1024 * 1024 * 1024),
            classes: vec![TuiClassSlice {
                class: "browsers".to_string(),
                slice: "resguard-browsers.slice".to_string(),
                source: "user".to_string(),
                configured_memory_high: None,
                configured_memory_max: None,
                configured_cpu_weight: None,
                live_memory_high: Some("4G".to_string()),
                live_memory_max: Some("6G".to_string()),
                live_cpu_weight: Some(100),
                memory_current: Some(1024 * 1024 * 1024),
            }],
            recent_actions: vec![TuiLedgerAction {
                timestamp: Some(1),
                tick: Some(2),
                decision: "trigger".to_string(),
                actions: vec!["reduce-heavy-cpuweight".to_string()],
                applied: vec!["user:heavy:resguard-heavy.slice".to_string()],
                reverted: Vec::new(),
                warnings: Vec::new(),
                in_cooldown: false,
            }],
        };

        let lines = render_snapshot_lines(&snap, true, false).join("\n");
        assert!(lines.contains("pressure.cpu.avg10=1.00"));
        assert!(lines.contains("class=browsers"));
        assert!(lines.contains("recent_actions"));
        assert!(lines.contains("decision=trigger"));
    }

    #[test]
    fn human_output_marks_missing_class_data() {
        let snap = TuiSnapshot::default();
        let lines = render_snapshot_lines(&snap, false, false).join("\n");
        assert!(lines.contains("resguard monitor"));
        assert!(lines.contains("class_slices=unavailable"));
    }
}
