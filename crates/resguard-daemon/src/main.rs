use anyhow::{anyhow, Context, Result};
use clap::Parser;
use resguard_runtime::{parse_prop_u64, read_mem_total_bytes, read_pressure, systemctl_show_props};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "resguardd", about = "Resguard freeze watchdog daemon")]
struct Cli {
    #[arg(long, default_value = "/etc/resguard/resguardd.yml")]
    config: String,
    #[arg(long, default_value = "/var/lib/resguard")]
    state_dir: String,
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WatchdogAction {
    Panic,
    SetProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DaemonConfig {
    memory_avg10_threshold: f64,
    cpu_avg10_threshold: f64,
    hold_seconds: u64,
    cooldown_seconds: u64,
    action: WatchdogAction,
    #[serde(default = "default_action_duration_seconds")]
    action_duration_seconds: u64,
    #[serde(default = "default_poll_interval_ms")]
    poll_interval_ms: u64,
    log_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LedgerRecord {
    timestamp: u64,
    mem_avg10: f64,
    mem_avg60: f64,
    cpu_avg10: f64,
    cpu_avg60: f64,
    action: String,
    decision: String,
    revert_ok: Option<bool>,
}

#[derive(Debug, Clone)]
struct SetPropertySnapshot {
    before_high: String,
    before_max: String,
}

#[derive(Debug, Clone)]
struct ActionOutcome {
    action: String,
    revert_ok: Option<bool>,
}

trait CommandRunner {
    fn run_command(&mut self, program: &str, args: &[String]) -> Result<bool>;
}

struct OsCommandRunner;

impl CommandRunner for OsCommandRunner {
    fn run_command(&mut self, program: &str, args: &[String]) -> Result<bool> {
        let status = Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("failed to execute {program}"))?;
        Ok(status.success())
    }
}

#[derive(Debug, Clone, Default)]
struct WatchdogState {
    exceed_since_sec: Option<u64>,
    cooldown_until_sec: Option<u64>,
}

fn default_action_duration_seconds() -> u64 {
    60
}

fn default_poll_interval_ms() -> u64 {
    1000
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            memory_avg10_threshold: 10.0,
            cpu_avg10_threshold: 10.0,
            hold_seconds: 5,
            cooldown_seconds: 60,
            action: WatchdogAction::Panic,
            action_duration_seconds: default_action_duration_seconds(),
            poll_interval_ms: default_poll_interval_ms(),
            log_file: None,
        }
    }
}

fn validate_config(cfg: &DaemonConfig) -> Result<()> {
    if !(0.0..=100.0).contains(&cfg.memory_avg10_threshold) || cfg.memory_avg10_threshold == 0.0 {
        return Err(anyhow!("memory_avg10_threshold must be in range (0, 100]"));
    }
    if !(0.0..=100.0).contains(&cfg.cpu_avg10_threshold) || cfg.cpu_avg10_threshold == 0.0 {
        return Err(anyhow!("cpu_avg10_threshold must be in range (0, 100]"));
    }
    if cfg.hold_seconds == 0 {
        return Err(anyhow!("hold_seconds must be > 0"));
    }
    if cfg.cooldown_seconds == 0 {
        return Err(anyhow!("cooldown_seconds must be > 0"));
    }
    if cfg.action_duration_seconds == 0 {
        return Err(anyhow!("action_duration_seconds must be > 0"));
    }
    if cfg.poll_interval_ms < 200 {
        return Err(anyhow!("poll_interval_ms must be >= 200"));
    }
    Ok(())
}

struct Logger {
    file_path: Option<String>,
}

impl Logger {
    fn new(file_path: Option<String>) -> Self {
        Self { file_path }
    }

