//! Per-hart local state, accessed in O(1) via the `tp` (thread-pointer) CSR.
//!
//! Each hart keeps `tp = &HART_LOCALS[hart_id]` at all times while running
//! kernel code.  The trap entry restores the kernel `tp` on U→S transitions
//! so all kernel code (syscall handler, allocator, scheduler) always sees the
//! correct HartLocal.  Cells run with the value stored in
//! `ViHartLocal::kernel_tp_for_cells`, which is independent of HartLocal.
//!
//! Phase 02 replaces the single global `CURRENT_CELL_ID` with a per-hart
//! `current_cell_id` field inside `ViHartLocal`.  Phase 03 adds per-hart
//! ready queues and the work-stealing scheduler.

pub mod ready;

use crate::task::smp::MAX_HARTS;
use alloc::collections::{BTreeMap, VecDeque};
#[cfg(not(target_arch = "riscv32"))]
use core::sync::atomic::AtomicU64;
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_arch = "riscv32")]
use portable_atomic::AtomicU64;

/// Per-hart local state.
///
/// LAYOUT IS ABI — the trap.S reads `kernel_tp_for_cells` by hardcoded offset
/// and the field order is FIXED.  Add new fields AFTER existing ones.
/// `#[repr(C)]` ensures Rust does not reorder or pad unexpectedly.
#[repr(C)]
pub struct ViHartLocal {
    /// This hart's id (0 = boot hart).
    pub hart_id: usize, // offset 0
    /// Cell ID currently running on this hart.  0 = kernel (no quota limit).
    pub current_cell_id: AtomicUsize, // offset 8  (AtomicUsize is transparent over usize)
    /// Value of `gp` captured at `install()` time — handed to new cells.
    pub kernel_gp: usize, // offset 16
    /// Value of `tp` that cells inherit on context switch.  Currently 0 (cells
    /// have no TLS); Phase 05 may give each cell a private tp.
    pub kernel_tp_for_cells: usize, // offset 24
    /// Per-hart ready queues keyed by priority (`u8`; higher = higher priority).
    /// Leaf lock: may be locked while holding SCHEDULER, never the reverse.
    pub ready: crate::sync::Spinlock<BTreeMap<u8, VecDeque<usize>>>,
    /// Task selected by the scheduler for this hart.  This can lead the actual
    /// CPU context while `Context::switch` is in flight.
    pub current_task_id: AtomicUsize,
    /// Task whose saved context is actually executing on this hart.  Published
    /// by the incoming context only after the raw switch has changed stacks.
    pub executing_task_id: AtomicUsize,
    /// Incoming task whose raw Context has been selected but has not yet
    /// completed the stack/register switch.  This is a transient ownership pin:
    /// retirement must treat it as live until the incoming-side completion hook
    /// publishes `executing_task_id` and clears this field.
    pub selected_task_id: AtomicUsize,
    /// Runnable outgoing task whose Context save is in flight.  It may be on
    /// this hart's ready queue for round-robin fairness, but another hart must
    /// not steal it until the raw switch has saved its Context.
    pub outgoing_context_save_task_id: AtomicUsize,
    /// Generation paired with `current_cell_id` for the task currently selected
    /// on this hart.  Trap code snapshots it with the CellId before surrendering
    /// allocation attribution to the kernel.
    current_cell_generation: AtomicU64,
    /// Private-root identity selected for this hart. Zero is the shared SAS root.
    current_domain_id: AtomicU64,
    /// Immutable generation paired with `current_domain_id`.
    current_domain_generation: AtomicU64,
    /// Last safe-root completion acknowledgement for remote retirement.
    domain_ack_generation: AtomicU64,
    /// A safe root is active but its incoming Context has not yet acknowledged it.
    safe_root_pending: AtomicUsize,
    /// Fixed retirement handoff slot.  A trap fault or clean `Exit` writes
    /// scalar state here before scheduler-owned collections are touched; the
    /// scheduler consumes it later with kernel allocation attribution.
    deferred_retirement_pending: AtomicUsize,
    deferred_retirement_kind: AtomicUsize,
    deferred_retirement_tid: AtomicUsize,
    deferred_retirement_cell_id: AtomicUsize,
    deferred_retirement_generation: AtomicU64,
    deferred_retirement_exit_code: AtomicUsize,
    deferred_retirement_fault_cause: AtomicUsize,
    deferred_retirement_fault_pc: AtomicUsize,
    deferred_retirement_fault_addr: AtomicUsize,
    /// Private root whose execution pin this hart currently holds. Mirrors
    /// `current_domain_id` in lifetime: set on successful activation, moved out
    /// when the hart plans a transition away.
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    pinned_domain: execution_pin::HartOwnedSlot,
    /// Outgoing root awaiting release at the switch-completion hook. Staged
    /// under `SCHEDULER`, consumed exactly once per completed transition away.
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    staged_domain_release: execution_pin::HartOwnedSlot,
    /// Recoverable user-copy fault guard (RV64). Armed by `task::user_copy`
    /// around exactly one guarded byte-copy window and read by the trap
    /// handler through `vi_user_copy_guard_fault`. Hart-owned: only the
    /// executing hart touches its own slot, so plain atomics need no locks.
    /// Zero resume PC means "no landing pad"; the active flag is the gate.
    #[cfg(target_arch = "riscv64")]
    pub user_copy_guard_active: AtomicUsize,
    #[cfg(target_arch = "riscv64")]
    pub user_copy_guard_resume_pc: AtomicUsize,
    /// Inclusive start of the user range covered by the armed guard. The
    /// trap hook rejects faults whose `stval` falls outside [start, end).
    #[cfg(target_arch = "riscv64")]
    pub user_copy_guard_start: AtomicUsize,
    /// Exclusive end of the guarded user range.
    #[cfg(target_arch = "riscv64")]
    pub user_copy_guard_end: AtomicUsize,
}

