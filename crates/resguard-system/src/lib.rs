use anyhow::{anyhow, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn daemon_reload() -> Result<()> {
    Command::new("systemctl").arg("daemon-reload").status()?;

    Ok(())
}

pub fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

pub fn exec_command(
    program: &str,
    args: &[String],
    envs: &std::collections::BTreeMap<String, String>,
) -> Result<std::process::ExitStatus> {
    let status = Command::new(program).args(args).envs(envs).status()?;
    Ok(status)
}

pub fn systemctl_cat_unit(user: bool, unit: &str) -> Result<bool> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    let status = cmd.arg("cat").arg(unit).status()?;
    Ok(status.success())
}

pub fn systemd_run(user: bool, slice: &str, wait: bool, cmd: &[String]) -> Result<i32> {
    let mut c = Command::new("systemd-run");
    if user {
        c.arg("--user");
    }
    c.arg("--scope");
    if wait {
        c.arg("--wait");
    }
    c.arg("-p").arg(format!("Slice={slice}"));
    c.arg("--");
    for a in cmd {
        c.arg(a);
    }
    let status = c.status()?;
    Ok(status.code().unwrap_or(1))
}

pub fn systemctl_show_props(
    user: bool,
    unit: &str,
    keys: &[&str],
) -> Result<BTreeMap<String, String>> {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }

    cmd.arg("show").arg(unit);
    for key in keys {
        cmd.arg("-p").arg(key);
    }

    let out = cmd.output()?;
    if !out.status.success() {
        return Err(anyhow!(
            "systemctl show failed for {} (user={}): status={}",
            unit,
            user,
            out.status
        ));
    }

    let mut map = BTreeMap::new();
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(map)
}

pub fn systemctl_is_active(unit: &str) -> Result<bool> {
    let status = Command::new("systemctl")
        .arg("is-active")
        .arg(unit)
        .status()?;
    Ok(status.success())
}

pub fn read_pressure_1min(path: &str) -> Result<Option<f64>> {
    let s = fs::read_to_string(path)?;
    for line in s.lines() {
        if line.starts_with("some ") {
            for tok in line.split_whitespace() {
                if let Some(v) = tok.strip_prefix("avg60=") {
                    return Ok(Some(v.parse()?));
                }
            }
        }
    }
    Ok(None)
}

pub fn read_mem_total_bytes() -> Result<u64> {
    let content = fs::read_to_string("/proc/meminfo")?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let kb: u64 = parts
                .first()
                .ok_or_else(|| anyhow!("MemTotal parse failed"))?
                .parse()?;
            return kb
                .checked_mul(1024)
                .ok_or_else(|| anyhow!("MemTotal overflow"));
        }
    }

    Err(anyhow!("MemTotal not found in /proc/meminfo"))
}

pub fn cpu_count() -> Result<u32> {
    let n = std::thread::available_parallelism()?;
    Ok(n.get() as u32)
}

pub fn default_reserve_bytes(total: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    match total / gb {
        0..=9 => gb,
        10..=19 => 2 * gb,
        20..=39 => 4 * gb,
        _ => 6 * gb,
    }
}

pub fn is_root_user() -> Result<bool> {
    let content = fs::read_to_string("/proc/self/status")?;
    let uid_line = content
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| anyhow!("Uid line not found in /proc/self/status"))?;
    let first_uid = uid_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("Uid parse failed"))?;
    Ok(first_uid == "0")
}

#[cfg(test)]
mod tests {
    use super::default_reserve_bytes;

    #[test]
    fn reserve_heuristic_matches_expected_buckets() {
        let gb = 1024_u64.pow(3);
        assert_eq!(default_reserve_bytes(8 * gb), gb);
        assert_eq!(default_reserve_bytes(16 * gb), 2 * gb);
        assert_eq!(default_reserve_bytes(32 * gb), 4 * gb);
        assert_eq!(default_reserve_bytes(64 * gb), 6 * gb);
    }
}
