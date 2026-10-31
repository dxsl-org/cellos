//! Kernel-owned fast-IPC dispatch table — the single canonical instance.
//!
//! In a Single Address Space there is no privilege wall between Cells: a trusted
//! A future direct path would reduce a service call to an indirect branch, but
//! that path has no runtime measurement or loader bridge today. To work, ONE handler
//! pointer must be shared by the VFS cell (which registers it), client cells
//! (which call it), and the kernel (which nulls it if VFS faults).
//!
//! Because Cells are separately-loaded ELFs (each with its own copy of any
//! `static`), the shared instance cannot live in a per-cell library — it lives
//! HERE, in the kernel. No loader import-resolution bridge exists today.
//! Separately linked Cells use their private `ostd` table and therefore take
//! the message fallback. This table is retained only as design scaffolding for
//! the ruled Tier-1 rewrite; the kernel uses
//! `set_vfs_handler_cell`/`clear_vfs_if_cell` directly.
//!
//! ## Safety invariant
//! The handler pointer is published once at VFS startup (before any client call)
//! with `Release` ordering, read with `Acquire`, and only ever nulled on VFS
//! fault. Every writer of the pointer or its owner takes `VFS_HANDLER_WRITE`,
//! so an owner-conditional clear on one hart cannot interleave with a
//! re-registration on another. Readers stay lock-free.

use api::caller_identity::CallerIdentity;
use api::fast_ipc::{TrustedHandle, VfsCell};
use api::ipc::{VfsRequest, IPC_BUF_SIZE};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Signature of a registered VFS fast-IPC handler: read `req` on behalf of
/// `caller`, write the response into `out`, return the number of bytes written.
///
/// `caller` is `None` when the kernel could not attribute the call to a live
/// cell. The handler must then refuse the request: this path serves `GetFile`,
/// which hands back a raw `DataPtr` — permanent, unrevocable read authority in a
/// single address space — so an unauthorized answer here cannot be taken back.
pub type VfsFastHandler = unsafe fn(
    caller: Option<CallerIdentity>,
    req: &VfsRequest<'_>,
    out: &mut [u8; IPC_BUF_SIZE],
) -> usize;

static VFS_HANDLER_PTR: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
/// Raw CellId that registered the handler; 0 = unregistered. Lets the kernel
/// null the pointer when (and only when) that specific cell faults.
static VFS_HANDLER_CELL: AtomicUsize = AtomicUsize::new(0);
/// Serializes every mutation of `VFS_HANDLER_PTR`/`VFS_HANDLER_CELL` so the
/// owner compare and the paired clear in `clear_vfs_if_cell` are one step.
/// Never held across a handler call.
static VFS_HANDLER_WRITE: crate::sync::Spinlock<()> = crate::sync::Spinlock::new(());

/// Register the kernel-side VFS fast-IPC handler for the future Tier-1 bridge.
/// No Cell import resolves here today.
///
/// The stable symbol name is retained for the reviewed bridge design.
#[no_mangle]
pub extern "Rust" fn register_vfs(handler: VfsFastHandler) {
    let _write = VFS_HANDLER_WRITE.lock();
    // SAFETY: fn-ptr → *mut () for atomic storage; recovered with the same type
    // in `call_vfs`. Published Release so the handler body is visible to Acquire readers.
    VFS_HANDLER_PTR.store(handler as *mut (), Ordering::Release);
}

/// Record which cell owns the registered handler (kernel-internal; called from
/// the VFS spawn path so a later fault of that cell can null the pointer).
pub fn set_vfs_handler_cell(cell_id_raw: usize) {
    let _write = VFS_HANDLER_WRITE.lock();
    VFS_HANDLER_CELL.store(cell_id_raw, Ordering::Release);
}

/// Null the handler pointer iff `cell_id_raw` is the registered owner. Called by
/// the kernel fault path so a future `call_vfs` does not jump into dead VFS code.
pub fn clear_vfs_if_cell(cell_id_raw: usize) {
    if cell_id_raw == 0 {
        return;
    }
    let _write = VFS_HANDLER_WRITE.lock();
    if VFS_HANDLER_CELL.load(Ordering::Acquire) == cell_id_raw {
        VFS_HANDLER_PTR.store(core::ptr::null_mut(), Ordering::Release);
        VFS_HANDLER_CELL.store(0, Ordering::Release);
    }
}

pub(crate) fn is_registered_vfs_cell(cell_id_raw: usize) -> bool {
    cell_id_raw != 0 && VFS_HANDLER_CELL.load(Ordering::Acquire) == cell_id_raw
}

#[cfg(feature = "test-hooks")]
pub(crate) fn vfs_handler_cell_snapshot() -> usize {
    VFS_HANDLER_CELL.load(Ordering::Acquire)
}

/// Snapshot the registered handler pointer separately from its Cell owner.
/// Fixture code may exercise owner routing but must never clear another
/// component's registered handler.
#[cfg(feature = "test-hooks")]
pub(crate) fn vfs_handler_pointer_snapshot() -> usize {
    VFS_HANDLER_PTR.load(Ordering::Acquire) as usize
}

/// Restore a pointer obtained from `vfs_handler_pointer_snapshot` after a
/// test-only fixture temporarily registers its own handler.
#[cfg(feature = "test-hooks")]
pub(crate) fn restore_vfs_handler_pointer_for_test(handler: usize) {
    let _write = VFS_HANDLER_WRITE.lock();
    VFS_HANDLER_PTR.store(handler as *mut (), Ordering::Release);
}

