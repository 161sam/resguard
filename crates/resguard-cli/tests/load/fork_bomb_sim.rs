use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub fn start_process_churn(duration: Duration, max_spawns: usize) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let start = Instant::now();
        let mut spawned = 0usize;

        while start.elapsed() < duration && spawned < max_spawns {
            let _ = Command::new("true").status();
            spawned += 1;
            thread::sleep(Duration::from_millis(10));
        }
    })
}