/// Static array of per-hart local state, one entry per supported hart.
/// Accessed without any lock: hart N only writes HART_LOCALS[N] from N.
/// SAFETY: interior mutability via AtomicUsize; the `usize` fields are only
/// written during `install()` before the hart handles any interrupt.
pub static HART_LOCALS: [ViHartLocal; MAX_HARTS] = {
    // `ZERO` is consumed exactly once below, by the `[ZERO; MAX_HARTS]` array-repeat
    // expression — rustc evaluates a `const` operand fresh per array slot, so each
    // hart gets its OWN independent `AtomicUsize`/`Spinlock`, not a shared instance.
    // That is the desired behaviour here (see module doc: hart N only touches slot N),
    // so this is not the aliasing footgun `declare_interior_mutable_const` warns about;
    // switching to `static` would break the repeat expression (requires a `const` for
    // non-`Copy` element types).
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: ViHartLocal = ViHartLocal {
        hart_id: 0,
        current_cell_id: AtomicUsize::new(0),
        kernel_gp: 0,
        kernel_tp_for_cells: 0,
        ready: crate::sync::Spinlock::new(BTreeMap::new()),
        current_task_id: AtomicUsize::new(0),
        executing_task_id: AtomicUsize::new(0),
        selected_task_id: AtomicUsize::new(0),
        outgoing_context_save_task_id: AtomicUsize::new(0),
        current_cell_generation: AtomicU64::new(0),
        current_domain_id: AtomicU64::new(0),
        current_domain_generation: AtomicU64::new(0),
        domain_ack_generation: AtomicU64::new(0),
        safe_root_pending: AtomicUsize::new(0),
        deferred_retirement_pending: AtomicUsize::new(0),
        deferred_retirement_kind: AtomicUsize::new(0),
        deferred_retirement_tid: AtomicUsize::new(0),
        deferred_retirement_cell_id: AtomicUsize::new(0),
        deferred_retirement_generation: AtomicU64::new(0),
        deferred_retirement_exit_code: AtomicUsize::new(0),
        deferred_retirement_fault_cause: AtomicUsize::new(0),
        deferred_retirement_fault_pc: AtomicUsize::new(0),
        deferred_retirement_fault_addr: AtomicUsize::new(0),
        #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
        pinned_domain: execution_pin::HartOwnedSlot(core::cell::UnsafeCell::new(None)),
        #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
        staged_domain_release: execution_pin::HartOwnedSlot(core::cell::UnsafeCell::new(None)),
        #[cfg(target_arch = "riscv64")]
        user_copy_guard_active: AtomicUsize::new(0),
        #[cfg(target_arch = "riscv64")]
        user_copy_guard_resume_pc: AtomicUsize::new(0),
        #[cfg(target_arch = "riscv64")]
        user_copy_guard_start: AtomicUsize::new(0),
        #[cfg(target_arch = "riscv64")]
        user_copy_guard_end: AtomicUsize::new(0),
    };
    [ZERO; MAX_HARTS]
};

