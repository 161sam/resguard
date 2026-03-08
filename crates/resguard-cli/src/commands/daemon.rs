use crate::*;
use resguard_services::daemon_service::{daemon_disable, daemon_enable, daemon_status};

pub(crate) fn handle_daemon_enable() -> Result<i32> {
    daemon_enable()
}

pub(crate) fn handle_daemon_disable() -> Result<i32> {
    daemon_disable()
}

pub(crate) fn handle_daemon_status() -> Result<i32> {
    daemon_status()
}
