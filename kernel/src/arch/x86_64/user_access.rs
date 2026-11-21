// SPDX-License-Identifier: MPL-2.0
//! `user_access`: the kernel's window onto user memory and user-visible
//! registers.

// The kernel sets CR4.SMAP whenever the CPU has it unless `x86-no-smap` is
// set, and its only window onto user pages (STAC/CLAC around the user-copy
// guard) is compiled with `native-domains`. Without it `copy_glue` copies
// user memory raw, which would take a kernel #PF on every SMAP CPU.
#[cfg(not(any(feature = "native-domains", feature = "x86-no-smap")))]
compile_error!(
    "x86_64 needs `native-domains` (the SMAP user-copy window) or `x86-no-smap` (SMAP off)"
);

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
        "lea  rax, [rip + 4f]",
        "mov  [{slot}], rax",
        "xor  {idx}, {idx}",
        "2:",
        "cmp  {idx}, {cnt}",
        "jae  3f",
        "mov  al, byte ptr [{sptr} + {idx}]",
        "mov  byte ptr [{dptr} + {idx}], al",
        "inc  {idx}",
        "jmp  2b",
        "3:",
        "xor  {out:e}, {out:e}",
        "jmp  5f",
        "4:",
        "mov  {out:e}, 1",
        "5:",
        "mov  qword ptr [{slot}], 0",
        sptr = in(reg) src,
        dptr = in(reg) dst,
        cnt = in(reg) len,
        slot = in(reg) resume_slot,
        idx = out(reg) _,
        out = out(reg) ret,
        out("rax") _,
        options(nostack)
    );
    ret
}

/// User-copy guard window: open SMAP user access (STAC) once the window has
/// masked interrupts. The HAL's `protect::open_user_access` is the kernel's
/// only AC setter; it is a no-op where SMAP is not active.
#[cfg(feature = "native-domains")]
pub(crate) fn guard_open() {
    crate::hal::x86_64::protect::open_user_access();
}

/// User-copy guard window: close SMAP user access (CLAC) before the window
/// unmasks interrupts. A guard fault recovered inside the window returns to
/// the landing pad with AC set again (IRETQ restores it), so the close sits
/// here, after the copy, never inside the copy loop.
#[cfg(feature = "native-domains")]
pub(crate) fn guard_close() {
    crate::hal::x86_64::protect::close_user_access();
}

/// Test hooks: whether CR4.SMAP is set on this CPU (`SMAP-WINDOW` fixture).
#[cfg(all(feature = "native-domains", feature = "test-hooks"))]
pub(crate) fn smap_active() -> bool {
    crate::hal::x86_64::protect::active().smap
}

/// Whether `base` may become the user thread pointer. WRMSR of a
/// non-canonical `IA32_FS_BASE` raises #GP in ring 0, on the set and on every
/// resume, so only a user-range (and so canonical) address is admitted.
pub(crate) fn thread_pointer_admissible(base: usize) -> bool {
    base < crate::hal::paging::USER_LIMIT
}

/// Write the architecture's user thread pointer register.
pub(crate) fn write_user_thread_pointer(base: usize) {
    const IA32_FS_BASE: u32 = 0xC000_0100;
    // SAFETY: WRMSR of IA32_FS_BASE from ring 0 is always permitted; it sets the
    // user-mode FS segment base and leaves the kernel's GS-based per-CPU state
    // (GS_BASE/KERNEL_GS_BASE) untouched.
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") IA32_FS_BASE,
            in("eax") base as u32,
            in("edx") (base >> 32) as u32,
            options(nomem, nostack)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::thread_pointer_admissible;

    #[test]
    fn only_user_range_thread_pointers_are_admitted() {
        let limit = crate::hal::paging::USER_LIMIT;
        assert!(thread_pointer_admissible(0));
        assert!(thread_pointer_admissible(limit - 1));
        assert!(!thread_pointer_admissible(limit));
        // Non-canonical: WRMSR IA32_FS_BASE would #GP in ring 0.
        assert!(!thread_pointer_admissible(0x8000_0000_0000_0000));
        assert!(!thread_pointer_admissible(0x0000_8000_0000_0000));
        // Canonical but kernel half.
        assert!(!thread_pointer_admissible(0xFFFF_8000_0000_0000));
    }
}