/// Pointer to the calling hart's `ViHartLocal`, stored as a plain `usize`.
///
/// `trap.S` loads `tp` from this address on every U→S trap so kernel code
/// always runs with a valid hart-local pointer.  Exposed `#[no_mangle]` so
/// the assembler can reference it by name without mangling.
///
/// Phase 03 upgrades this to the `sscratch = &HartLocal` protocol for full
/// SMP correctness.  For Phase 02 (single active hart), the single-entry
/// array gives the correct result.
#[no_mangle]
pub static HART_LOCAL_TP_ADDRS: [AtomicUsize; MAX_HARTS] =
    [AtomicUsize::new(0), AtomicUsize::new(0)];

/// Initialize the calling hart's `ViHartLocal` and write `tp` to point at it.
///
/// Call BEFORE enabling the scheduler or handling any interrupt on this hart.
/// Hart 0 calls this as the first action of `task::init()`.
/// Secondary harts call this from `smp_hart_entry`, after installing stvec.
pub fn install(hart_id: usize) {
    assert!(
        hart_id < MAX_HARTS,
        "hart_id {} >= MAX_HARTS {}",
        hart_id,
        MAX_HARTS
    );

    // Capture the current gp and tp so we can hand them to cells unchanged.
    let (gp, tp) = crate::hal::arch::get_gp_tp();

    // SAFETY: hart_id < MAX_HARTS; we only write HART_LOCALS[hart_id] from
    // the hart with that id — no concurrent writers for this index.
    let hl = &HART_LOCALS[hart_id];

    // Write hart_id once (not atomic — no other hart touches this slot yet).
    // SAFETY: single writer; written before any reader could observe this hart.
    unsafe {
        let ptr = hl as *const ViHartLocal as *mut ViHartLocal;
        core::ptr::addr_of_mut!((*ptr).hart_id).write(hart_id);
        core::ptr::addr_of_mut!((*ptr).kernel_gp).write(gp);
        core::ptr::addr_of_mut!((*ptr).kernel_tp_for_cells).write(tp);
    }
    hl.current_cell_id.store(0, Ordering::Relaxed);
    hl.current_cell_generation.store(0, Ordering::Relaxed);
    hl.current_domain_id.store(0, Ordering::Relaxed);
    hl.current_domain_generation.store(0, Ordering::Relaxed);
    hl.domain_ack_generation.store(0, Ordering::Relaxed);
    hl.safe_root_pending.store(0, Ordering::Relaxed);
    hl.deferred_retirement_pending.store(0, Ordering::Relaxed);

    // Publish this logical hart's restore pointer before installing its stvec.
    // The hart-specific trap stub reads only its own array slot.
    let hl_addr = hl as *const ViHartLocal as usize;
    HART_LOCAL_TP_ADDRS[hart_id].store(hl_addr, Ordering::Release);

    // Write tp CSR to point at this hart's HartLocal.
    // SAFETY: tp is a callee-save GPR used here as a kernel-internal pointer;
    // cells receive `kernel_tp_for_cells` (not this pointer) on context switch.
    unsafe { write_tp(hl_addr) };

    #[cfg(target_arch = "riscv64")]
    crate::hal::trap::init_for_hart(hart_id);
}

