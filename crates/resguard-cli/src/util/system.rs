use crate::*;

pub(crate) fn partial_exit_code(partial: bool) -> i32 {
    if partial {
        1
    } else {
        0
    }
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
    resguard_runtime::systemctl_list_units(false, "slice")
        .unwrap_or_default()
        .into_iter()
        .filter(|unit| unit.ends_with(".slice"))
        .collect()
}
