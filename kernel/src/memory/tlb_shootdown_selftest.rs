//! Two-hart RV64 proof for the RFENCE completion boundary.
//!
//! Compiled only with `test-hooks`: hart 1 primes a writable translation, hart
//! 0 lowers it, then hart 1 either faults without changing the physical byte or
//! proves the negative control can still write when RFENCE is deliberately skipped.

use crate::memory::frame::FRAME_ALLOCATOR;
use crate::memory::paging::{self, Flags};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use hal::arch::ViTrapFrame;
use types::VAddr;

// Sv39 requires bits 63:39 to sign-extend bit 38; keep the oracle in the
// canonical lower half so a permission fault is not reported as access fault.
const TEST_VA: VAddr = 0x3F00_0000;
const MAGIC_PRIME: usize = 0x51A7_0001;
const MAGIC_WRITE: usize = 0x51A7_0002;
const IDLE: u8 = 0;
const POSITIVE_ARMED: u8 = 1;
const POSITIVE_PRIMED: u8 = 2;
const POSITIVE_LOWERED: u8 = 3;
const POSITIVE_DONE: u8 = 4;
const NEGATIVE_ARMED: u8 = 5;
const NEGATIVE_PRIMED: u8 = 6;
const NEGATIVE_LOWERED: u8 = 7;
const NEGATIVE_DONE: u8 = 8;
const FAILED: u8 = 9;

static PHASE: AtomicU8 = AtomicU8::new(IDLE);
static TEST_FRAME: AtomicUsize = AtomicUsize::new(0);
static EXPECT_STORE_FAULT: AtomicBool = AtomicBool::new(false);
static STORE_FAULTED: AtomicBool = AtomicBool::new(false);

fn writable_flags() -> Flags {
    Flags::from_bits(Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY)
}

fn readonly_flags() -> Flags {
    Flags::from_bits(Flags::VALID | Flags::READ | Flags::ACCESSED | Flags::DIRTY)
}

fn wait_for(expected: u8) -> bool {
    let deadline = crate::task::system_ticks() + 50;
    loop {
        match PHASE.load(Ordering::Acquire) {
            value if value == expected => return true,
            FAILED => return false,
            _ if crate::task::system_ticks() >= deadline => return false,
            _ => core::hint::spin_loop(),
        }
    }
}

unsafe fn store_test_value(value: usize) {
    // SAFETY: TEST_VA is mapped writable before this instruction executes. The
    // expected post-lowering fault is consumed only for this exact address.
    unsafe {
        core::arch::asm!(
            "sd {value}, 0({address})",
            address = in(reg) TEST_VA,
            value = in(reg) value,
            options(nostack)
        );
    }
}

fn frame_value() -> usize {
    let frame = TEST_FRAME.load(Ordering::Acquire);
    // SAFETY: the frame stays allocated until the primary finishes this probe.
    unsafe { core::ptr::read_volatile(frame as *const usize) }
}

fn set_phase(value: u8) {
    PHASE.store(value, Ordering::Release);
}

fn unmap_and_release(frame: usize) {
    let _ = paging::unmap_page(TEST_VA);
    crate::memory::tlb_shootdown::flush_page(TEST_VA);
    let mut frames = FRAME_ALLOCATOR.lock();
    if let Some(allocator) = frames.as_mut() {
        allocator.deallocate_frame(frame);
    }
}

fn fail(frame: usize, reason: &str) {
    log::error!("[selftest] TLB-SHOOTDOWN: FAIL — {}", reason);
    set_phase(FAILED);
    crate::memory::tlb_shootdown::set_test_skip_remote_rfence(false);
    unmap_and_release(frame);
}