    fn log(&self, level: &str, event: &str, msg: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
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

fn append_line(path: &str, line: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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

#[derive(Debug, Clone, Copy)]
struct PsiSnapshot {
    mem_avg10: f64,
    mem_avg60: f64,
    cpu_avg10: f64,
    cpu_avg60: f64,
}

fn read_psi_snapshot() -> Result<PsiSnapshot> {
    let mem = read_pressure("/proc/pressure/memory")?
        .ok_or_else(|| anyhow!("memory PSI unavailable in /proc/pressure/memory"))?;
    let cpu = read_pressure("/proc/pressure/cpu")?
        .ok_or_else(|| anyhow!("cpu PSI unavailable in /proc/pressure/cpu"))?;
    Ok(PsiSnapshot {
        mem_avg10: mem.avg10,
        mem_avg60: mem.avg60,
        cpu_avg10: cpu.avg10,
        cpu_avg60: cpu.avg60,
    })
}

fn threshold_exceeded(cfg: &DaemonConfig, s: PsiSnapshot) -> bool {
    s.mem_avg10 >= cfg.memory_avg10_threshold
        || s.mem_avg60 >= cfg.memory_avg10_threshold
        || s.cpu_avg10 >= cfg.cpu_avg10_threshold
        || s.cpu_avg60 >= cfg.cpu_avg10_threshold
}

fn run_once_with_snapshot(cfg: &DaemonConfig, logger: &Logger, snap: PsiSnapshot) -> i32 {
    let over = threshold_exceeded(cfg, snap);
    let decision = if over { "trigger" } else { "idle" };
    logger.log(
        "INFO",
        "once_decision",
        &format!(
            "decision={} mem(avg10/avg60)={:.2}/{:.2} cpu(avg10/avg60)={:.2}/{:.2}",
            decision, snap.mem_avg10, snap.mem_avg60, snap.cpu_avg10, snap.cpu_avg60
        ),
    );
    if over {
        1
    } else {
        0
    }
}

fn should_trigger_watchdog(
    cfg: &DaemonConfig,
    state: &mut WatchdogState,
    now_sec: u64,
    snap: PsiSnapshot,
) -> bool {
    let over = threshold_exceeded(cfg, snap);
    if over {
        if state.exceed_since_sec.is_none() {
            state.exceed_since_sec = Some(now_sec);
        }
    } else {
        state.exceed_since_sec = None;
        return false;
    }

    let in_cooldown = state
        .cooldown_until_sec
        .map(|until| now_sec < until)
        .unwrap_or(false);
    if in_cooldown {
        return false;
    }

    if let Some(since) = state.exceed_since_sec {
        if now_sec.saturating_sub(since) >= cfg.hold_seconds {
            state.exceed_since_sec = None;
            state.cooldown_until_sec = Some(now_sec.saturating_add(cfg.cooldown_seconds));
            return true;
        }
    }
    false
}

fn run_panic_action(
    cfg: &DaemonConfig,
    logger: &Logger,
    runner: &mut dyn CommandRunner,
) -> Result<ActionOutcome> {
    let duration = format!("{}s", cfg.action_duration_seconds);
    logger.log(
        "WARN",
        "action_trigger",
        &format!("action=panic duration={duration} via resguard panic"),
    );
    let args = vec!["panic".to_string(), "--duration".to_string(), duration];
    let ok = runner.run_command("resguard", &args)?;
    if !ok {
        return Err(anyhow!("resguard panic failed"));
    }
    Ok(ActionOutcome {
        action: "panic".to_string(),
        revert_ok: None,
    })
}

fn set_property_snapshot() -> Result<SetPropertySnapshot> {
    let props = systemctl_show_props(false, "user.slice", &["MemoryMax", "MemoryCurrent"])?;
    let before_max = props
        .get("MemoryMax")
        .cloned()
        .unwrap_or_else(|| "infinity".to_string());
    let before_high = systemctl_show_props(false, "user.slice", &["MemoryHigh"])?
        .get("MemoryHigh")
        .cloned()
        .unwrap_or_else(|| "infinity".to_string());
    Ok(SetPropertySnapshot {
        before_high,
        before_max,
    })
}

fn apply_set_property_limits(
    logger: &Logger,
    runner: &mut dyn CommandRunner,
    base: u64,
) -> Result<()> {
    let target_high = (base as f64 * 0.5) as u64;
    let target_max = (base as f64 * 0.6) as u64;
    logger.log(
        "WARN",
        "action_trigger",
        &format!(
            "action=set-property target_high={} target_max={}",
            target_high, target_max
        ),
    );
    let args = vec![
        "set-property".to_string(),
        "user.slice".to_string(),
        format!("MemoryHigh={target_high}"),
        format!("MemoryMax={target_max}"),
    ];
    let ok = runner.run_command("systemctl", &args)?;
    if !ok {
        return Err(anyhow!("systemctl set-property failed"));
    }
    Ok(())
}

fn revert_set_property(
    snapshot: &SetPropertySnapshot,
    logger: &Logger,
    runner: &mut dyn CommandRunner,
) -> Result<()> {
    let args = vec![
        "set-property".to_string(),
        "user.slice".to_string(),
        format!("MemoryHigh={}", snapshot.before_high),
        format!("MemoryMax={}", snapshot.before_max),
    ];
    let ok = runner.run_command("systemctl", &args)?;
    if !ok {
        return Err(anyhow!("set-property revert failed"));
    }
    logger.log("INFO", "action_revert", "set-property action reverted");
    Ok(())
}

fn run_set_property_action_with(
    cfg: &DaemonConfig,
    logger: &Logger,
    terminate: &Arc<AtomicBool>,
    runner: &mut dyn CommandRunner,
    snapshot: SetPropertySnapshot,
    base: u64,
) -> Result<ActionOutcome> {
    apply_set_property_limits(logger, runner, base)?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(cfg.action_duration_seconds) {
        if terminate.load(Ordering::Relaxed) {
            logger.log(
                "WARN",
                "signal",
                "termination requested during action duration; reverting early",
            );
            let revert_ok = revert_set_property(&snapshot, logger, runner).is_ok();
            return Ok(ActionOutcome {
                action: "set-property".to_string(),
                revert_ok: Some(revert_ok),
            });
        }
        thread::sleep(Duration::from_millis(200));
    }
    let revert_ok = revert_set_property(&snapshot, logger, runner).is_ok();
    Ok(ActionOutcome {
        action: "set-property".to_string(),
        revert_ok: Some(revert_ok),
    })
}

fn run_set_property_action(
    cfg: &DaemonConfig,
    logger: &Logger,
    terminate: &Arc<AtomicBool>,
    runner: &mut dyn CommandRunner,
) -> Result<ActionOutcome> {
    let snapshot = set_property_snapshot()?;
    let props = systemctl_show_props(false, "user.slice", &["MemoryMax", "MemoryCurrent"])?;
    let base = parse_prop_u64(&props, "MemoryMax")
        .filter(|v| *v > 0)
        .or_else(|| parse_prop_u64(&props, "MemoryCurrent").filter(|v| *v > 0))
        .or_else(|| read_mem_total_bytes().ok())
        .ok_or_else(|| anyhow!("failed to determine base memory for set-property action"))?;
    run_set_property_action_with(cfg, logger, terminate, runner, snapshot, base)
}

fn run_action(
    cfg: &DaemonConfig,
    logger: &Logger,
    terminate: &Arc<AtomicBool>,
    runner: &mut dyn CommandRunner,
) -> Result<ActionOutcome> {
    match cfg.action {
        WatchdogAction::Panic => run_panic_action(cfg, logger, runner),
        WatchdogAction::SetProperty => run_set_property_action(cfg, logger, terminate, runner),
    }
}

fn load_config(path: &str) -> Result<DaemonConfig> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read config {}", path))?;
    let cfg: DaemonConfig =
        serde_yaml::from_str(&content).with_context(|| format!("invalid yaml in {}", path))?;
    validate_config(&cfg)?;
    Ok(cfg)
}

fn run_once(cfg: &DaemonConfig, logger: &Logger) -> Result<i32> {
    let snap = read_psi_snapshot()?;
    Ok(run_once_with_snapshot(cfg, logger, snap))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config(&cli.config)?;
    let logger = Logger::new(cfg.log_file.clone());
    let ledger_path = PathBuf::from(&cli.state_dir).join("daemon-ledger.jsonl");
    let terminate = Arc::new(AtomicBool::new(false));
    let mut runner = OsCommandRunner;
    let term_flag = Arc::clone(&terminate);
    ctrlc::set_handler(move || {
        term_flag.store(true, Ordering::Relaxed);
    })
    .context("failed to install signal handler")?;

    if cli.once {
        let code = run_once(&cfg, &logger)?;
        process::exit(code);
    }

    logger.log(
        "INFO",
        "daemon_start",
        &format!(
            "resguardd started action={:?} hold={}s cooldown={}s poll={}ms ledger={}",
            cfg.action,
            cfg.hold_seconds,
            cfg.cooldown_seconds,
            cfg.poll_interval_ms,
            ledger_path.display()
        ),
    );

    let mut state = WatchdogState::default();

    loop {
        if terminate.load(Ordering::Relaxed) {
            logger.log("INFO", "signal", "termination requested; exiting cleanly");
            break;
        }

        let snap = match read_psi_snapshot() {
            Ok(v) => v,
            Err(err) => {
                logger.log("WARN", "psi_read", &format!("psi read failed: {err}"));
                thread::sleep(Duration::from_millis(cfg.poll_interval_ms));
                continue;
            }
        };

        let now_sec = now_unix();
        if threshold_exceeded(&cfg, snap) && state.exceed_since_sec.is_none() {
            logger.log(
                "WARN",
                "threshold",
                &format!(
                    "threshold exceeded mem(avg10/avg60)={:.2}/{:.2} cpu(avg10/avg60)={:.2}/{:.2}",
                    snap.mem_avg10, snap.mem_avg60, snap.cpu_avg10, snap.cpu_avg60
                ),
            );
        }
        if should_trigger_watchdog(&cfg, &mut state, now_sec, snap) {
            let mut record = LedgerRecord {
                timestamp: now_unix(),
                mem_avg10: snap.mem_avg10,
                mem_avg60: snap.mem_avg60,
                cpu_avg10: snap.cpu_avg10,
                cpu_avg60: snap.cpu_avg60,
                action: match cfg.action {
                    WatchdogAction::Panic => "panic".to_string(),
                    WatchdogAction::SetProperty => "set-property".to_string(),
                },
                decision: "trigger".to_string(),
                revert_ok: None,
            };

            match run_action(&cfg, &logger, &terminate, &mut runner) {
                Ok(outcome) => {
                    record.action = outcome.action;
                    record.revert_ok = outcome.revert_ok;
                    logger.log("INFO", "action_done", "action completed");
                }
                Err(err) => {
                    record.decision = "action-failed".to_string();
                    logger.log("ERROR", "action_failed", &format!("action failed: {err}"));
                }
            }

            if let Err(err) = append_ledger_record(&ledger_path, &record) {
                logger.log(
                    "ERROR",
                    "ledger_write",
                    &format!("failed to append ledger: {err}"),
                );
            }

            logger.log(
                "INFO",
                "cooldown",
                &format!("enter cooldown {}s", cfg.cooldown_seconds),
            );
        }

        thread::sleep(Duration::from_millis(cfg.poll_interval_ms));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestRunner {
        calls: Vec<(String, Vec<String>)>,
        results: Vec<bool>,
    }

    impl TestRunner {
        fn with_results(results: Vec<bool>) -> Self {
            Self {
                calls: Vec::new(),
                results,
            }
        }
    }

    impl CommandRunner for TestRunner {
        fn run_command(&mut self, program: &str, args: &[String]) -> Result<bool> {
            self.calls.push((program.to_string(), args.to_vec()));
            if self.results.is_empty() {
                return Ok(true);
            }
            Ok(self.results.remove(0))
        }
    }

    fn cfg_for_tests() -> DaemonConfig {
        DaemonConfig {
            memory_avg10_threshold: 10.0,
            cpu_avg10_threshold: 10.0,
            hold_seconds: 2,
            cooldown_seconds: 5,
            action: WatchdogAction::SetProperty,
            action_duration_seconds: 1,
            poll_interval_ms: 200,
            log_file: None,
        }
    }

    fn over_snapshot() -> PsiSnapshot {
        PsiSnapshot {
            mem_avg10: 20.0,
            mem_avg60: 20.0,
            cpu_avg10: 20.0,
            cpu_avg60: 20.0,
        }
    }

    fn under_snapshot() -> PsiSnapshot {
        PsiSnapshot {
            mem_avg10: 1.0,
            mem_avg60: 1.0,
            cpu_avg10: 1.0,
            cpu_avg60: 1.0,
        }
    }

    #[test]
    fn once_snapshot_returns_expected_exit_code() {
        let cfg = cfg_for_tests();
        let logger = Logger::new(None);
        assert_eq!(run_once_with_snapshot(&cfg, &logger, under_snapshot()), 0);
        assert_eq!(run_once_with_snapshot(&cfg, &logger, over_snapshot()), 1);
    }

    #[test]
    fn watchdog_triggers_only_after_hold_seconds() {
        let cfg = cfg_for_tests();
        let mut state = WatchdogState::default();

        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            0,
            over_snapshot()
        ));
        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            1,
            over_snapshot()
        ));
        assert!(should_trigger_watchdog(
            &cfg,
            &mut state,
            2,
            over_snapshot()
        ));
    }

    #[test]
    fn watchdog_cooldown_blocks_repeated_trigger_until_expiry() {
        let cfg = cfg_for_tests();
        let mut state = WatchdogState::default();

        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            0,
            over_snapshot()
        ));
        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            1,
            over_snapshot()
        ));
        assert!(should_trigger_watchdog(
            &cfg,
            &mut state,
            2,
            over_snapshot()
        ));
        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            3,
            over_snapshot()
        ));
        assert!(should_trigger_watchdog(
            &cfg,
            &mut state,
            7,
            over_snapshot()
        ));
    }

    #[test]
    fn watchdog_resets_hold_when_pressure_drops() {
        let cfg = cfg_for_tests();
        let mut state = WatchdogState::default();

        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            0,
            over_snapshot()
        ));
        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            1,
            under_snapshot()
        ));
        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            2,
            over_snapshot()
        ));
        assert!(!should_trigger_watchdog(
            &cfg,
            &mut state,
            3,
            over_snapshot()
        ));
        assert!(should_trigger_watchdog(
            &cfg,
            &mut state,
            4,
            over_snapshot()
        ));
    }

    #[test]
    fn set_property_action_calls_revert_with_runner() {
        let cfg = cfg_for_tests();
        let logger = Logger::new(None);
        let terminate = Arc::new(AtomicBool::new(true));
        let snapshot = SetPropertySnapshot {
            before_high: "1G".to_string(),
            before_max: "2G".to_string(),
        };
        let mut runner = TestRunner::with_results(vec![true, true]);

        let out = run_set_property_action_with(
            &cfg,
            &logger,
            &terminate,
            &mut runner,
            snapshot,
            8 * 1024 * 1024 * 1024,
        )
        .expect("set-property action");

        assert_eq!(out.action, "set-property");
        assert_eq!(out.revert_ok, Some(true));
        assert_eq!(runner.calls.len(), 2);
        assert_eq!(runner.calls[0].0, "systemctl");
        assert_eq!(runner.calls[1].0, "systemctl");
        assert_eq!(runner.calls[0].1[0], "set-property");
        assert_eq!(runner.calls[1].1[0], "set-property");
        assert_eq!(runner.calls[1].1[2], "MemoryHigh=1G");
        assert_eq!(runner.calls[1].1[3], "MemoryMax=2G");
    }
}
