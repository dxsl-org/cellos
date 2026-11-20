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
    /// f0-f31 of the task, saved and restored by `__switch` (offsets 19*8 ..
    /// 50*8). Cells are built for rv64gc/lp64d and keep live floating-point
    /// values across preemption. Trap entry does not save them: kernel FP use
    /// is limited to boot-config TOML parsing and float formatting/parsing in
    /// `core`, none of which runs on a Cell's syscall path today.
    pub fp: [u64; 32],
    /// `fcsr` (rounding mode and accrued exception flags), offset 51*8.
    pub fcsr: usize,
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
        Self::switch_with_saved_sstatus(old, new, outgoing_sstatus, 0, 0);
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
        root_ppn: usize,
        asid: usize,
        // ABI extension of `__switch(old, new, outgoing_sstatus)`: root/asid
        // remain trailing so the complete pre-mask status stays in a2.
    ) {
        extern "C" {
            fn __switch(
                old: *mut Context,
                new: *const Context,
                outgoing_sstatus: usize,
                root_ppn: usize,
                asid: usize,
            );
        }
        __switch(old, new, outgoing_sstatus, root_ppn, asid);
    }
}

/// A private root the switch programs between the outgoing save and the
/// incoming load: the root table's page number and its ASID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Root {
    base: core::num::NonZeroUsize,
    asid: usize,
}

impl Root {
    /// `base` is the root table's PPN. A zero PPN names no root (`None`): the
    /// switch's no-write path.
    pub fn new(base: usize, asid: usize) -> Option<Self> {
        core::num::NonZeroUsize::new(base).map(|base| Self { base, asid })
    }

    pub fn base(self) -> usize {
        self.base.get()
    }

    pub fn asid(self) -> usize {
        self.asid
    }
}

/// The `sstatus` of the activation that called [`IrqSaved::save`], with SIE masked
/// until the token is dropped or handed to [`switch`].
///
/// Dropping a live token restores the captured `sstatus`. [`switch`] consumes
/// the token: `__switch` stores the value in the outgoing `Context` and the
/// hart that resumes it restores it, so the token it returns is spent.
#[must_use]
pub struct IrqSaved {
    sstatus: Option<usize>,
    _not_send: core::marker::PhantomData<*mut ()>,
}

impl IrqSaved {
    /// Capture `sstatus` and mask SIE ([`save_and_disable_interrupts`]).
    #[inline(always)]
    pub fn save() -> Self {
        Self {
            sstatus: Some(save_and_disable_interrupts()),
            _not_send: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    fn spent() -> Self {
        Self {
            sstatus: None,
            _not_send: core::marker::PhantomData,
        }
    }
}

impl Drop for IrqSaved {
    #[inline(always)]
    fn drop(&mut self) {
        if let Some(sstatus) = self.sstatus {
            // SAFETY: captured on this activation by `save`; the `sstatus`
            // belongs to the suspended context, so SIE is never enabled for an
            // activation that entered masked.
            unsafe { restore_sstatus(sstatus) }
        }
    }
}

/// One context switch: the outgoing and incoming contexts, the root to program
/// between them, and the interrupt state captured before selection.
///
/// `root: None` writes no `satp`: after trap entry the kernel root is live, so
/// this is the SAS-to-SAS path.
#[must_use]
pub struct SwitchPlan {
    pub out: *mut Context,
    pub inc: *const Context,
    pub root: Option<Root>,
    pub irq: IrqSaved,
}

/// Run `plan` through `__switch`.
///
/// Returns this activation's interrupt token when it is resumed. On RV64 it is
/// already spent: the resuming `__switch` restored this context's `sstatus`.
///
/// # Safety
/// Both pointers must be valid context slots, `plan.irq` must have been saved
/// on this hart, and `plan.root` must name a completed root this hart may run.
#[inline(always)]
pub unsafe fn switch(plan: SwitchPlan) -> IrqSaved {
    let SwitchPlan {
        out,
        inc,
        root,
        irq,
    } = plan;
    let outgoing_sstatus = irq.sstatus.expect("switch needs a live interrupt token");
    core::mem::forget(irq);
    let (root_ppn, asid) = root.map_or((0, 0), |root| (root.base(), root.asid()));
    // SAFETY: the caller's contract; the token's `sstatus` moves into `old`.
    unsafe { Context::switch_with_saved_sstatus(out, inc, outgoing_sstatus, root_ppn, asid) };
    IrqSaved::spent()
}

/// Publish the incoming context's kernel stack for trap entry. RV64 traps find
/// their stack through `sscratch`, so there is nothing to publish.
#[inline(always)]
pub fn set_kernel_stack_for(inc: &Context) {
    super::trap::set_kernel_stack(inc.sp);
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
