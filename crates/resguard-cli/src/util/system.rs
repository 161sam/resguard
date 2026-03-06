use crate::*;

pub(crate) fn partial_exit_code(partial: bool) -> i32 {
    if partial {
        1
    } else {
        0
    }
}

pub(crate) fn check_command_success(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub(crate) fn read_meminfo_kb(field: &str) -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(field) {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let kb = parts.first()?.parse::<u64>().ok()?;
            return Some(kb);
        }
    }
    None
}

pub(crate) fn format_bytes_human(bytes: u64) -> String {
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

pub(crate) fn parse_u64_prop(props: &BTreeMap<String, String>, key: &str) -> Option<u64> {
    props.get(key).and_then(|v| v.parse::<u64>().ok())
}

pub(crate) fn list_system_slices() -> Vec<String> {
    let out = Command::new("systemctl")
        .args([
            "list-units",
            "--type=slice",
            "--all",
            "--no-legend",
            "--no-pager",
        ])
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut units = Vec::new();
    for line in text.lines() {
        if let Some(unit) = line.split_whitespace().next() {
            if unit.ends_with(".slice") {
                units.push(unit.to_string());
            }
        }
    }
    units
}