/// Called by hart 0 after RFENCE-gated secondary startup.
pub fn run_primary() {
    if !crate::task::smp::is_rt_hart_online() {
        log::warn!("[selftest] TLB-SHOOTDOWN: RUNTIME-GATED (hart 1 offline)");
        return;
    }

    // The boot identity map may already cover this canonical VA. Remove that
    // leaf first so the oracle maps the allocated frame rather than retaining
    // the pre-existing VA-to-PA binding.
    let _ = paging::unmap_page(TEST_VA);
    crate::memory::tlb_shootdown::flush_page(TEST_VA);

    let frame = {
        let mut frames = FRAME_ALLOCATOR.lock();
        let Some(allocator) = frames.as_mut() else {
            log::error!("[selftest] TLB-SHOOTDOWN: FAIL — frame allocator unavailable");
            return;
        };
        let Some(frame) = allocator.allocate_frame() else {
            log::error!("[selftest] TLB-SHOOTDOWN: FAIL — no frame");
            return;
        };
        if paging::map_page(allocator, TEST_VA, frame, writable_flags()).is_err() {
            allocator.deallocate_frame(frame);
            log::error!("[selftest] TLB-SHOOTDOWN: FAIL — map failed");
            return;
        }
        crate::memory::tlb_shootdown::flush_page(TEST_VA);
        let translated = paging::virt_to_phys(TEST_VA).unwrap_or(0) & !(paging::PAGE_SIZE - 1);
        log::info!(
            "[selftest] TLB-SHOOTDOWN mapping va={:#x} frame={:#x} translated={:#x}",
            TEST_VA,
            frame,
            translated
        );
        if translated != frame {
            allocator.deallocate_frame(frame);
            log::error!("[selftest] TLB-SHOOTDOWN: FAIL — test VA retained another frame");
            return;
        }
        frame
    };
    // SAFETY: the freshly allocated frame is exclusively owned by this test.
    unsafe { core::ptr::write_volatile(frame as *mut usize, 0) };
    TEST_FRAME.store(frame, Ordering::Release);
    STORE_FAULTED.store(false, Ordering::Release);

    set_phase(POSITIVE_ARMED);
    if !wait_for(POSITIVE_PRIMED) || paging::protect_page(TEST_VA, readonly_flags()).is_err() {
        fail(frame, "positive setup failed");
        return;
    }
    EXPECT_STORE_FAULT.store(true, Ordering::Release);
    set_phase(POSITIVE_LOWERED);
    let positive_done = wait_for(POSITIVE_DONE);
    let store_faulted = STORE_FAULTED.load(Ordering::Acquire);
    let observed = frame_value();
    if !positive_done || !store_faulted || observed != MAGIC_PRIME {
        log::error!(
            "[selftest] positive done={} faulted={} value={:#x} phase={}",
            positive_done,
            store_faulted,
            observed,
            PHASE.load(Ordering::Acquire)
        );
        fail(
            frame,
            "post-RFENCE store did not fault without changing memory",
        );
        return;
    }

    if paging::protect_page(TEST_VA, writable_flags()).is_err() {
        fail(frame, "negative-control writable remap failed");
        return;
    }
    set_phase(NEGATIVE_ARMED);
    if !wait_for(NEGATIVE_PRIMED) {
        fail(frame, "negative-control prime timed out");
        return;
    }
    crate::memory::tlb_shootdown::set_test_skip_remote_rfence(true);
    let lowered = paging::protect_page(TEST_VA, readonly_flags()).is_ok();
    crate::memory::tlb_shootdown::set_test_skip_remote_rfence(false);
    if !lowered {
        fail(frame, "negative-control lowering failed");
        return;
    }
    set_phase(NEGATIVE_LOWERED);
    if !wait_for(NEGATIVE_DONE) || frame_value() != MAGIC_WRITE {
        fail(frame, "negative control did not prove stale write");
        return;
    }

    unmap_and_release(frame);
    log::info!(
        "[selftest] TLB-SHOOTDOWN: PASS (hart 1, RFENCE + physical oracle + negative control)"
    );
}

/// Called by the RV64 secondary before it enters its scheduler loop.
pub fn run_secondary(hart_id: usize) {
    if hart_id != crate::task::smp::HART_RT || !wait_for(POSITIVE_ARMED) {
        return;
    }
    // SAFETY: hart 0 mapped TEST_VA writable before publishing POSITIVE_ARMED.
    unsafe { store_test_value(MAGIC_PRIME) };
    set_phase(POSITIVE_PRIMED);
    if !wait_for(POSITIVE_LOWERED) {
        return;
    }
    // SAFETY: this must trap after RFENCE; the exact test-only handler skips it.
    unsafe { store_test_value(MAGIC_WRITE) };
    set_phase(POSITIVE_DONE);

    if !wait_for(NEGATIVE_ARMED) {
        return;
    }
    // SAFETY: hart 0 remapped TEST_VA writable before publishing NEGATIVE_ARMED.
    unsafe { store_test_value(MAGIC_PRIME) };
    set_phase(NEGATIVE_PRIMED);
    if !wait_for(NEGATIVE_LOWERED) {
        return;
    }
    // SAFETY: the negative control deliberately retains this hart's stale entry.
    unsafe { store_test_value(MAGIC_WRITE) };
    set_phase(NEGATIVE_DONE);
}

/// Consume only the expected probe store fault and resume after its `sd` instruction.
pub fn handle_store_fault(frame: &mut ViTrapFrame) -> bool {
    if crate::task::hart_local::current_hart_id() != crate::task::smp::HART_RT
        || frame.stval != TEST_VA
        || !EXPECT_STORE_FAULT.swap(false, Ordering::AcqRel)
    {
        return false;
    }
    STORE_FAULTED.store(true, Ordering::Release);
    frame.sepc += 4;
    true
}
