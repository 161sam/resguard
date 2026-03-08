use anyhow::Result;
use resguard_runtime::{check_command_success, systemctl_service_action};

pub fn daemon_enable() -> Result<i32> {
    println!("command=daemon enable");
    if systemctl_service_action("enable", "resguardd.service")? {
        println!("result=ok action=enable service=resguardd.service");
        Ok(0)
    } else {
        eprintln!("systemctl enable resguardd.service failed");
        Ok(1)
    }
}

pub fn daemon_disable() -> Result<i32> {
    println!("command=daemon disable");
    if systemctl_service_action("disable", "resguardd.service")? {
        println!("result=ok action=disable service=resguardd.service");
        Ok(0)
    } else {
        eprintln!("systemctl disable resguardd.service failed");
        Ok(1)
    }
}

pub fn daemon_status() -> Result<i32> {
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
