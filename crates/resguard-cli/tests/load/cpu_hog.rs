use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

pub fn start_cpu_hog(threads: usize) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    for _ in 0..threads.max(1) {
        let stop_flag = Arc::clone(&stop);
        thread::spawn(move || {
            let mut x: u64 = 1;
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                std::hint::black_box(x);
            }
        });
    }
    stop
}