/// Return a reference to the calling hart's `ViHartLocal`.
///
/// On RISC-V reads the `tp` CSR. On other architectures (x86_64 single-hart
/// bring-up) returns HART_LOCALS[0] directly.
///
/// # Safety
/// On RISC-V: `tp` must point to a valid `ViHartLocal` (guaranteed after `install()`).
#[inline(always)]
pub unsafe fn current_hart() -> &'static ViHartLocal {
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        let tp: usize;
        core::arch::asm!("mv {}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
        &*(tp as *const ViHartLocal)
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
    {
        &HART_LOCALS[0]
    }
}

/// Return the calling hart's id.
///
/// Returns 0 if called before `install()` (safe — hart 0 is hart 0).
#[inline(always)]
pub fn current_hart_id() -> usize {
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        let tp: usize;
        unsafe {
            core::arch::asm!("mv {}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
        }
        if tp == 0 {
            return 0;
        }
        unsafe { (*(tp as *const ViHartLocal)).hart_id }
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
    {
        0
    }
}

/// Cell ID currently running on this hart (0 = kernel, no quota).
///
/// On RISC-V reads `tp` CSR. On other architectures returns HART_LOCALS[0].current_cell_id.
#[inline(always)]
pub fn current_cell_id() -> usize {
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        let tp: usize;
        unsafe {
            core::arch::asm!("mv {}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
        }
        if tp == 0 {
            return 0;
        }
        unsafe {
            (*(tp as *const ViHartLocal))
                .current_cell_id
                .load(Ordering::Relaxed)
        }
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
    {
        HART_LOCALS[0].current_cell_id.load(Ordering::Relaxed)
    }
}

/// Provenance required to place a fault in the recoverable scheduler funnel.
///
/// A Cell attribution is accounting state, not evidence that the current CPU
/// context was executing Cell code.  In particular, kernel code servicing a
/// Cell syscall retains that attribution while it holds kernel locks.  Only a
/// trap handler that has established a U-mode origin can mint this capability.
#[derive(Clone, Copy)]
pub struct TrapProvenUserFault(());

impl TrapProvenUserFault {
    #[inline(always)]
    pub(super) const fn new() -> Self {
        Self(())
    }
}

/// Origin of a fault considered for deferred Cell retirement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FaultOrigin {
    TrapProvenUser,
    #[cfg(test)]
    KernelPanic,
}

impl FaultOrigin {
    #[inline(always)]
    const fn permits_deferred_retirement(self) -> bool {
        matches!(self, Self::TrapProvenUser)
    }
}

/// Scalar fault state captured by a trap-proven U-mode fault and consumed by
/// the scheduler.
///
/// This deliberately contains no task name or heap-backed diagnostic payload:
/// quota-exhausted Cell faults must be able to hand off without allocating.
#[derive(Clone, Copy)]
pub struct DeferredFault {
    pub tid: usize,
    pub cell_id: usize,
    pub generation: u64,
    pub cause: usize,
    pub pc: usize,
    pub fault_addr: usize,
    origin: FaultOrigin,
}

/// Scalar clean-exit state captured before the exiting Cell's allocation
/// attribution is surrendered.  Root versus worker classification belongs to
/// the scheduler, where it can use the authoritative task table.
#[derive(Clone, Copy)]
pub struct DeferredExit {
    pub tid: usize,
    pub cell_id: usize,
    pub generation: u64,
    pub code: usize,
}

/// One allocation-free handoff record per hart.  A task can reach only one of
/// these paths before yielding, so a single fixed slot preserves the ordering
/// between the attribution handoff and scheduler retirement.
#[derive(Clone, Copy)]
pub enum DeferredRetirement {
    Fault(DeferredFault),
    Exit(DeferredExit),
}

impl DeferredFault {
    /// Construct the sole recoverable deferred-fault record.
    ///
    /// `TrapProvenUserFault` cannot be minted outside `task`; this prevents
    /// Cell accounting attribution or a kernel panic from entering the
    /// scheduler retirement path.
    #[inline(always)]
    pub(super) fn from_user_trap(
        _provenance: TrapProvenUserFault,
        tid: usize,
        cell_id: usize,
        generation: u64,
        cause: usize,
        pc: usize,
        fault_addr: usize,
    ) -> Self {
        Self::from_origin(
            FaultOrigin::TrapProvenUser,
            tid,
            cell_id,
            generation,
            cause,
            pc,
            fault_addr,
        )
        .expect("trap-proven U-mode origin must permit deferred retirement")
    }

    #[inline(always)]
    fn from_origin(
        origin: FaultOrigin,
        tid: usize,
        cell_id: usize,
        generation: u64,
        cause: usize,
        pc: usize,
        fault_addr: usize,
    ) -> Option<Self> {
        origin.permits_deferred_retirement().then_some(Self {
            tid,
            cell_id,
            generation,
            cause,
            pc,
            fault_addr,
            origin,
        })
    }

    #[inline(always)]
    pub fn is_trap_proven_user_fault(self) -> bool {
        self.origin.permits_deferred_retirement()
    }
}

#[cfg(test)]
impl DeferredFault {
    /// Unit-test-only constructor for the same origin produced by the trap ABI.
    pub(crate) fn test_trap_proven_user(
        tid: usize,
        cell_id: usize,
        generation: u64,
        cause: usize,
        pc: usize,
        fault_addr: usize,
    ) -> Self {
        Self::from_user_trap(
            TrapProvenUserFault::new(),
            tid,
            cell_id,
            generation,
            cause,
            pc,
            fault_addr,
        )
    }
}

#[cfg(test)]
mod fault_origin_tests {
    use super::{DeferredFault, FaultOrigin};

    #[test]
    fn kernel_panic_with_cell_attribution_cannot_enter_deferred_retirement() {
        // Model a kernel panic while SCHEDULER is held for Cell 71: attribution
        // must not be mistaken for a U-mode execution proof.
        assert!(DeferredFault::from_origin(FaultOrigin::KernelPanic, 9, 71, 3, 0, 0, 0).is_none());
    }

    #[test]
    fn trap_proven_user_fault_remains_recoverable() {
        let fault = DeferredFault::from_origin(FaultOrigin::TrapProvenUser, 9, 71, 3, 0xf, 0, 0)
            .expect("U-mode trap provenance must enter deferred retirement");
        assert!(fault.is_trap_proven_user_fault());
    }
}

/// Read the generation paired with the currently attributed Cell.
#[inline(always)]
pub fn current_cell_generation() -> u64 {
    unsafe { current_hart() }
        .current_cell_generation
        .load(Ordering::Relaxed)
}

/// Update the current Cell attribution and its generation together.
///
/// Scheduler dispatch uses this before entering a Cell.  Callers that only
/// temporarily suppress allocation attribution preserve the generation with
/// `set_current_cell_id(0)` and restore the original CellId afterwards.
#[inline(always)]
pub fn set_current_cell_context(id: usize, generation: u64) {
    let hart = unsafe { current_hart() };
    hart.current_cell_generation
        .store(generation, Ordering::Relaxed);
    hart.current_cell_id.store(id, Ordering::Relaxed);
}

/// Publish the private root selected while the scheduler state was stable.
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[inline(always)]
pub(crate) fn set_current_domain(id: u64, generation: u64) {
    let hart = unsafe { current_hart() };
    hart.current_domain_generation
        .store(generation, Ordering::Release);
    hart.current_domain_id.store(id, Ordering::Release);
}

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[inline(always)]
pub(crate) fn current_domain() -> (u64, u64) {
    let hart = unsafe { current_hart() };
    (
        hart.current_domain_id.load(Ordering::Acquire),
        hart.current_domain_generation.load(Ordering::Acquire),
    )
}

/// Clear a domain only after the incoming safe-root context has completed.
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
pub(crate) fn acknowledge_safe_root() {
    let hart = unsafe { current_hart() };
    let generation = hart.current_domain_generation.swap(0, Ordering::AcqRel);
    hart.current_domain_id.store(0, Ordering::Release);
    hart.domain_ack_generation
        .store(generation, Ordering::Release);
}

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[inline(always)]
pub(crate) fn mark_safe_root_pending() {
    unsafe { current_hart() }
        .safe_root_pending
        .store(1, Ordering::Release);
}

/// Execution-pin bookkeeping for native domains. The slots are touched only by
/// the owning hart, and only inside the interrupt-masked window from plan
/// construction to the incoming-side completion hook — no other hart can
/// observe a partially updated slot.
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
mod execution_pin {
    use super::current_hart;
    use alloc::sync::Arc;
    use core::cell::UnsafeCell;
    use crate::memory::address_space::AddressSpace;

    /// Hart-owned slot. `HART_LOCALS` is shared across harts as a static, so
    /// the wrapper carries `Sync`; soundness rests on the single-owner rule
    /// documented on [`super::ViHartLocal`], not on this impl.
    pub(crate) struct HartOwnedSlot(pub(crate) UnsafeCell<Option<Arc<AddressSpace>>>);
    unsafe impl Sync for HartOwnedSlot {}

    /// Advance this hart's execution-pin slot to `next`, staging any displaced
    /// root for release at the switch-completion hook. Called under `SCHEDULER`
    /// while planning a transition, so the displaced root cannot be selected
    /// again on this hart before its pin is released.
    pub(crate) fn advance(next: Option<Arc<AddressSpace>>) {
        let hart = unsafe { current_hart() };
        // SAFETY: sole access is this hart within the masked plan→completion
        // window documented on the slot fields.
        unsafe {
            let pinned = &mut *hart.pinned_domain.0.get();
            let displaced = pinned.take();
            *pinned = next;
            if displaced.is_some() {
                *hart.staged_domain_release.0.get() = displaced;
            }
        }
    }

    /// Consume the staged outgoing-root release. Returns `None` on every switch
    /// that did not leave a pinned root.
    pub(crate) fn take_staged() -> Option<Arc<AddressSpace>> {
        // SAFETY: sole access is this hart, within the masked window above.
        unsafe { (*current_hart().staged_domain_release.0.get()).take() }
    }
}

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
pub(crate) use execution_pin::{
    advance as advance_execution_pin, take_staged as take_staged_execution_release,
};

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[inline(always)]
pub(crate) fn take_safe_root_pending() -> bool {
    unsafe { current_hart() }
        .safe_root_pending
        .swap(0, Ordering::AcqRel)
        != 0
}

/// Remote retirement tests observe this generation after an incoming safe-root
/// completion; a root remains owned until every relevant hart reports it.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn domain_ack_generation_for(hart_id: usize) -> u64 {
    HART_LOCALS
        .get(hart_id)
        .map(|hart| hart.domain_ack_generation.load(Ordering::Acquire))
        .unwrap_or(0)
}
const DEFERRED_RETIREMENT_EXIT: usize = 1;
const DEFERRED_RETIREMENT_FAULT: usize = 2;

