#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub s0: usize,
    pub s1: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub sepc: usize,
    pub sstatus: usize,
    pub gp: usize,
    pub tp: usize,
    pub sscratch: usize,
}

// Assembly implementation handled by build.rs.

/// Atomically capture the complete outgoing supervisor status and mask SIE.
///
/// The scheduler calls this before it publishes any selection state. Keeping
/// the full value, rather than only a boolean SIE snapshot, lets `__switch`
/// persist the exact outgoing status in its `Context`.
#[inline(always)]
pub fn save_and_disable_interrupts() -> usize {
    let sstatus: usize;
    // SAFETY: `csrrci` is an atomic S-mode CSR read/modify/write. Omitting
    // `nomem` makes this a compiler barrier for the scheduling publications
    // that follow it.
    unsafe {
        core::arch::asm!(
            "csrrci {saved}, sstatus, 0x2",
            saved = out(reg) sstatus,
            options(nostack),
        );
    }
    sstatus
}

/// Restore a value returned by [`save_and_disable_interrupts`].
///
/// # Safety
/// `sstatus` must be a supervisor-status snapshot captured on this hart.
#[inline(always)]
pub unsafe fn restore_sstatus(sstatus: usize) {
    core::arch::asm!(
        "csrw sstatus, {saved}",
        saved = in(reg) sstatus,
        options(nostack),
    );
}

impl Context {
    /// Perform a context switch after masking interrupts at this call boundary.
    ///
    /// Scheduler code must use [`Context::switch_with_saved_sstatus`] instead:
    /// it has to mask SIE before selection, not merely at the assembly boundary.
    ///
    /// # Safety
    /// This function performs a raw context switch and must be called with valid pointers.
    #[inline(always)]
    pub unsafe fn switch(old: *mut Context, new: *const Context) {
        let outgoing_sstatus = save_and_disable_interrupts();
        Self::switch_with_saved_sstatus(old, new, outgoing_sstatus);
    }

    /// Switch contexts while saving the caller's pre-mask `sstatus` in `old`.
    ///
    /// The current hart must still have SIE masked from the matching
    /// [`save_and_disable_interrupts`] call. `__switch` restores the incoming
    /// context's saved `sstatus` only after its registers and stack are complete.
    ///
    /// # Safety
    /// Both pointers must be valid context slots, and `outgoing_sstatus` must be
    /// the snapshot captured on this hart before scheduler selection.
    #[inline(always)]
    pub unsafe fn switch_with_saved_sstatus(
        old: *mut Context,
        new: *const Context,
        outgoing_sstatus: usize,
    ) {
        extern "C" {
            fn __switch(
                old: *mut Context,
                new: *const Context,
                outgoing_sstatus: usize,
            );
        }
        __switch(old, new, outgoing_sstatus);
    }
}

pub fn get_gp_tp() -> (usize, usize) {
    let gp: usize;
    let tp: usize;
    unsafe {
        #[cfg(target_arch = "riscv64")]
        {
            core::arch::asm!("mv {0}, gp", out(reg) gp);
            core::arch::asm!("mv {0}, tp", out(reg) tp);
        }
        #[cfg(not(target_arch = "riscv64"))]
        {
            gp = 0;
            tp = 0;
        }
    }
    (gp, tp)
}

/// Read the current value of the `tp` (thread-pointer) register.
///
/// # Safety
/// Reading tp is always safe from S-mode; no side effects.
#[cfg(target_arch = "riscv64")]
pub unsafe fn read_tp() -> usize {
    let tp: usize;
    core::arch::asm!("mv {0}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
    tp
}

/// Write a new value to the `tp` (thread-pointer) register.
///
/// # Safety
/// Caller must ensure `val` is either 0 or a pointer to a valid `ViHartLocal`.
/// Should only be called from boot context or `hart_local::install()` with
/// interrupts disabled so no trap fires with a half-written tp.
#[cfg(target_arch = "riscv64")]
pub unsafe fn write_tp(val: usize) {
    // SAFETY: writing tp from S-mode is always permitted; caller ensures value
    // is a valid ViHartLocal pointer.
    core::arch::asm!("mv tp, {0}", in(reg) val, options(nomem, nostack, preserves_flags));
}
