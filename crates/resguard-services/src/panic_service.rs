use anyhow::{anyhow, Result};
use resguard_runtime::{
    is_root_user, parse_prop_u64, read_meminfo_kb, systemctl_set_slice_memory_limits,
    systemctl_show_props,
};
use std::time::Duration;

fn format_bytes_human(bytes: u64) -> String {
    let gb = 1024_u64.pow(3);
    let mb = 1024_u64.pow(2);
    if bytes >= gb {
        format!("{}G", bytes / gb)
    } else if bytes >= mb {
        format!("{}M", bytes / mb)
    } else {
        format!("{}B", bytes)
    }
}

fn parse_duration_arg(input: &str) -> Result<Duration> {
    let s = input.trim();
    if s.is_empty() {
        return Err(anyhow!("duration must not be empty"));
    }

    let split_at = s
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());

    let (num_s, unit_s) = s.split_at(split_at);
    let n: u64 = num_s
        .parse()
        .map_err(|_| anyhow!("invalid duration value: {}", num_s))?;

    let secs = match unit_s {
        "" | "s" => n,
        "m" => n.saturating_mul(60),
        "h" => n.saturating_mul(60 * 60),
        _ => return Err(anyhow!("invalid duration unit '{}', use s/m/h", unit_s)),
    };
    Ok(Duration::from_secs(secs))
}

pub fn panic_mode(root: &str, duration: Option<String>) -> Result<i32> {
    println!("command=panic");
    if root != "/" {
        return Err(anyhow!("panic mode requires --root /"));
    }
    if !is_root_user()? {
        return Ok(3);
    }

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
        .or_else(|| read_meminfo_kb("MemTotal:").map(|kb| kb * 1024))
        .ok_or_else(|| anyhow!("failed to resolve base memory for panic mode"))?;

    let target_high = (base as f64 * 0.5) as u64;
    let target_max = (base as f64 * 0.6) as u64;

    systemctl_set_slice_memory_limits(
        "user.slice",
        &target_high.to_string(),
        &target_max.to_string(),
    )?;

    println!(
        "panic_applied user.slice MemoryHigh={} MemoryMax={}",
        format_bytes_human(target_high),
        format_bytes_human(target_max)
    );

    if let Some(d) = duration {
        let wait = parse_duration_arg(&d)?;
        println!("panic_duration={}s", wait.as_secs());
        std::thread::sleep(wait);

        systemctl_set_slice_memory_limits("user.slice", &before_high, &before_max)?;
        println!("panic_reverted");
    } else {
        println!("hint=to revert manually run: sudo systemctl revert user.slice");
    }

    Ok(0)
}
