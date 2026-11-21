// SPDX-License-Identifier: MPL-2.0
//! `user_access`: the kernel's window onto user memory and user-visible
//! registers.

/// Byte-copy loop with a trap-recoverable error landing pad (local label `3`).
/// Direction-neutral: reading `src` and writing `dst` byte-by-byte covers both
/// copy directions through the boundary; the caller picks which side points
/// at user memory and publishes the matching guard range.
///
/// Register contract: the block touches ONLY caller-saved registers and never
/// changes `sp` or `ra`. When the trap hook recovers a guard fault it rewrites
/// `sepc` to label `3`; the trap epilogue then restores the fault-time register
/// image, which is exactly the state label `3` expects — it sets the return
/// value, clears the resume slot, and returns through the untouched `ra`.
///
/// # Safety
/// The guard window must be armed around the call, `resume_slot` must point at
/// this hart's `user_copy_guard_resume_pc`, and both ranges must be resolved
/// or recoverable per the module docs.
#[cfg(feature = "native-domains")]
#[inline(never)]
pub(crate) unsafe fn guarded_byte_copy(
    src: *const u8,
    dst: *mut u8,
    len: usize,
    resume_slot: *mut usize,
) -> usize {
    let ret;
    // SAFETY: see the function contract above.
    core::arch::asm!(
        "adr  {tmp}, 3f",
        "str  {tmp}, [{slot}]",
        "mov  {idx}, #0",
        "2:",
        "cmp  {idx}, {cnt}",
        "b.ge 1f",
        "ldrb {byte:w}, [{sptr}, {idx}]",
        "strb {byte:w}, [{dptr}, {idx}]",
        "add  {idx}, {idx}, #1",
        "b    2b",
        "1:",
        "mov  {out:w}, #0",
        "b    4f",
        "3:",
        "mov  {out:w}, #1",
        "4:",
        "str  xzr, [{slot}]",
        sptr = in(reg) src,
        dptr = in(reg) dst,
        cnt = in(reg) len,
        slot = in(reg) resume_slot,
        tmp = out(reg) _,
        idx = out(reg) _,
        byte = out(reg) _,
        out = out(reg) ret,
        options(nostack)
    );
    ret
}

/// Whether `base` may become the user thread pointer: `TPIDR_EL0` holds any
/// value without faulting, and a bad base only faults the cell itself.
pub(crate) fn thread_pointer_admissible(_base: usize) -> bool {
    true
}

/// Write the architecture's user thread pointer register.
pub(crate) fn write_user_thread_pointer(base: usize) {
    // SAFETY: TPIDR_EL0 is the EL0-visible thread pointer; writing it from EL1 is
    // always permitted and touches no kernel state (the kernel uses TPIDR_EL1).
    unsafe {
        core::arch::asm!("msr tpidr_el0, {0}", in(reg) base, options(nomem, nostack));
    }
}
