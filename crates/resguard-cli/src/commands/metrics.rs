use crate::*;

pub(crate) fn handle_metrics() -> Result<i32> {
    resguard_services::metrics_service::metrics()
}

pub(crate) fn run() -> Result<i32> {
    handle_metrics()
}
