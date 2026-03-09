use crate::*;
use std::io;
use std::io::IsTerminal;
#[cfg(feature = "tui")]
use std::time::{Duration, Instant};

#[cfg(feature = "tui")]
use crossterm::event::{self, Event, KeyCode};
#[cfg(feature = "tui")]
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
#[cfg(feature = "tui")]
use crossterm::ExecutableCommand;

#[cfg(feature = "tui")]
use ratatui::backend::CrosstermBackend;
#[cfg(feature = "tui")]
use ratatui::layout::{Constraint, Direction, Layout};
#[cfg(feature = "tui")]
use ratatui::widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table};
#[cfg(feature = "tui")]
use ratatui::Terminal;

#[cfg(feature = "tui")]
use resguard_services::tui_service::{
    collect_tui_snapshot, TuiClassSlice, TuiLedgerAction, TuiSnapshot,
};

#[cfg(feature = "tui")]
fn pressure_pair(v: Option<resguard_model::PressureSnapshot>) -> (String, String) {
    match v {
        Some(p) => (format!("{:.2}", p.avg10), format!("{:.2}", p.avg60)),
        None => ("-".to_string(), "-".to_string()),
    }
}

#[cfg(feature = "tui")]
fn opt_bytes(v: Option<u64>) -> String {
    v.map(format_bytes_human).unwrap_or_else(|| "-".to_string())
}

#[cfg(feature = "tui")]
fn class_limit_text(v: Option<&str>) -> String {
    if let Some(s) = v {
        s.to_string()
    } else {
        "-".to_string()
    }
}

#[cfg(feature = "tui")]
fn action_text(action: &TuiLedgerAction) -> String {
    if action.actions.is_empty() {
        "-".to_string()
    } else {
        action.actions.join(",")
    }
}

#[cfg(feature = "tui")]
fn print_tui_summary(snapshot: &TuiSnapshot, no_top: bool) -> i32 {
    let mut partial = false;
    let (cpu10, cpu60) = pressure_pair(snapshot.cpu_pressure);
    let (mem10, mem60) = pressure_pair(snapshot.memory_pressure);
    let (io10, io60) = pressure_pair(snapshot.io_pressure);

    println!("mode=summary non_tty=true");
    println!(
        "psi cpu(avg10/avg60)={}/{} mem(avg10/avg60)={}/{} io(avg10/avg60)={}/{}",
        cpu10, cpu60, mem10, mem60, io10, io60
    );

    if snapshot.cpu_pressure.is_none()
        || snapshot.memory_pressure.is_none()
        || snapshot.io_pressure.is_none()
    {
        partial = true;
    }

    println!(
        "memory total={} available={}",
        opt_bytes(snapshot.mem_total_bytes),
        opt_bytes(snapshot.mem_available_bytes),
    );
    if snapshot.mem_total_bytes.is_none() || snapshot.mem_available_bytes.is_none() {
        partial = true;
    }

    if !no_top {
        println!("class_slices");
        if snapshot.classes.is_empty() {
            println!("unavailable");
            partial = true;
        } else {
            for class_row in snapshot.classes.iter().take(6) {
                println!(
                    "class={} slice={} src={} current={} high={} max={} cpu={}",
                    class_row.class,
                    class_row.slice,
                    class_row.source,
                    opt_bytes(class_row.memory_current),
                    class_limit_text(class_row.live_memory_high.as_deref()),
                    class_row.live_memory_max.as_deref().unwrap_or("-"),
                    class_row
                        .live_cpu_weight
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string())
                );
            }
        }

        println!("recent_actions");
        if snapshot.recent_actions.is_empty() {
            println!("none");
        } else {
            for row in &snapshot.recent_actions {
                println!(
                    "tick={} decision={} cooldown={} actions={} applied={} warnings={}",
                    row.tick
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    row.decision,
                    row.in_cooldown,
                    action_text(row),
                    row.applied.len(),
                    row.warnings.len(),
                );
            }
        }
    }

    partial_exit_code(partial)
}

