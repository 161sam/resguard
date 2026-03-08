use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub fn parse_prop_u64(props: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    props.get(key).and_then(|v| v.parse::<u64>().ok())
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
    let content = std::fs::read_to_string("/proc/self/status")?;
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