/// Publish a fixed, per-hart fault record without allocating or acquiring a
/// lock. Trap entry has interrupts masked, so only this hart can produce it.
#[inline(always)]
pub fn defer_fault(fault: DeferredFault) {
    let hart = unsafe { current_hart() };
    hart.deferred_retirement_tid
        .store(fault.tid, Ordering::Relaxed);
    hart.deferred_retirement_cell_id
        .store(fault.cell_id, Ordering::Relaxed);
    hart.deferred_retirement_generation
        .store(fault.generation, Ordering::Relaxed);
    hart.deferred_retirement_fault_cause
        .store(fault.cause, Ordering::Relaxed);
    hart.deferred_retirement_fault_pc
        .store(fault.pc, Ordering::Relaxed);
    hart.deferred_retirement_fault_addr
        .store(fault.fault_addr, Ordering::Relaxed);
    hart.deferred_retirement_kind
        .store(DEFERRED_RETIREMENT_FAULT, Ordering::Relaxed);
    hart.deferred_retirement_pending.store(1, Ordering::Release);
}

/// Publish a clean task exit before surrendering the exiting Cell's allocation
/// attribution.  Root classification and every heap-backed lifecycle action
/// remain deferred to the scheduler.
#[inline(always)]
pub fn defer_exit(exit: DeferredExit) {
    let hart = unsafe { current_hart() };
    hart.deferred_retirement_tid
        .store(exit.tid, Ordering::Relaxed);
    hart.deferred_retirement_cell_id
        .store(exit.cell_id, Ordering::Relaxed);
    hart.deferred_retirement_generation
        .store(exit.generation, Ordering::Relaxed);
    hart.deferred_retirement_exit_code
        .store(exit.code, Ordering::Relaxed);
    hart.deferred_retirement_kind
        .store(DEFERRED_RETIREMENT_EXIT, Ordering::Relaxed);
    hart.deferred_retirement_pending.store(1, Ordering::Release);
}