/// RAII guard that restores the S-mode interrupt-enable bit (SIE) on drop.
///
/// Constructed by disabling SIE and recording its prior state.  `Drop` restores
/// it, so SIE is always restored even if the handler panics (Rust drop glue runs
/// before the panic handler, giving the guard a chance to clean up).
struct SieGuard(
    /// `true` if SIE was set before we disabled it; `false` = was already clear.
    bool,
);

impl SieGuard {
    /// Disable SIE and return a guard that will restore it.
    ///
    /// # Safety
    /// Must be called from S-mode.
    #[inline]
    unsafe fn disable() -> Self {
        #[cfg(target_arch = "riscv64")]
        {
            let v: usize;
            // SAFETY: csrrci reads-and-clears sstatus.SIE (bit 1) atomically.
            core::arch::asm!("csrrci {}, sstatus, 0x2", out(reg) v);
            Self(v & 0x2 != 0)
        }
        #[cfg(not(target_arch = "riscv64"))]
        Self(false)
    }
}

impl Drop for SieGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: restoring SIE to the value saved in disable(); S-mode only.
            #[cfg(target_arch = "riscv64")]
            unsafe {
                core::arch::asm!("csrsi sstatus, 0x2");
            }
        }
    }
}

/// Call the registered VFS handler directly, bypassing the `ecall` trap. Returns
/// bytes written into `out`, or 0 if no handler is registered (caller falls back
/// to the `sys_send`/`sys_recv` path).
///
/// The stable symbol name is retained for the reviewed bridge design; no client
/// Cell import resolves here today.
///
/// # Note (PIE limitation)
/// For non-PIE cells (current default), each cell ELF links `libs/ostd` statically
/// and gets its own copy of `VFS_HANDLER_PTR` — so `call_vfs` in the shell reads
/// null and always takes the ecall fallback.  The fast path becomes effective once
/// cells are compiled as PIE and a reviewed loader import bridge resolves this
/// kernel function. The fallback is always safe; that bridge is absent today.
///
/// # Caller identity
/// This path does not go through `ecall`, so the request carries no sender tid
/// and the handler has nothing of its own to authorize against. The identity is
/// therefore derived HERE, from live scheduler state for the task currently
/// running on this hart — never from an argument, because every argument on this
/// path is chosen by the cell being authorized. That makes the fast path exactly
/// as attested as the message path; without it, gating `GetFile` on the message
/// path would only move the hole rather than close it.
///
/// `TrustedHandle` is not the control: its own contract says it is advisory.
///
/// # Safety
/// The caller must own `out` exclusively for the call. `_handle` documents that
/// the caller was granted fast-path access; it is not enforced at runtime.
#[no_mangle]
pub unsafe extern "Rust" fn call_vfs(
    _handle: TrustedHandle<VfsCell>,
    req: &VfsRequest<'_>,
    out: &mut [u8; IPC_BUF_SIZE],
) -> usize {
    let ptr = VFS_HANDLER_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        return 0; // VFS not yet registered — caller falls back to ecall path.
    }
    // Resolve identity BEFORE disabling interrupts: it takes the scheduler lock,
    // and holding that lock across the handler would deadlock the VFS backends.
    let caller = crate::task::syscall::attested_identity_of(crate::task::current_task_id());

    // SAFETY: ptr was stored by register_vfs from a valid VfsFastHandler.
    let handler: VfsFastHandler = core::mem::transmute(ptr);

    // Disable S-mode interrupts for the handler's duration. The VFS FAT16 driver
    // holds a spinlock; timer preemption mid-handler to another VFS caller would
    // deadlock on it. SieGuard restores SIE on drop — safe even on handler panic.
    // SAFETY: called from S-mode trap handler context.
    let _sie = SieGuard::disable();

    handler(caller, req, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn handler_a(
        _: Option<CallerIdentity>,
        _: &VfsRequest<'_>,
        _: &mut [u8; IPC_BUF_SIZE],
    ) -> usize {
        1
    }

    unsafe fn handler_b(
        _: Option<CallerIdentity>,
        _: &VfsRequest<'_>,
        _: &mut [u8; IPC_BUF_SIZE],
    ) -> usize {
        2
    }

    fn owner() -> usize {
        VFS_HANDLER_CELL.load(Ordering::Acquire)
    }

    fn pointer() -> *mut () {
        VFS_HANDLER_PTR.load(Ordering::Acquire)
    }

    /// A stale owner's clear must not tear down a newer registration; the
    /// current owner's clear removes pointer and owner together.
    #[test]
    fn stale_owner_clear_preserves_newer_registration() {
        const CELL_A: usize = 0xA11;
        const CELL_B: usize = 0xB22;
        let _guard = crate::TEST_STATE_LOCK.lock();
        let saved_ptr = pointer();
        let saved_owner = owner();

        register_vfs(handler_a);
        set_vfs_handler_cell(CELL_A);
        register_vfs(handler_b);
        set_vfs_handler_cell(CELL_B);
        let b_ptr = (handler_b as VfsFastHandler) as *mut ();

        clear_vfs_if_cell(CELL_A);
        assert_eq!(owner(), CELL_B);
        assert_eq!(pointer(), b_ptr);
        assert!(is_registered_vfs_cell(CELL_B));
        assert!(!is_registered_vfs_cell(CELL_A));

        clear_vfs_if_cell(CELL_B);
        assert_eq!(owner(), 0);
        assert!(pointer().is_null());
        assert!(!is_registered_vfs_cell(CELL_B));

        VFS_HANDLER_PTR.store(saved_ptr, Ordering::Release);
        VFS_HANDLER_CELL.store(saved_owner, Ordering::Release);
    }
}
