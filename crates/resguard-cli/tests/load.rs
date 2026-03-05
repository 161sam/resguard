#![allow(clippy::needless_pass_by_value)]

#[path = "load/cpu_hog.rs"]
mod cpu_hog;
#[path = "load/fork_bomb_sim.rs"]
mod fork_bomb_sim;
#[path = "load/memory_hog.rs"]
mod memory_hog;

use std::process::Command;
use std::thread;
use std::time::Duration;

fn resguard_help_ok() -> bool {
    let bin = env!("CARGO_BIN_EXE_resguard");
    Command::new(bin)
        .arg("--help")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
#[ignore = "load test: cpu pressure"]
fn cpu_pressure_responsiveness() {
    let stop = cpu_hog::start_cpu_hog(4);
    thread::sleep(Duration::from_millis(500));
    assert!(
        resguard_help_ok(),
        "resguard --help should remain responsive"
    );
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[test]
#[ignore = "load test: memory pressure"]
fn memory_pressure_responsiveness() {
    let handle = memory_hog::start_memory_hog(256 * 1024 * 1024, Duration::from_secs(3));
    thread::sleep(Duration::from_millis(500));
    assert!(
        resguard_help_ok(),
        "resguard --help should remain responsive"
    );
    handle.join().expect("memory hog thread should complete");
}

#[test]
#[ignore = "load test: process churn simulation"]
fn fork_bomb_sim_responsiveness() {
    let handle = fork_bomb_sim::start_process_churn(Duration::from_secs(3), 128);
    thread::sleep(Duration::from_millis(300));
    assert!(
        resguard_help_ok(),
        "resguard --help should remain responsive"
    );
    handle.join().expect("process churn thread should complete");
}
