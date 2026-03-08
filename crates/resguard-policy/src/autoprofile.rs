use resguard_model::{Class, Cpu, Memory, Metadata, Oomd, Profile, Spec, SystemMemory, UserMemory};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct AutoProfileSnapshot {
    pub total_mem_bytes: u64,
    pub cpu_cores: u32,
}

fn format_bytes_binary(bytes: u64) -> String {
    let gb = 1024_u64.pow(3);
    let mb = 1024_u64.pow(2);
    if bytes >= gb {
        format!("{}G", bytes / gb)
    } else if bytes >= mb {
        format!("{}M", bytes / mb)
    } else {
        bytes.to_string()
    }
}

fn default_reserve_bytes(total: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    match total / gb {
        0..=9 => gb,
        10..=19 => 2 * gb,
        20..=39 => 4 * gb,
        _ => 6 * gb,
    }
}

fn clamp(value: u64, min: u64, max: u64) -> u64 {
    value.max(min).min(max)
}

fn round_down_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    (value / step) * step
}

fn round_up_to_step(value: u64, step: u64) -> u64 {
    if step == 0 {
        return value;
    }
    value.div_ceil(step) * step
}

fn reserve_rounding_step(reserve: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    let mib_256 = 256 * 1024_u64.pow(2);
    if reserve >= 2 * gb {
        gb
    } else {
        mib_256
    }
}

fn class_cap_percent(user_max: u64, pct: u64, hard_cap: u64) -> u64 {
    let gb = 1024_u64.pow(3);
    let raw = user_max.saturating_mul(pct) / 100;
    let rounded = round_up_to_step(raw, gb);
    clamp(rounded, gb.min(user_max), hard_cap.min(user_max))
}

pub fn build_auto_profile(name: &str, snapshot: AutoProfileSnapshot) -> Profile {
    let gb = 1024_u64.pow(3);
    let total_mem_bytes = snapshot.total_mem_bytes;
    let cpu_cores = snapshot.cpu_cores;

    let base_reserve = default_reserve_bytes(total_mem_bytes).min(total_mem_bytes);
    let reserve_step = reserve_rounding_step(base_reserve);
    let reserve = round_up_to_step(base_reserve, reserve_step).min(total_mem_bytes);

    let mut user_max = round_down_to_step(total_mem_bytes.saturating_sub(reserve), gb);
    if user_max == 0 {
        user_max = round_down_to_step(total_mem_bytes, 256 * 1024_u64.pow(2));
    }

    let high_margin = (user_max / 10).min(2 * gb);
    let mut user_high = round_down_to_step(user_max.saturating_sub(high_margin), gb);
    if user_high == 0 {
        user_high = user_max;
    }

    let (cpu, oomd) = if cpu_cores >= 4 {
        (
            Some(Cpu {
                enabled: Some(true),
                reserve_core_for_system: Some(true),
                system_allowed_cpus: Some("0".to_string()),
                user_allowed_cpus: Some(format!("1-{}", cpu_cores - 1)),
            }),
            Some(Oomd {
                enabled: Some(true),
                memory_pressure: Some("kill".to_string()),
                memory_pressure_limit: Some("60%".to_string()),
            }),
        )
    } else {
        (
            Some(Cpu {
                enabled: Some(false),
                reserve_core_for_system: Some(false),
                system_allowed_cpus: None,
                user_allowed_cpus: None,
            }),
            Some(Oomd {
                enabled: Some(true),
                memory_pressure: Some("kill".to_string()),
                memory_pressure_limit: Some("60%".to_string()),
            }),
        )
    };

    let browsers_max = class_cap_percent(user_max, 40, 6 * gb);
    let ide_max = class_cap_percent(user_max, 25, 4 * gb);
    let heavy_rest = user_max.saturating_sub(browsers_max.saturating_add(ide_max));
    let heavy_max = if heavy_rest == 0 {
        gb.min(user_max)
    } else {
        let rounded = round_down_to_step(heavy_rest, gb);
        if rounded == 0 {
            round_down_to_step(heavy_rest, 256 * 1024_u64.pow(2))
        } else {
            rounded
        }
    }
    .min(8 * gb)
    .min(user_max);

    let mut classes = BTreeMap::new();
    classes.insert(
        "browsers".to_string(),
        Class {
            slice_name: Some("resguard-browsers.slice".to_string()),
            memory_high: None,
            memory_max: Some(format_bytes_binary(browsers_max)),
            cpu_weight: Some(80),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("55%".to_string()),
        },
    );
    classes.insert(
        "ide".to_string(),
        Class {
            slice_name: Some("resguard-ide.slice".to_string()),
            memory_high: None,
            memory_max: Some(format_bytes_binary(ide_max)),
            cpu_weight: Some(70),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("60%".to_string()),
        },
    );
    classes.insert(
        "heavy".to_string(),
        Class {
            slice_name: Some("resguard-heavy.slice".to_string()),
            memory_high: None,
            memory_max: Some(format_bytes_binary(heavy_max)),
            cpu_weight: Some(90),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("50%".to_string()),
        },
    );
    classes.insert(
        "rescue".to_string(),
        Class {
            slice_name: Some("resguard-rescue.slice".to_string()),
            memory_high: None,
            memory_max: Some("1G".to_string()),
            cpu_weight: Some(100),
            oomd_memory_pressure: Some("kill".to_string()),
            oomd_memory_pressure_limit: Some("70%".to_string()),
        },
    );

    Profile {
        api_version: "resguard.io/v1".to_string(),
        kind: "Profile".to_string(),
        metadata: Metadata {
            name: name.to_string(),
        },
        spec: Spec {
            memory: Some(Memory {
                system: Some(SystemMemory {
                    memory_low: Some(format_bytes_binary(reserve)),
                }),
                user: Some(UserMemory {
                    memory_high: Some(format_bytes_binary(user_high)),
                    memory_max: Some(format_bytes_binary(user_max)),
                }),
            }),
            cpu,
            oomd,
            classes,
            slices: None,
            suggest: None,
        },
    }
}
