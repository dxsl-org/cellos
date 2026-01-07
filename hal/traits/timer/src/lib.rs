#![no_std]

/// Timer trait.
pub trait Timer: Send + Sync {
    /// Get current time in nanoseconds.
    fn now_ns(&self) -> u64;
    
    /// Set a one-shot timer.
    fn set_timeout(&self, ns: u64);
}
