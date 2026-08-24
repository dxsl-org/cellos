//! Per-hart recoverable-fault guard: window arming, the guarded byte-copy
//! core, and the trap-facing glue contract. The RV64 trap handler routes
//! guard-owned page faults here by rewinding `sepc` to the landing pad
//! published in `user_copy_guard_resume_pc`.

use crate::task::hart_local;
use super::range::CopyError;
use core::sync::atomic::Ordering;

/// Run one byte-copy chunk inside the per-hart recoverable fault window.
///
/// `guard_lo`/`guard_hi` describe the faulting-address window published to the
/// trap hook: Sas commits publish the user range (faulting `stval` is the user
/// VA); Domain commits publish the kernel-linear alias range actually touched.
pub(super) fn commit_guarded(
    src: *const u8,
    dst: *mut u8,
    len: usize,
    guard_lo: usize,
    guard_hi: usize,
) -> Result<(), CopyError> {
    if len == 0 {
        return Ok(());
    }
    let window = GuardWindow::arm(guard_lo, guard_hi);
    let hart = unsafe { hart_local::current_hart() };
    // SAFETY: `hart` is the executing hart's static ViHartLocal; the atomic is
    // USIZE-repr and only this hart touches its own slot, so publishing the
    // landing-pad address through a plain store is race-free.
    let slot = core::ptr::addr_of!(hart.user_copy_guard_resume_pc) as *mut usize;
    // SAFETY: the guard window is armed around the call; see
    // guarded_byte_copy for the register/trap contract.
    let failed = unsafe { guarded_byte_copy(src, dst, len, slot) };
    drop(window);
    if failed == 0 {
        Ok(())
    } else {
        Err(CopyError::InvalidAddress)
    }
}

/// Arm the per-hart recoverable-fault window. Interrupts are masked FIRST so a
/// timer preempting mid-window can never switch tasks with the guard set; the
/// active flag publishes last (Release) so the trap hook never sees an armed
/// guard with an unpublished range.
struct GuardWindow {
    saved_sstatus: usize,
}

impl GuardWindow {
    fn arm(lo: usize, hi: usize) -> Self {
        let hart = unsafe { hart_local::current_hart() };
        hart.user_copy_guard_start.store(lo, Ordering::Relaxed);
        hart.user_copy_guard_end.store(hi, Ordering::Relaxed);
        hart.user_copy_guard_resume_pc.store(0, Ordering::Relaxed);
        let saved_sstatus = crate::hal::arch::save_and_disable_interrupts();
        let sum_bit: usize = 1 << 18;
        unsafe {
            core::arch::asm!(
                "csrs sstatus, {sum}",
                sum = in(reg) sum_bit,
                options(nomem, nostack),
            );
        }
        hart.user_copy_guard_active.store(1, Ordering::Release);
        Self { saved_sstatus }
    }
}

impl Drop for GuardWindow {
    fn drop(&mut self) {
        let hart = unsafe { hart_local::current_hart() };
        hart.user_copy_guard_active.store(0, Ordering::Release);
        // SAFETY: paired with save_and_disable_interrupts in arm(); no lock,
        // allocation, or callback ran while interrupts were masked.
        unsafe { crate::hal::arch::restore_sstatus(self.saved_sstatus) };
    }
}

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
#[inline(never)]
unsafe fn guarded_byte_copy(
    src: *const u8,
    dst: *mut u8,
    len: usize,
    resume_slot: *mut usize,
) -> usize {
    let ret;
    // SAFETY: see the function contract above.
    unsafe {
        core::arch::asm!(
            "la   {tmp}, 3f",
            "sd   {tmp}, 0({slot})",
            "li   {idx}, 0",
            "2:",
            "bgeu {idx}, {cnt}, 1f",
            "add  {sa}, {sptr}, {idx}",
            "lbu  {byte}, 0({sa})",
            "add  {da}, {dptr}, {idx}",
            "sb   {byte}, 0({da})",
            "addi {idx}, {idx}, 1",
            "j    2b",
            "1:",
            "li   {out}, 0",
            "j    4f",
            "3:",
            "li   {out}, 1",
            "4:",
            "sd   zero, 0({slot})",
            sptr = in(reg) src,
            dptr = in(reg) dst,
            cnt = in(reg) len,
            slot = in(reg) resume_slot,
            tmp = out(reg) _,
            idx = out(reg) _,
            sa = out(reg) _,
            da = out(reg) _,
            byte = out(reg) _,
            out = lateout(reg) ret,
            options(nostack)
        );
    }
    ret
}

/// Defense-in-depth: a hart switching tasks must never carry an armed guard
/// into the next task. The guarded window masks interrupts, so in the current
/// design this can only fire if a future change reintroduces preemption
/// mid-window; clearing costs one store either way.
pub(crate) fn clear_guard_for_context_switch() {
    let hart = unsafe { hart_local::current_hart() };
    hart.user_copy_guard_active.store(0, Ordering::Release);
}

/// TEST HOOK: fire one genuine guard-recovered page fault through the full
/// trap path — asm landing pad, `vi_user_copy_guard_fault` claim, `sepc`
/// rewind — by loading from a kernel-linear alias of an unmapped physical
/// hole inside an armed window. Returns whether the landing pad reported the
/// recovered fault and cleared its own resume slot.
#[cfg(feature = "test-hooks")]
pub(crate) fn forced_guard_fault_recovers_for_test(hole_pa: usize) -> bool {
    use crate::memory::paging::PAGE_SIZE;
    let hole_va = crate::memory::frame::phys_to_virt(hole_pa);
    let mut sink = [0u8; 8];
    let window = GuardWindow::arm(hole_va, hole_va + PAGE_SIZE);
    let hart = unsafe { hart_local::current_hart() };
    let slot = core::ptr::addr_of!(hart.user_copy_guard_resume_pc) as *mut usize;
    // SAFETY: the window is armed around the call; the load faults on purpose
    // and recovery resumes at the landing pad without writing `sink`.
    let failed = unsafe { guarded_byte_copy(hole_va as *const u8, sink.as_mut_ptr(), 8, slot) };
    drop(window);
    failed == 1 && hart.user_copy_guard_resume_pc.load(Ordering::Acquire) == 0
}
