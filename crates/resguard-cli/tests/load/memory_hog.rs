use std::thread;
use std::time::Duration;

pub fn start_memory_hog(target_bytes: usize, hold_for: Duration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let chunk = 8 * 1024 * 1024;
        let mut buffers: Vec<Vec<u8>> = Vec::new();
        let mut allocated = 0usize;

        while allocated < target_bytes {
            let mut v = vec![0u8; chunk.min(target_bytes.saturating_sub(allocated))];
            for b in v.iter_mut().step_by(4096) {
                *b = 1;
            }
            allocated += v.len();
            buffers.push(v);
            thread::sleep(Duration::from_millis(10));
        }

        thread::sleep(hold_for);
        drop(buffers);
    })
}
