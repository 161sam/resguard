use anyhow::{anyhow, Result};

pub fn read_meminfo_kb(field: &str) -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    parse_meminfo_field_kb(&content, field)
}

pub fn parse_meminfo_field_kb(content: &str, field: &str) -> Option<u64> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let kb = parts.first()?.parse::<u64>().ok()?;
            return Some(kb);
        }
    }
    None
}

pub fn read_mem_available_bytes() -> Result<u64> {
    read_meminfo_kb("MemAvailable:")
        .and_then(|kb| kb.checked_mul(1024))
        .ok_or_else(|| anyhow!("MemAvailable not found in /proc/meminfo"))
}

pub fn read_mem_total_bytes() -> Result<u64> {
    read_meminfo_kb("MemTotal:")
        .and_then(|kb| kb.checked_mul(1024))
        .ok_or_else(|| anyhow!("MemTotal not found in /proc/meminfo"))
}

#[cfg(test)]
mod tests {
    use super::parse_meminfo_field_kb;

    #[test]
    fn parse_meminfo_field() {
        let content = "MemTotal:       16384 kB\nMemAvailable:   8192 kB\n";
        assert_eq!(parse_meminfo_field_kb(content, "MemTotal:"), Some(16384));
        assert_eq!(parse_meminfo_field_kb(content, "MemAvailable:"), Some(8192));
        assert_eq!(parse_meminfo_field_kb(content, "MemFree:"), None);
    }
}
