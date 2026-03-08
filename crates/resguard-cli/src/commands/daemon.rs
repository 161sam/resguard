use crate::*;

fn run_systemctl_service_action(action: &str, service: &str) -> Result<i32> {
    if systemctl_service_action(action, service)? {
        println!("result=ok action={} service={}", action, service);
        Ok(0)
    } else {
        eprintln!("systemctl {} {} failed", action, service);
        Ok(1)
    }
}

pub(crate) fn handle_daemon_enable() -> Result<i32> {
    println!("command=daemon enable");
    run_systemctl_service_action("enable", "resguardd.service")
}

pub(crate) fn handle_daemon_disable() -> Result<i32> {
    println!("command=daemon disable");
    run_systemctl_service_action("disable", "resguardd.service")
}

pub(crate) fn handle_daemon_status() -> Result<i32> {
    println!("command=daemon status");
    let enabled = check_command_success("systemctl", &["is-enabled", "resguardd.service"]);
    let active = check_command_success("systemctl", &["is-active", "resguardd.service"]);
    println!("resguardd.enabled={}", enabled);
    println!("resguardd.active={}", active);
    if !enabled {
        println!("fix: sudo systemctl enable resguardd.service");
    }
    if !active {
        println!("fix: sudo systemctl start resguardd.service");
    }
    Ok(if enabled && active { 0 } else { 1 })
}