/// Consume this hart's pending retirement record after allocation attribution
/// has switched to Cell 0 and normal scheduler locking is permitted.
#[inline(always)]
pub fn take_deferred_retirement() -> Option<DeferredRetirement> {
    let hart = unsafe { current_hart() };
    if hart.deferred_retirement_pending.swap(0, Ordering::AcqRel) == 0 {
        return None;
    }

    let tid = hart.deferred_retirement_tid.load(Ordering::Relaxed);
    let cell_id = hart.deferred_retirement_cell_id.load(Ordering::Relaxed);
    let generation = hart.deferred_retirement_generation.load(Ordering::Relaxed);
    match hart.deferred_retirement_kind.load(Ordering::Relaxed) {
        DEFERRED_RETIREMENT_EXIT => Some(DeferredRetirement::Exit(DeferredExit {
            tid,
            cell_id,
            generation,
            code: hart.deferred_retirement_exit_code.load(Ordering::Relaxed),
        })),
        DEFERRED_RETIREMENT_FAULT => {
            Some(DeferredRetirement::Fault(DeferredFault::from_user_trap(
                TrapProvenUserFault::new(),
                tid,
                cell_id,
                generation,
                hart.deferred_retirement_fault_cause.load(Ordering::Relaxed),
                hart.deferred_retirement_fault_pc.load(Ordering::Relaxed),
                hart.deferred_retirement_fault_addr.load(Ordering::Relaxed),
            )))
        }
        kind => panic!("[task] invalid deferred retirement kind {kind}"),
    }
}

