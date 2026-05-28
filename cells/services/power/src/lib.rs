#![no_std]
#![forbid(unsafe_code)]

//! Power management interfaces.

use ostd::prelude::*;

/// Device power management trait.
pub trait Powerable: Send + Sync {
    /// Suspend the device (enter low power state).
    fn suspend(&mut self) -> Result<()>;

    /// Resume the device (return to active state).
    fn resume(&mut self) -> Result<()>;
}

/// CPU frequency governor interface.
pub trait Governor: Send + Sync {
    /// Set CPU frequency in Hz.
    fn set_frequency(&self, hz: u64) -> Result<()>;

    /// Get current CPU frequency.
    fn get_frequency(&self) -> u64;
}

/// System power states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// S0: Working state.
    Working,
    /// S3: Suspend to RAM.
    SuspendToRam,
    /// S4: Hibernate (suspend to disk).
    Hibernate,
}
