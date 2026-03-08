use crate::systemd::{daemon_reload_if_root, systemctl_set_slice_memory_limits};
use anyhow::Result;

pub fn rollback_slice_memory_limits(
    slice: &str,
    memory_high: &str,
    memory_max: &str,
) -> Result<()> {
    systemctl_set_slice_memory_limits(slice, memory_high, memory_max)
}

pub fn rollback_apply_reload(root: &str) -> Result<()> {
    daemon_reload_if_root(root)
}
