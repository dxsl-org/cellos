//! Kernel-owned fast-IPC dispatch table — the single canonical instance.
//!
//! In a Single Address Space there is no privilege wall between Cells: a trusted
//! Cell calling a service handler is just an indirect call (~3 cycles) versus
//! ~100+ for an `ecall` round-trip. For the fast path to work, ONE handler
//! pointer must be shared by the VFS cell (which registers it), client cells
//! (which call it), and the kernel (which nulls it if VFS faults).
//!
//! Because Cells are separately-loaded ELFs (each with its own copy of any
//! `static`), the shared instance cannot live in a per-cell library — it lives
//! HERE, in the kernel. Cells reach `register_vfs`/`call_vfs` by name through the
//! loader's global-symbol-table resolution (see `loader::dynsym`); the kernel
//! uses `set_vfs_handler_cell`/`clear_vfs_if_cell` directly.
//!
//! ## Safety invariant
//! The handler pointer is published once at VFS startup (before any client call)
//! with `Release` ordering, read with `Acquire`, and only ever nulled on VFS
//! fault. Single-hart QEMU: no concurrent modification.

use api::fast_ipc::{TrustedHandle, VfsCell};
use api::ipc::{VfsRequest, IPC_BUF_SIZE};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Signature of a registered VFS fast-IPC handler: read `req`, write the
/// response into `out`, return the number of bytes written.
pub type VfsFastHandler =
    unsafe fn(req: &VfsRequest<'_>, out: &mut [u8; IPC_BUF_SIZE]) -> usize;

static VFS_HANDLER_PTR: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
/// Raw CellId that registered the handler; 0 = unregistered. Lets the kernel
/// null the pointer when (and only when) that specific cell faults.
static VFS_HANDLER_CELL: AtomicUsize = AtomicUsize::new(0);

/// Register the VFS fast-IPC handler. Called once by the VFS cell at startup
/// (resolved to this kernel symbol via the loader global symbol table).
///
/// `#[no_mangle]` so a Cell's undefined `register_vfs` import resolves here.
#[no_mangle]
pub extern "Rust" fn register_vfs(handler: VfsFastHandler) {
    // SAFETY: fn-ptr → *mut () for atomic storage; recovered with the same type
    // in `call_vfs`. Published Release so the handler body is visible to Acquire readers.
    VFS_HANDLER_PTR.store(unsafe { core::mem::transmute(handler) }, Ordering::Release);
}

/// Record which cell owns the registered handler (kernel-internal; called from
/// the VFS spawn path so a later fault of that cell can null the pointer).
pub fn set_vfs_handler_cell(cell_id_raw: usize) {
    VFS_HANDLER_CELL.store(cell_id_raw, Ordering::Relaxed);
}

/// Null the handler pointer iff `cell_id_raw` is the registered owner. Called by
/// the kernel fault path so a future `call_vfs` does not jump into dead VFS code.
pub fn clear_vfs_if_cell(cell_id_raw: usize) {
    if VFS_HANDLER_CELL.load(Ordering::Relaxed) == cell_id_raw && cell_id_raw != 0 {
        VFS_HANDLER_PTR.store(core::ptr::null_mut(), Ordering::Release);
        VFS_HANDLER_CELL.store(0, Ordering::Relaxed);
    }
}

/// Call the registered VFS handler directly, bypassing the `ecall` trap. Returns
/// bytes written into `out`, or 0 if no handler is registered (caller falls back
/// to the `sys_send`/`sys_recv` path).
///
/// `#[no_mangle]` so a client Cell's undefined `call_vfs` import resolves here.
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
    // SAFETY: ptr was stored by register_vfs from a valid VfsFastHandler.
    let handler: VfsFastHandler = core::mem::transmute(ptr);

    // Disable S-mode interrupts for the handler's duration. The VFS FAT16 driver
    // holds a spinlock; timer preemption mid-handler to another VFS caller would
    // deadlock on it. This makes the fast path an atomic critical section w.r.t.
    // the scheduler.
    #[cfg(target_arch = "riscv64")]
    let sie_was_set = {
        let v: usize;
        // SAFETY: csrrci reads and clears SIE (bit 1) atomically from S-mode.
        core::arch::asm!("csrrci {}, sstatus, 0x2", out(reg) v);
        v & 0x2 != 0
    };
    #[cfg(not(target_arch = "riscv64"))]
    let sie_was_set = false;

    let result = handler(req, out);

    // SAFETY: restore SIE to its prior state; csrsi sstatus,0x2 sets bit 1.
    #[cfg(target_arch = "riscv64")]
    if sie_was_set {
        core::arch::asm!("csrsi sstatus, 0x2");
    }

    result
}

/// Resolve a kernel-exported symbol name to its runtime address — the loader's
/// "Global Symbol Table" lookup for a Cell's undefined dynamic symbols. Returns
/// `None` for names the kernel does not intentionally export.
///
/// Hand-maintained: add an arm here when a Cell is permitted to call a kernel
/// function by name. (A `static` table can't hold these — `fn as usize` is not
/// permitted in const eval — so resolution is a runtime match.)
pub fn resolve_export(name: &str) -> Option<usize> {
    match name {
        "register_vfs" => Some(register_vfs as usize),
        "call_vfs" => Some(call_vfs as usize),
        _ => None,
    }
}
