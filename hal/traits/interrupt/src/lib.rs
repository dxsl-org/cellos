#![no_std]

/// Interrupt controller trait.
pub trait InterruptController: Send + Sync {
    /// Initialize the controller.
    fn init(&self);

    /// Enable an IRQ.
    fn enable_irq(&self, irq: u32);

    /// Disable an IRQ.
    fn disable_irq(&self, irq: u32);

    /// Acknowledge an IRQ.
    fn ack_irq(&self, irq: u32);
}
