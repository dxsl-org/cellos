//! Boot abstractions
//! 
//! Defines how the system powers up, restarts, or shuts down.

/// The BootController trait handles system life-cycle events.
pub trait BootController {
    /// Cold boot initialization logic.
    /// Should be called exactly once at startup.
    fn init(&self);

    /// Hard reset the system (Reboot).
    /// This function should not return.
    fn reset(&self) -> !;

    /// Power off the system completely.
    /// This function should not return.
    fn shutdown(&self) -> !;
}
