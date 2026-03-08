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
#[derive(Debug, Clone)]
struct TuiRow {
    unit: String,
    memory_current: u64,
}

#[cfg(feature = "tui")]
#[derive(Debug, Clone, Default)]
struct TuiSnapshot {
    cpu_avg10: Option<f64>,
    cpu_avg60: Option<f64>,
    mem_avg10: Option<f64>,
    mem_avg60: Option<f64>,
    io_avg10: Option<f64>,
    io_avg60: Option<f64>,
    mem_total: Option<u64>,
    mem_available: Option<u64>,
    user_slice_current: Option<u64>,
    user_slice_max: Option<u64>,
    top_units: Vec<TuiRow>,
}

#[cfg(feature = "tui")]
fn collect_tui_snapshot() -> TuiSnapshot {
    let mut snap = TuiSnapshot::default();

    if let Ok(Some(v)) = read_pressure("/proc/pressure/cpu") {
        snap.cpu_avg10 = Some(v.avg10);
        snap.cpu_avg60 = Some(v.avg60);
    }
    if let Ok(Some(v)) = read_pressure("/proc/pressure/memory") {
        snap.mem_avg10 = Some(v.avg10);
        snap.mem_avg60 = Some(v.avg60);
    }
    if let Ok(Some(v)) = read_pressure("/proc/pressure/io") {
        snap.io_avg10 = Some(v.avg10);
        snap.io_avg60 = Some(v.avg60);
    }

    snap.mem_total = read_mem_total_bytes().ok();
    snap.mem_available = read_mem_available_bytes().ok();

    if let Ok(props) = systemctl_show_props(false, "user.slice", &["MemoryCurrent", "MemoryMax"]) {
        snap.user_slice_current = parse_prop_u64(&props, "MemoryCurrent");
        snap.user_slice_max = parse_prop_u64(&props, "MemoryMax");
    }

    let mut units = Vec::new();
    for unit_type in ["slice", "scope"] {
        if let Ok(list) = systemctl_list_units(false, unit_type) {
            for unit in list {
                if !unit.ends_with(".slice") && !unit.ends_with(".scope") {
                    continue;
                }
                if let Ok(props) = systemctl_show_props(false, &unit, &["MemoryCurrent"]) {
                    if let Some(cur) = parse_prop_u64(&props, "MemoryCurrent") {
                        if cur > 0 {
                            units.push(TuiRow {
                                unit,
                                memory_current: cur,
                            });
                        }
                    }
                }
            }
        }
    }
    units.sort_by(|a, b| b.memory_current.cmp(&a.memory_current));
    units.dedup_by(|a, b| a.unit == b.unit);
    snap.top_units = units.into_iter().take(10).collect();

    snap
}

#[cfg(feature = "tui")]
fn opt_f64(v: Option<f64>) -> String {
    v.map(|x| format!("{x:.2}"))
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(feature = "tui")]
fn opt_bytes(v: Option<u64>) -> String {
    v.map(format_bytes_human).unwrap_or_else(|| "-".to_string())
}

#[cfg(feature = "tui")]
fn print_tui_summary(snapshot: &TuiSnapshot, no_top: bool) -> i32 {
    let mut partial = false;
    println!("mode=summary non_tty=true");
    println!(
        "psi cpu(avg10/avg60)={}/{} mem(avg10/avg60)={}/{} io(avg10/avg60)={}/{}",
        opt_f64(snapshot.cpu_avg10),
        opt_f64(snapshot.cpu_avg60),
        opt_f64(snapshot.mem_avg10),
        opt_f64(snapshot.mem_avg60),
        opt_f64(snapshot.io_avg10),
        opt_f64(snapshot.io_avg60)
    );
    if snapshot.cpu_avg10.is_none()
        || snapshot.mem_avg10.is_none()
        || snapshot.io_avg10.is_none()
        || snapshot.cpu_avg60.is_none()
        || snapshot.mem_avg60.is_none()
        || snapshot.io_avg60.is_none()
    {
        partial = true;
    }

    println!(
        "memory total={} available={} user.slice.current={} user.slice.max={}",
        opt_bytes(snapshot.mem_total),
        opt_bytes(snapshot.mem_available),
        opt_bytes(snapshot.user_slice_current),
        opt_bytes(snapshot.user_slice_max),
    );
    if snapshot.mem_total.is_none() || snapshot.mem_available.is_none() {
        partial = true;
    }

    if !no_top {
        println!("top_slices");
        if snapshot.top_units.is_empty() {
            println!("unavailable");
            partial = true;
        } else {
            for row in snapshot.top_units.iter().take(5) {
                println!("{}\t{}", row.unit, format_bytes_human(row.memory_current));
            }
        }
    }

    partial_exit_code(partial)
}

#[cfg(feature = "tui")]
pub(crate) fn handle_tui(interval_ms: u64, no_top: bool) -> Result<i32> {
    println!("command=tui");
    if interval_ms == 0 {
        return Ok(2);
    }

    if !io::stdout().is_terminal() {
        let snapshot = collect_tui_snapshot();
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
                let snapshot = collect_tui_snapshot();
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

                    let psi_text = format!(
                        "CPU avg10={} avg60={}   MEM avg10={} avg60={}   IO avg10={} avg60={}",
                        opt_f64(snapshot.cpu_avg10),
                        opt_f64(snapshot.cpu_avg60),
                        opt_f64(snapshot.mem_avg10),
                        opt_f64(snapshot.mem_avg60),
                        opt_f64(snapshot.io_avg10),
                        opt_f64(snapshot.io_avg60)
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
                        "MemTotal={}  MemAvailable={}  user.slice MemoryCurrent={}  MemoryMax={}",
                        opt_bytes(snapshot.mem_total),
                        opt_bytes(snapshot.mem_available),
                        opt_bytes(snapshot.user_slice_current),
                        opt_bytes(snapshot.user_slice_max)
                    );
                    f.render_widget(
                        Paragraph::new(mem_line)
                            .block(Block::default().borders(Borders::ALL).title("Memory")),
                        layout[1],
                    );

                    let ratio = match (snapshot.user_slice_current, snapshot.user_slice_max) {
                        (Some(cur), Some(max)) if max > 0 => (cur as f64 / max as f64).min(1.0),
                        _ => 0.0,
                    };
                    f.render_widget(
                        Gauge::default()
                            .block(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .title("user.slice usage"),
                            )
                            .ratio(ratio)
                            .label(format!("{:.0}%", ratio * 100.0)),
                        layout[2],
                    );

                    if !no_top {
                        let header =
                            Row::new(vec![Cell::from("Unit"), Cell::from("MemoryCurrent")]);
                        let rows = snapshot.top_units.iter().map(|r| {
                            Row::new(vec![
                                Cell::from(r.unit.clone()),
                                Cell::from(format_bytes_human(r.memory_current)),
                            ])
                        });
                        let table = Table::new(
                            rows,
                            [Constraint::Percentage(70), Constraint::Percentage(30)],
                        )
                        .header(header)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("Top scopes/slices by MemoryCurrent (q to quit)"),
                        );
                        f.render_widget(table, layout[3]);
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
