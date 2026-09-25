//! The only `unsafe` in this crate: reading the user thread pointer register and
//! touching a cell-owned TLS block through it.
//!
//! Both are unavoidable for what this cell proves — that the *register* is
//! per-task, not merely a number the kernel remembers — and both are confined to
//! this file, which is named in `scripts/unsafe-allowlist.toml` (class `c-ffi`
//! style test boundary).

/// Read the architecture's user thread pointer.
///
/// x86_64 has no unprivileged way to read `FS_BASE` without `FSGSBASE`, so the
/// caller falls back to the syscall readback there (see `main.rs`).
#[cfg(target_arch = "riscv64")]
pub fn read_thread_pointer() -> usize {
    let value: usize;
    // SAFETY: `tp` is a plain register read; it clobbers nothing.
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) value, options(nomem, nostack));
    }
    value
}

#[cfg(target_arch = "aarch64")]
pub fn read_thread_pointer() -> usize {
    let value: usize;
    // SAFETY: `TPIDR_EL0` is readable from EL0; it clobbers nothing.
    unsafe {
        core::arch::asm!("mrs {}, tpidr_el0", out(reg) value, options(nomem, nostack));
    }
    value
}

#[cfg(target_arch = "x86_64")]
pub fn read_thread_pointer() -> usize {
    0
}

/// Whether [`read_thread_pointer`] can observe the register on this architecture.
pub const CAN_READ_REGISTER: bool = !cfg!(target_arch = "x86_64");

/// Write a sentinel into the calling thread's own TLS block.
///
/// # Safety
/// `base` must be the address of a block this thread owns and that is at least
/// eight bytes long; the test passes its own heap allocation.
pub unsafe fn write_sentinel(base: usize, value: u64) {
    (base as *mut u64).write_volatile(value);
}

/// Read the sentinel back.
///
/// # Safety
/// As [`write_sentinel`]: `base` must point at this thread's own block.
pub unsafe fn read_sentinel(base: usize) -> u64 {
    (base as *const u64).read_volatile()
}

/// Write a sentinel into the calling thread's own TLS block (safe wrapper).
///
/// The caller must pass the block *it* owns; this cell allocates one block per
/// thread before it starts, so the precondition is structural rather than
/// per-call.
pub fn write_own_sentinel(base: usize, value: u64) {
    // SAFETY: `base` is the calling thread's own allocation (see above).
    unsafe { write_sentinel(base, value) }
}

/// Read the sentinel back from the calling thread's own TLS block.
pub fn read_own_sentinel(base: usize) -> u64 {
    // SAFETY: as `write_own_sentinel`.
    unsafe { read_sentinel(base) }
}
