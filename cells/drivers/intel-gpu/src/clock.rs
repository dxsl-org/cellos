//! Time for bounded register waits.
//!
//! Upstream bounds every hardware wait by time, not by a try count (`xe_mmio_wait32` in
//! `drivers/gpu/drm/xe/xe_mmio.c`, `poll_timeout_us` in `guc_wait_ucode`): a try count
//! means a different timeout on every CPU and clock. The driver reads time through this
//! trait so host tests can run a wait to its limit in no time and see exactly when it
//! gave up.

use ostd::syscall;

/// A monotonic microsecond clock with a way to let time pass.
pub trait Clock {
    /// Microseconds since an arbitrary fixed point.
    fn now_us(&self) -> u64;
    /// Let about `us` microseconds pass before the next register read (yielding the CPU
    /// where possible). May return early; callers re-check the time.
    fn relax(&mut self, us: u64);
}

/// The kernel's monotonic counter (`GetTime` raw ticks at the timer frequency).
pub struct KernelClock {
    hz: u64,
}

impl KernelClock {
    /// `None` when the kernel does not report a timer frequency: no bounded wait is
    /// possible then, so bring-up refuses to start rather than spin unbounded.
    pub fn new() -> Option<Self> {
        syscall::sys_get_timer_freq().map(|hz| Self { hz })
    }
}

impl Clock for KernelClock {
    fn now_us(&self) -> u64 {
        let ticks = u128::from(syscall::sys_get_time());
        (ticks * 1_000_000 / u128::from(self.hz)) as u64
    }

    fn relax(&mut self, us: u64) {
        let until = self.now_us().saturating_add(us);
        while self.now_us() < until {
            syscall::sys_yield();
        }
    }
}
