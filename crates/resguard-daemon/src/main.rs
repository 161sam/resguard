use anyhow::{anyhow, Context, Result};
use clap::Parser;
use resguard_services::daemon_service::{
    daemon_autopilot_tick, DaemonAutopilotState, DaemonAutopilotTick,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "resguardd", about = "Resguard autopilot daemon")]
struct Cli {
    #[arg(long, default_value = "/etc/resguard/resguardd.yml")]
    config: String,
    #[arg(long, default_value = "/etc/resguard")]
    config_dir: String,
    #[arg(long, default_value = "/var/lib/resguard")]
    state_dir: String,
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonConfig {
    #[serde(default = "default_poll_interval_ms")]
    poll_interval_ms: u64,
    log_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LedgerRecord {
    timestamp: u64,
    tick: u64,
    decision: String,
    transition: Option<String>,
    in_cooldown: bool,
    had_profile: bool,
    decision_actions: Vec<String>,
    applied: Vec<String>,
    reverted: Vec<String>,
    skipped_noop: Vec<String>,
    warnings: Vec<String>,
}

struct Logger {
    file_path: Option<String>,
}

impl Logger {
    fn new(file_path: Option<String>) -> Self {
        Self { file_path }
    }

    fn log(&self, level: &str, event: &str, msg: &str) {
        let ts = now_unix();
        let line = serde_json::json!({
            "ts": ts,
            "level": level,
            "event": event,
            "msg": msg
        })
        .to_string();
        println!("{line}");
        if let Some(path) = &self.file_path {
            let _ = append_line(path, &line);
        }
    }
}

fn default_poll_interval_ms() -> u64 {
    5000
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: default_poll_interval_ms(),
            log_file: None,
        }
    }
}

fn validate_config(cfg: &DaemonConfig) -> Result<()> {
    if cfg.poll_interval_ms < 200 {
        return Err(anyhow!("poll_interval_ms must be >= 200"));
    }
    Ok(())
}

fn load_config(path: &str) -> Result<DaemonConfig> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read config {}", path))?;
    let cfg: DaemonConfig =
        serde_yaml::from_str(&content).with_context(|| format!("invalid yaml in {}", path))?;
    validate_config(&cfg)?;
    Ok(cfg)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn append_line(path: &str, line: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

fn append_ledger_record(path: &Path, rec: &LedgerRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(rec)?;
    writeln!(f, "{line}")?;
    Ok(())
}

fn decision_for_tick(tick: &DaemonAutopilotTick) -> &'static str {
    if !tick.had_profile {
        "no-profile"
    } else if tick.in_cooldown {
        "cooldown"
    } else if !tick.reverted.is_empty()
        || tick
            .decision_actions
            .iter()
            .any(|a| a == "revert-adaptive-limits")
    {
        "revert"
    } else if !tick.decision_actions.is_empty() || !tick.applied.is_empty() {
        "trigger"
    } else {
        "idle"
    }
}

fn record_from_tick(tick_no: u64, out: DaemonAutopilotTick) -> LedgerRecord {
    LedgerRecord {
        timestamp: now_unix(),
        tick: tick_no,
        decision: decision_for_tick(&out).to_string(),
        transition: out.transition,
        in_cooldown: out.in_cooldown,
        had_profile: out.had_profile,
        decision_actions: out.decision_actions,
        applied: out.applied,
        reverted: out.reverted,
        skipped_noop: out.skipped_noop,
        warnings: out.warnings,
    }
}

