//! Cooperative shutdown signal shared by every long-running thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Shutdown {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    flag: AtomicBool,
    lock: Mutex<()>,
    cv: Condvar,
}

static CTRL_C: AtomicBool = AtomicBool::new(false);

impl Shutdown {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger(&self) {
        self.inner.flag.store(true, Ordering::SeqCst);
        let _g = crate::lock(&self.inner.lock);
        self.inner.cv.notify_all();
    }

    pub fn is_triggered(&self) -> bool {
        self.inner.flag.load(Ordering::SeqCst)
    }

    /// Sleep up to `d`, returning early (with `true`) when shutdown is triggered.
    pub fn sleep(&self, d: Duration) -> bool {
        if self.is_triggered() {
            return true;
        }
        let g = crate::lock(&self.inner.lock);
        let _ = self.inner.cv.wait_timeout_while(g, d, |_| !self.is_triggered());
        self.is_triggered()
    }

    /// Block until triggered.
    pub fn wait(&self) {
        while !self.sleep(Duration::from_millis(250)) {}
    }

    /// Trigger this shutdown when the process receives SIGINT/SIGTERM (unix only; no-op elsewhere).
    pub fn trigger_on_ctrl_c(&self) {
        install_handlers();
        let me = self.clone();
        std::thread::Builder::new()
            .name("ctrl-c".into())
            .spawn(move || {
                while !me.is_triggered() {
                    if CTRL_C.load(Ordering::SeqCst) {
                        me.trigger();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
            .ok();
    }
}

#[cfg(unix)]
fn install_handlers() {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    extern "C" fn on_signal(_sig: i32) {
        CTRL_C.store(true, Ordering::SeqCst);
    }
    unsafe {
        signal(2, on_signal as extern "C" fn(i32) as usize); // SIGINT
        signal(15, on_signal as extern "C" fn(i32) as usize); // SIGTERM
    }
}

#[cfg(not(unix))]
fn install_handlers() {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn trigger_wakes_sleepers() {
        let s = Shutdown::new();
        let s2 = s.clone();
        let t = std::thread::spawn(move || {
            let start = Instant::now();
            assert!(s2.sleep(Duration::from_secs(10)));
            start.elapsed()
        });
        std::thread::sleep(Duration::from_millis(50));
        s.trigger();
        assert!(t.join().unwrap() < Duration::from_secs(5));
        assert!(s.is_triggered());
    }

    #[test]
    fn sleep_times_out() {
        let s = Shutdown::new();
        assert!(!s.sleep(Duration::from_millis(20)));
    }
}
