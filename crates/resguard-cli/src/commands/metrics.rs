use crate::*;

pub(crate) fn handle_metrics() -> Result<i32> {
    println!("command=metrics");
    let mut partial = false;

    let cpu_p = read_pressure_1min("/proc/pressure/cpu").ok().flatten();
    let mem_p = read_pressure_1min("/proc/pressure/memory").ok().flatten();
    let io_p = read_pressure_1min("/proc/pressure/io").ok().flatten();

    println!("CPU pressure");
    match cpu_p {
        Some(v) => println!("avg60={:.2}", v),
        None => {
            println!("avg60=-");
            partial = true;
        }
    }
    println!("Memory pressure");
    match mem_p {
        Some(v) => println!("avg60={:.2}", v),
        None => {
            println!("avg60=-");
            partial = true;
        }
    }
    println!("IO pressure");
    match io_p {
        Some(v) => println!("avg60={:.2}", v),
        None => {
            println!("avg60=-");
            partial = true;
        }
    }
    println!();

    println!("System memory");
    let total = read_meminfo_kb("MemTotal:");
    let available = read_meminfo_kb("MemAvailable:");
    match (total, available) {
        (Some(t), Some(a)) => {
            println!("total={}", format_bytes_human(t * 1024));
            println!("available={}", format_bytes_human(a * 1024));
            println!("used={}", format_bytes_human((t.saturating_sub(a)) * 1024));
        }
        _ => {
            println!("total=-");
            println!("available=-");
            partial = true;
        }
    }
    println!();

    let keys = [
        "MemoryCurrent",
        "MemoryPeak",
        "MemoryLow",
        "MemoryHigh",
        "MemoryMax",
    ];
    println!("User slice usage");
    match systemctl_show_props(false, "user.slice", &keys) {
        Ok(props) => {
            let current = parse_u64_prop(&props, "MemoryCurrent").unwrap_or(0);
            let max = status_value(&props, "MemoryMax");
            let high = status_value(&props, "MemoryHigh");
            println!("user.slice MemoryCurrent: {}", format_bytes_human(current));
            println!("user.slice MemoryHigh: {}", high);
            println!("user.slice MemoryMax: {}", max);
        }
        Err(err) => {
            println!("user.slice: unavailable ({})", err);
            partial = true;
        }
    }
    println!();

    println!("Top slices");
    let mut slice_usage: Vec<(String, u64)> = Vec::new();
    for unit in list_system_slices() {
        if let Ok(props) = systemctl_show_props(false, &unit, &["MemoryCurrent"]) {
            if let Some(cur) = parse_u64_prop(&props, "MemoryCurrent") {
                slice_usage.push((unit, cur));
            }
        }
    }
    if slice_usage.is_empty() {
        println!("unavailable");
        partial = true;
    } else {
        slice_usage.sort_by(|a, b| b.1.cmp(&a.1));
        for (unit, cur) in slice_usage.into_iter().take(5) {
            println!("{} {}", unit, format_bytes_human(cur));
        }
    }

    Ok(partial_exit_code(partial))
}
