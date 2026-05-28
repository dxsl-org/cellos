#![no_std]

/// Architecture-specific HAL trait.
pub trait Arch: Send + Sync {
    /// Architecture-specific Context type.
    type Context;

    /// Initialize the architecture.
    fn init(&self);

    /// Perform context switch.
    /// # Safety
    /// This function is unsafe because it manipulates raw pointers and machine state.
    unsafe fn switch_context(&self, old: *mut Self::Context, new: *const Self::Context);

    /// Enable interrupts.
    fn enable_interrupts(&self);

    /// Disable interrupts.
    fn disable_interrupts(&self);

    /// Wait for interrupt.
    fn wait_for_interrupt(&self);

    /// Check if interrupts are enabled.
    fn interrupts_enabled(&self) -> bool;
}

/// The BootController trait handles system life-cycle events.
pub trait BootController {
    fn init(&self);
    fn reset(&self) -> !;
    fn shutdown(&self) -> !;
}
