//! Per-hart recoverable-fault guard: window arming, the guarded byte-copy
//! core, and the trap-facing glue contract. The RV64 trap handler routes
//! guard-owned page faults here by rewinding `sepc` to the landing pad
//! published in `user_copy_guard_resume_pc`.

use super::range::CopyError;
use crate::sched::hart_local;
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
    let failed = unsafe { crate::arch::user_access::guarded_byte_copy(src, dst, len, slot) };
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
        #[cfg(target_arch = "riscv64")]
        crate::arch::user_access::guard_open();
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

/// Defense-in-depth: a hart switching tasks must never carry an armed guard
/// into the next task. The guarded window masks interrupts, so in the current
/// design this can only fire if a future change reintroduces preemption
/// mid-window; clearing costs one store either way.
#[allow(dead_code)]
pub(crate) fn clear_guard_for_context_switch() {
    let hart = unsafe { hart_local::current_hart() };
    hart.user_copy_guard_active.store(0, Ordering::Release);
}

/// TEST HOOK: fire one genuine guard-recovered page fault through the full
/// trap path — asm landing pad, `vi_user_copy_guard_fault` claim, `sepc`
/// rewind — by loading from a kernel-linear alias of an unmapped physical
/// hole inside an armed window. Returns whether the landing pad reported the
/// recovered fault and cleared its own resume slot.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) fn forced_guard_fault_recovers_for_test(hole_pa: usize) -> bool {
    use crate::memory::paging::PAGE_SIZE;
    let hole_va = crate::memory::frame::phys_to_virt(hole_pa);
    let mut sink = [0u8; 8];
    let window = GuardWindow::arm(hole_va, hole_va + PAGE_SIZE);
    let hart = unsafe { hart_local::current_hart() };
    let slot = core::ptr::addr_of!(hart.user_copy_guard_resume_pc) as *mut usize;
    // SAFETY: the window is armed around the call; the load faults on purpose
    // and recovery resumes at the landing pad without writing `sink`.
    let failed = unsafe {
        crate::arch::user_access::guarded_byte_copy(
            hole_va as *const u8,
            sink.as_mut_ptr(),
            8,
            slot,
        )
    };
    drop(window);
    failed == 1 && hart.user_copy_guard_resume_pc.load(Ordering::Acquire) == 0
}
