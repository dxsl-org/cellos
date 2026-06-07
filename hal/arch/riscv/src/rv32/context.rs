use crate::rv32::Rv32Context;

impl Rv32Context {
    /// Switch CPU context from `old` to `new`.
    ///
    /// Saves all callee-saved registers + S-mode CSRs into `*old`, then
    /// restores them from `*new` and returns into the new task's execution.
    ///
    /// # Safety
    /// Both pointers must be valid, properly aligned `Rv32Context` values.
    /// Call with interrupts disabled to prevent a context switch mid-switch.
    #[inline(always)]
    pub unsafe fn switch(old: *mut Rv32Context, new: *const Rv32Context) {
        extern "C" {
            fn __switch32(old: *mut Rv32Context, new: *const Rv32Context);
        }
        __switch32(old, new);
    }
}