fn run_once_with<F>(
    logger: &Logger,
    state: &mut DaemonAutopilotState,
    mut tick: F,
) -> Result<(i32, LedgerRecord)>
where
    F: FnMut(&mut DaemonAutopilotState) -> Result<DaemonAutopilotTick>,
{
    let out = tick(state)?;
    let record = record_from_tick(state.tick, out);
    logger.log(
        "INFO",
        "once_decision",
        &format!(
            "decision={} actions={} applied={} reverted={} cooldown={} profile={}",
            record.decision,
            record.decision_actions.len(),
            record.applied.len(),
            record.reverted.len(),
            record.in_cooldown,
            record.had_profile,
        ),
    );
    let code = if record.decision == "trigger" { 1 } else { 0 };
    Ok((code, record))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config(&cli.config)?;
    let logger = Logger::new(cfg.log_file.clone());
    let ledger_path = PathBuf::from(&cli.state_dir).join("daemon-ledger.jsonl");
    let terminate = Arc::new(AtomicBool::new(false));
    let term_flag = Arc::clone(&terminate);
    ctrlc::set_handler(move || {
        term_flag.store(true, Ordering::Relaxed);
    })
    .context("failed to install signal handler")?;

    let mut state = DaemonAutopilotState::default();

    if cli.once {
        let (code, record) = run_once_with(&logger, &mut state, |s| {
            daemon_autopilot_tick(&cli.config_dir, &cli.state_dir, s)
        })?;
        append_ledger_record(&ledger_path, &record)?;
        process::exit(code);
    }

    logger.log(
        "INFO",
        "daemon_start",
        &format!(
            "resguardd started poll={}ms ledger={} config_dir={} state_dir={}",
            cfg.poll_interval_ms,
            ledger_path.display(),
            cli.config_dir,
            cli.state_dir,
        ),
    );

    loop {
        if terminate.load(Ordering::Relaxed) {
            logger.log("INFO", "signal", "termination requested; exiting cleanly");
            break;
        }

        match daemon_autopilot_tick(&cli.config_dir, &cli.state_dir, &mut state) {
            Ok(out) => {
                let record = record_from_tick(state.tick, out);
                logger.log(
                    "INFO",
                    "tick",
                    &format!(
                        "tick={} decision={} transition={} actions={} applied={} reverted={} noop={} cooldown={} profile={}",
                        record.tick,
                        record.decision,
                        record.transition.as_deref().unwrap_or("-"),
                        record.decision_actions.len(),
                        record.applied.len(),
                        record.reverted.len(),
                        record.skipped_noop.len(),
                        record.in_cooldown,
                        record.had_profile,
                    ),
                );
                if let Err(err) = append_ledger_record(&ledger_path, &record) {
                    logger.log(
                        "ERROR",
                        "ledger_write",
                        &format!("failed to append ledger: {err}"),
                    );
                }
            }
            Err(err) => {
                logger.log(
                    "WARN",
                    "tick_failed",
                    &format!("autopilot tick failed: {err}"),
                );
            }
        }

        thread::sleep(Duration::from_millis(cfg.poll_interval_ms));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decision_for_tick, run_once_with, DaemonAutopilotState, Logger};
    use anyhow::Result;
    use resguard_services::daemon_service::DaemonAutopilotTick;

    fn tick_trigger() -> DaemonAutopilotTick {
        DaemonAutopilotTick {
            had_profile: true,
            decision_actions: vec!["reduce-heavy-cpuweight".to_string()],
            applied: vec!["user:heavy:resguard-heavy.slice".to_string()],
            ..DaemonAutopilotTick::default()
        }
    }

    fn tick_idle() -> DaemonAutopilotTick {
        DaemonAutopilotTick {
            had_profile: true,
            ..DaemonAutopilotTick::default()
        }
    }

    fn tick_cooldown() -> DaemonAutopilotTick {
        DaemonAutopilotTick {
            had_profile: true,
            in_cooldown: true,
            ..DaemonAutopilotTick::default()
        }
    }

    fn tick_revert() -> DaemonAutopilotTick {
        DaemonAutopilotTick {
            had_profile: true,
            decision_actions: vec!["revert-adaptive-limits".to_string()],
            reverted: vec!["user:heavy:resguard-heavy.slice".to_string()],
            ..DaemonAutopilotTick::default()
        }
    }

    #[test]
    fn once_mode_trigger_path_returns_non_zero() {
        let logger = Logger::new(None);
        let mut state = DaemonAutopilotState::default();
        let (code, rec) =
            run_once_with(&logger, &mut state, |_s| Ok(tick_trigger())).expect("once");
        assert_eq!(code, 1);
        assert_eq!(rec.decision, "trigger");
    }

    #[test]
    fn once_mode_no_action_path_returns_zero() {
        let logger = Logger::new(None);
        let mut state = DaemonAutopilotState::default();
        let (code, rec) = run_once_with(&logger, &mut state, |_s| Ok(tick_idle())).expect("once");
        assert_eq!(code, 0);
        assert_eq!(rec.decision, "idle");
    }

    #[test]
    fn action_trigger_path_is_classified_as_trigger() {
        assert_eq!(decision_for_tick(&tick_trigger()), "trigger");
    }

    #[test]
    fn cooldown_behavior_is_classified_as_cooldown() {
        assert_eq!(decision_for_tick(&tick_cooldown()), "cooldown");
    }

    #[test]
    fn no_action_path_is_classified_as_idle() {
        assert_eq!(decision_for_tick(&tick_idle()), "idle");
    }

    #[test]
    fn revert_path_is_classified_as_revert() {
        assert_eq!(decision_for_tick(&tick_revert()), "revert");
    }

    #[test]
    fn no_profile_path_is_classified_as_no_profile() {
        let tick = DaemonAutopilotTick::default();
        assert_eq!(decision_for_tick(&tick), "no-profile");
    }

    #[test]
    fn once_mode_tick_state_is_mutable() {
        let logger = Logger::new(None);
        let mut state = DaemonAutopilotState::default();
        let (code, _) = run_once_with(&logger, &mut state, |s| {
            s.tick = s.tick.saturating_add(1);
            Ok(tick_idle())
        })
        .expect("once");
        assert_eq!(code, 0);
        assert_eq!(state.tick, 1);
    }

    #[test]
    fn once_mode_bubbles_tick_errors() {
        let logger = Logger::new(None);
        let mut state = DaemonAutopilotState::default();
        let err = run_once_with(&logger, &mut state, |_s| -> Result<DaemonAutopilotTick> {
            Err(anyhow::anyhow!("tick failed"))
        })
        .expect_err("must fail");
        assert!(err.to_string().contains("tick failed"));
    }
}
