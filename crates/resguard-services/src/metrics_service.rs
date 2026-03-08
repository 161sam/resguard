use anyhow::Result;
use resguard_runtime::{
    parse_prop_u64, read_meminfo_kb, read_pressure_1min, systemctl_list_units, systemctl_show_props,
};

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

fn status_value(props: &std::collections::BTreeMap<String, String>, key: &str) -> String {
    props
        .get(key)
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

fn pressure_line(name: &str, value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{name}_avg60={v:.2}"),
        None => format!("{name}_avg60=-"),
    }
}

fn top_slice_line(rank: usize, unit: &str, cur: u64) -> String {
    format!(
        "slice_rank={rank} unit={unit} memory_current={} memory_human={}",
        cur,
        format_bytes_human(cur)
    )
}

pub fn metrics() -> Result<i32> {
    println!("command=metrics");
    let mut partial = false;

    let cpu_p = read_pressure_1min("/proc/pressure/cpu").ok().flatten();
    let mem_p = read_pressure_1min("/proc/pressure/memory").ok().flatten();
    let io_p = read_pressure_1min("/proc/pressure/io").ok().flatten();

    println!("== Metrics: Pressure ==");
    println!("{}", pressure_line("cpu_pressure", cpu_p));
    println!("{}", pressure_line("memory_pressure", mem_p));
    println!("{}", pressure_line("io_pressure", io_p));
    if cpu_p.is_none() || mem_p.is_none() || io_p.is_none() {
        partial = true;
    }
    println!("\n== Metrics: Memory ==");

    let total = read_meminfo_kb("MemTotal:");
    let available = read_meminfo_kb("MemAvailable:");
    match (total, available) {
        (Some(t), Some(a)) => {
            let total_b = t * 1024;
            let available_b = a * 1024;
            let used_b = (t.saturating_sub(a)) * 1024;
            println!("mem_total_bytes={total_b}");
            println!("mem_available_bytes={available_b}");
            println!("mem_used_bytes={used_b}");
            println!("mem_total_human={}", format_bytes_human(total_b));
            println!("mem_available_human={}", format_bytes_human(available_b));
            println!("mem_used_human={}", format_bytes_human(used_b));
        }
        _ => {
            println!("mem_total_bytes=-");
            println!("mem_available_bytes=-");
            partial = true;
        }
    }
    println!("\n== Metrics: user.slice ==");

    let keys = [
        "MemoryCurrent",
        "MemoryPeak",
        "MemoryLow",
        "MemoryHigh",
        "MemoryMax",
    ];
    match systemctl_show_props(false, "user.slice", &keys) {
        Ok(props) => {
            let current = parse_prop_u64(&props, "MemoryCurrent").unwrap_or(0);
            let max = status_value(&props, "MemoryMax");
            let high = status_value(&props, "MemoryHigh");
            println!("user_slice_memory_current={current}");
            println!(
                "user_slice_memory_current_human={}",
                format_bytes_human(current)
            );
            println!("user_slice_memory_high={high}");
            println!("user_slice_memory_max={max}");
        }
        Err(err) => {
            println!("user_slice=unavailable ({err})");
            partial = true;
        }
    }
    println!("\n== Metrics: Top Slices ==");

    let mut slice_usage: Vec<(String, u64)> = Vec::new();
    for unit in systemctl_list_units(false, "slice").unwrap_or_default() {
        if let Ok(props) = systemctl_show_props(false, &unit, &["MemoryCurrent"]) {
            if let Some(cur) = parse_prop_u64(&props, "MemoryCurrent") {
                slice_usage.push((unit, cur));
            }
        }
    }
    if slice_usage.is_empty() {
        println!("top_slices=unavailable");
        partial = true;
    } else {
        slice_usage.sort_by(|a, b| b.1.cmp(&a.1));
        for (i, (unit, cur)) in slice_usage.into_iter().take(5).enumerate() {
            println!("{}", top_slice_line(i + 1, &unit, cur));
        }
    }

    Ok(if partial { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::{pressure_line, top_slice_line};

    #[test]
    fn pressure_line_is_stable_for_scripts() {
        assert_eq!(
            pressure_line("cpu_pressure", Some(1.234)),
            "cpu_pressure_avg60=1.23"
        );
        assert_eq!(pressure_line("cpu_pressure", None), "cpu_pressure_avg60=-");
    }

    #[test]
    fn top_slice_line_contains_raw_and_human_values() {
        let line = top_slice_line(1, "user.slice", 1_073_741_824);
        assert!(line.contains("slice_rank=1"));
        assert!(line.contains("unit=user.slice"));
        assert!(line.contains("memory_current=1073741824"));
        assert!(line.contains("memory_human=1G"));
    }
}
