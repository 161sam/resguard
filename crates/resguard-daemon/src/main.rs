use anyhow::{anyhow, Context, Result};
use clap::Parser;
use resguard_system::{parse_prop_u64, read_mem_total_bytes, read_pressure, systemctl_show_props};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "resguardd", about = "Resguard freeze watchdog daemon")]
struct Cli {
    #[arg(long, default_value = "/etc/resguard/resguardd.yml")]
    config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WatchdogAction {
    Panic,
    SetProperty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

    fn log(&self, level: &str, msg: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("ts={ts} level={level} msg=\"{msg}\"");
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

fn run_panic_action(cfg: &DaemonConfig, logger: &Logger) -> Result<()> {
    let duration = format!("{}s", cfg.action_duration_seconds);
    logger.log(
        "WARN",
        &format!("trigger action=panic duration={duration} via `resguard panic`"),
    );
    let status = Command::new("resguard")
        .arg("panic")
        .arg("--duration")
        .arg(&duration)
        .status()
        .context("failed to execute resguard panic")?;
    if !status.success() {
        return Err(anyhow!("resguard panic failed with status {}", status));
    }
    Ok(())
}

fn run_set_property_action(cfg: &DaemonConfig, logger: &Logger) -> Result<()> {
    let props = systemctl_show_props(false, "user.slice", &["MemoryMax", "MemoryCurrent"])?;
    let before_max = props
        .get("MemoryMax")
        .cloned()
        .unwrap_or_else(|| "infinity".to_string());
    let before_high = systemctl_show_props(false, "user.slice", &["MemoryHigh"])?
        .get("MemoryHigh")
        .cloned()
        .unwrap_or_else(|| "infinity".to_string());

    let base = parse_prop_u64(&props, "MemoryMax")
        .filter(|v| *v > 0)
        .or_else(|| parse_prop_u64(&props, "MemoryCurrent").filter(|v| *v > 0))
        .or_else(|| read_mem_total_bytes().ok())
        .ok_or_else(|| anyhow!("failed to determine base memory for set-property action"))?;

    let target_high = (base as f64 * 0.5) as u64;
    let target_max = (base as f64 * 0.6) as u64;
    logger.log(
        "WARN",
        &format!(
            "trigger action=set-property target_high={} target_max={}",
            target_high, target_max
        ),
    );

    let status = Command::new("systemctl")
        .arg("set-property")
        .arg("user.slice")
        .arg(format!("MemoryHigh={target_high}"))
        .arg(format!("MemoryMax={target_max}"))
        .status()
        .context("failed to execute systemctl set-property")?;
    if !status.success() {
        return Err(anyhow!(
            "systemctl set-property failed with status {}",
            status
        ));
    }

    thread::sleep(Duration::from_secs(cfg.action_duration_seconds));
    let revert = Command::new("systemctl")
        .arg("set-property")
        .arg("user.slice")
        .arg(format!("MemoryHigh={before_high}"))
        .arg(format!("MemoryMax={before_max}"))
        .status()
        .context("failed to revert systemctl set-property")?;
    if !revert.success() {
        return Err(anyhow!("set-property revert failed with status {}", revert));
    }
    logger.log("INFO", "set-property action reverted");
    Ok(())
}

fn run_action(cfg: &DaemonConfig, logger: &Logger) -> Result<()> {
    match cfg.action {
        WatchdogAction::Panic => run_panic_action(cfg, logger),
        WatchdogAction::SetProperty => run_set_property_action(cfg, logger),
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config(&cli.config)?;
    let logger = Logger::new(cfg.log_file.clone());
    logger.log(
        "INFO",
        &format!(
            "resguardd started action={:?} hold={}s cooldown={}s poll={}ms",
            cfg.action, cfg.hold_seconds, cfg.cooldown_seconds, cfg.poll_interval_ms
        ),
    );

    let mut exceed_since: Option<Instant> = None;
    let mut cooldown_until: Option<Instant> = None;

    loop {
        let snap = match read_psi_snapshot() {
            Ok(v) => v,
            Err(err) => {
                logger.log("WARN", &format!("psi read failed: {err}"));
                thread::sleep(Duration::from_millis(cfg.poll_interval_ms));
                continue;
            }
        };

        let over = threshold_exceeded(&cfg, snap);
        if over {
            if exceed_since.is_none() {
                exceed_since = Some(Instant::now());
                logger.log(
                    "WARN",
                    &format!(
                        "threshold exceeded mem(avg10/avg60)={:.2}/{:.2} cpu(avg10/avg60)={:.2}/{:.2}",
                        snap.mem_avg10, snap.mem_avg60, snap.cpu_avg10, snap.cpu_avg60
                    ),
                );
            }
        } else {
            exceed_since = None;
        }

        let in_cooldown = cooldown_until.map(|t| Instant::now() < t).unwrap_or(false);
        if let Some(since) = exceed_since {
            if since.elapsed() >= Duration::from_secs(cfg.hold_seconds) && !in_cooldown {
                if let Err(err) = run_action(&cfg, &logger) {
                    logger.log("ERROR", &format!("action failed: {err}"));
                } else {
                    logger.log("INFO", "action completed");
                }
                cooldown_until = Some(Instant::now() + Duration::from_secs(cfg.cooldown_seconds));
                exceed_since = None;
                logger.log("INFO", &format!("enter cooldown {}s", cfg.cooldown_seconds));
            }
        }

        thread::sleep(Duration::from_millis(cfg.poll_interval_ms));
    }
}
