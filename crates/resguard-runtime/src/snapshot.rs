use crate::meminfo::{read_mem_available_bytes, read_mem_total_bytes};
use crate::pressure::read_pressure;
use anyhow::Result;
use resguard_model::PressureSnapshot;

#[derive(Debug, Clone, Default)]
pub struct SystemSnapshot {
    pub cpu_pressure: Option<PressureSnapshot>,
    pub memory_pressure: Option<PressureSnapshot>,
    pub io_pressure: Option<PressureSnapshot>,
    pub mem_total_bytes: Option<u64>,
    pub mem_available_bytes: Option<u64>,
}

pub fn read_system_snapshot() -> SystemSnapshot {
    SystemSnapshot {
        cpu_pressure: read_pressure("/proc/pressure/cpu").ok().flatten(),
        memory_pressure: read_pressure("/proc/pressure/memory").ok().flatten(),
        io_pressure: read_pressure("/proc/pressure/io").ok().flatten(),
        mem_total_bytes: read_mem_total_bytes().ok(),
        mem_available_bytes: read_mem_available_bytes().ok(),
    }
}

pub fn read_pressure_snapshot() -> Result<Option<PressureSnapshot>> {
    read_pressure("/proc/pressure/memory")
}