/// Update the cell-id attribution for the calling hart.
#[inline(always)]
pub fn set_current_cell_id(id: usize) {
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        let tp: usize;
        unsafe {
            core::arch::asm!("mv {}, tp", out(reg) tp, options(nomem, nostack, preserves_flags));
        }
        if tp == 0 {
            return;
        }
        unsafe {
            (*(tp as *const ViHartLocal))
                .current_cell_id
                .store(id, Ordering::Relaxed)
        };
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
    {
        HART_LOCALS[0].current_cell_id.store(id, Ordering::Relaxed);
    }
}

/// Write the `tp` register.
///
/// # Safety
/// Caller is responsible for ensuring `val` is a valid `ViHartLocal` pointer
/// (or 0 for the pre-install sentinel).  Must run with interrupts disabled or
/// from boot context where no concurrent trap can misread a partial write.
#[inline(always)]
#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
pub unsafe fn write_tp(val: usize) {
    // SAFETY: writing tp CSR is always safe from S-mode; the value is either
    // a valid HART_LOCALS pointer or 0 (pre-install). Caller ensures context.
    core::arch::asm!("mv tp, {}", in(reg) val, options(nomem, nostack, preserves_flags));
}

/// No-op on non-RISC-V targets (no `tp`-based hart-local addressing).
///
/// # Safety
/// No preconditions on these targets — `_val` is unused and no state is written.
#[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
pub unsafe fn write_tp(_val: usize) {}
