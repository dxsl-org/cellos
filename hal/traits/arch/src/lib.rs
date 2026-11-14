#![no_std]

pub mod kernel_abi;
pub use kernel_abi::*;

/// Architecture-specific HAL trait.
pub trait Arch: Send + Sync {
    /// Architecture-specific Context type.
    type Context;

    /// Initialize the architecture.
    fn init(&self);

    /// Enable interrupts.
    fn enable_interrupts(&self);

    /// Disable interrupts.
    fn disable_interrupts(&self);

    /// Wait for interrupt.
    fn wait_for_interrupt(&self);

    /// Check if interrupts are enabled.
    fn interrupts_enabled(&self) -> bool;
}