#[cfg(feature = "tui")]
fn class_rows(snapshot: &TuiSnapshot) -> Vec<Row<'static>> {
    snapshot
        .classes
        .iter()
        .map(|row: &TuiClassSlice| {
            Row::new(vec![
                Cell::from(row.class.clone()),
                Cell::from(opt_bytes(row.memory_current)),
                Cell::from(
                    row.live_memory_high
                        .clone()
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(
                    row.live_memory_max
                        .clone()
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(
                    row.live_cpu_weight
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(row.source.clone()),
            ])
        })
        .collect()
}

#[cfg(feature = "tui")]
fn action_rows(snapshot: &TuiSnapshot) -> Vec<Row<'static>> {
    snapshot
        .recent_actions
        .iter()
        .map(|row: &TuiLedgerAction| {
            Row::new(vec![
                Cell::from(
                    row.tick
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                Cell::from(row.decision.clone()),
                Cell::from(if row.in_cooldown { "yes" } else { "no" }),
                Cell::from(action_text(row)),
                Cell::from(row.applied.len().to_string()),
                Cell::from(row.warnings.len().to_string()),
            ])
        })
        .collect()
}

#[cfg(feature = "tui")]
pub(crate) fn handle_tui(
    config_dir: &str,
    state_dir: &str,
    interval_ms: u64,
    no_top: bool,
) -> Result<i32> {
    println!("command=tui");
    if interval_ms == 0 {
        return Ok(2);
    }

    if !io::stdout().is_terminal() {
        let snapshot = collect_tui_snapshot(config_dir, state_dir);
        return Ok(print_tui_summary(&snapshot, no_top));
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = (|| -> Result<i32> {
        let tick = Duration::from_millis(interval_ms);
        let mut last = Instant::now()
            .checked_sub(tick)
            .unwrap_or_else(Instant::now);

        loop {
            if last.elapsed() >= tick {
                let snapshot = collect_tui_snapshot(config_dir, state_dir);
                terminal.draw(|f| {
                    let area = f.area();
                    let layout = if no_top {
                        Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Length(5),
                                Constraint::Length(4),
                                Constraint::Length(3),
                            ])
                            .split(area)
                    } else {
                        Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Length(5),
                                Constraint::Length(4),
                                Constraint::Length(3),
                                Constraint::Min(8),
                            ])
                            .split(area)
                    };

                    let (cpu10, cpu60) = pressure_pair(snapshot.cpu_pressure);
                    let (mem10, mem60) = pressure_pair(snapshot.memory_pressure);
                    let (io10, io60) = pressure_pair(snapshot.io_pressure);

                    let psi_text = format!(
                        "CPU avg10={} avg60={}   MEM avg10={} avg60={}   IO avg10={} avg60={}",
                        cpu10, cpu60, mem10, mem60, io10, io60
                    );
                    f.render_widget(
                        Paragraph::new(psi_text).block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("Pressure (PSI)"),
                        ),
                        layout[0],
                    );

                    let mem_line = format!(
                        "MemTotal={}  MemAvailable={}  ActiveClassSlices={}  RecentActions={}",
                        opt_bytes(snapshot.mem_total_bytes),
                        opt_bytes(snapshot.mem_available_bytes),
                        snapshot.classes.len(),
                        snapshot.recent_actions.len(),
                    );
                    f.render_widget(
                        Paragraph::new(mem_line)
                            .block(Block::default().borders(Borders::ALL).title("System")),
                        layout[1],
                    );

                    let ratio = match (snapshot.mem_total_bytes, snapshot.mem_available_bytes) {
                        (Some(total), Some(available)) if total > 0 && available <= total => {
                            ((total - available) as f64 / total as f64).min(1.0)
                        }
                        _ => 0.0,
                    };
                    f.render_widget(
                        Gauge::default()
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("Memory used (system-wide estimate)"),
                            )
                            .ratio(ratio)
                            .label(format!("{:.0}%", ratio * 100.0)),
                        layout[2],
                    );

                    if !no_top {
                        let lower = Layout::default()
                            .direction(Direction::Horizontal)
                            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                            .split(layout[3]);

                        let class_header = Row::new(vec![
                            Cell::from("Class"),
                            Cell::from("Current"),
                            Cell::from("High"),
                            Cell::from("Max"),
                            Cell::from("CPUW"),
                            Cell::from("Src"),
                        ]);
                        let class_table = Table::new(
                            class_rows(&snapshot),
                            [
                                Constraint::Length(10),
                                Constraint::Length(10),
                                Constraint::Length(10),
                                Constraint::Length(10),
                                Constraint::Length(6),
                                Constraint::Length(6),
                            ],
                        )
                        .header(class_header)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("Class slices (q to quit)"),
                        );
                        f.render_widget(class_table, lower[0]);

                        let action_header = Row::new(vec![
                            Cell::from("Tick"),
                            Cell::from("Decision"),
                            Cell::from("CD"),
                            Cell::from("Actions"),
                            Cell::from("Applied"),
                            Cell::from("Warn"),
                        ]);
                        let action_table = Table::new(
                            action_rows(&snapshot),
                            [
                                Constraint::Length(6),
                                Constraint::Length(10),
                                Constraint::Length(4),
                                Constraint::Percentage(50),
                                Constraint::Length(7),
                                Constraint::Length(5),
                            ],
                        )
                        .header(action_header)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("Recent daemon/autopilot actions"),
                        );
                        f.render_widget(action_table, lower[1]);
                    }
                })?;
                last = Instant::now();
            }

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(k) = event::read()? {
                    if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                        break;
                    }
                }
            }
        }

        Ok(0)
    })();

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}
