//! QEMU test-hook fixtures for the recoverable domain-aware user-copy
//! boundary (Spec 22, phase 03). Mirrors `domain_switch_tests`: fixtures run
//! from the boot hook section in `main.rs` and publish boot-terminal markers;
//! they are never linked without `test-hooks`.
//!
//! Markers carry no copied bytes and no virtual addresses:
//!
//! - `S22-RV64-COPY: PASS harts=N` — hostile-pointer suite plus forced-fault
//!   recovery (single-hart capable)
//! - `S22-RV64-COPY-RACE: PASS harts=2` — revoke-blocks-on-reader drain proof

use super::{
    smp,
    user_copy::{copy_to_user_scatter, stage_domain_for_test, CopyError, CopyView, UserReadSlice, UserWriteSlice},
};
use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder, MappingKind};
use crate::memory::frame::phys_to_virt;
use crate::memory::paging::{Flags, PAGE_SIZE};
use alloc::{sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use types::{CellId, ViError};

const VA_A: usize = 0x0100_0000;
const VA_B: usize = VA_A + PAGE_SIZE;
/// Never mapped anywhere: peer/unmapped hostile pointer.
const VA_HOLE: usize = 0x0200_0000;
/// Mapped read-only: exercises the W-permission rejection.
const VA_RO: usize = 0x0300_0000;
const SENTINEL: u8 = 0xA5;

/// Mapped in no root the fixtures build; in Sas the probe must refuse it
/// because the kernel root has no leaf for this VA gap (512 MiB region).
const VA_GAP: usize = 0x2000_0000;

fn payload_byte(base: usize, offset: usize) -> u8 {
    (base.wrapping_add(offset) & 0xFF) as u8
}

/// Build a private root with `(va, writable)` pages plus the read-only probe
/// page, filling every byte through its physical alias so assertions have a
/// known ground truth.
fn build_domain_space(pages: &[(usize, bool)]) -> Option<Arc<AddressSpace>> {
    let mut builder = AddressSpaceBuilder::new();
    for &(virtual_address, writable) in pages {
        let bits = Flags::READ | if writable { Flags::WRITE } else { 0 };
        builder
            .map_user_page(
                virtual_address,
                MappingKind::Private,
                Flags::from_bits(bits),
            )
            .ok()?;
    }
    let space = builder.build().ok()?;
    Some(space)
}

/// Fill one mapped page through its physical alias.
fn fill_page(space: &AddressSpace, va: usize, seed: usize) {
    let Some((_, pa)) = space.page_proof_for(va) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup missing-page");
        return;
    };
    // SAFETY: the ledger entry proves this frame is exclusively owned by this
    // address space while the fixture holds the Arc.
    unsafe {
        for offset in 0..PAGE_SIZE {
            (phys_to_virt(pa + offset) as *mut u8).write_volatile(payload_byte(seed, offset));
        }
    }
}

/// Read one byte through a page's physical alias.
fn peek_byte(space: &AddressSpace, va: usize, offset_in_page: usize) -> Option<u8> {
    let (_, pa) = space.page_proof_for(va & !(PAGE_SIZE - 1))?;
    // SAFETY: exclusive fixture ownership as above.
    Some(unsafe { (phys_to_virt(pa + (va & (PAGE_SIZE - 1)) + offset_in_page) as *const u8).read_volatile() })
}

fn sentinel_dst() -> Vec<u8> {
    vec![SENTINEL; 256]
}

fn dst_is_sentinel(dst: &[u8]) -> bool {
    dst.iter().all(|&byte| byte == SENTINEL)
}

/// Hostile pointers, permission edges, cross-page movement, output atomicity,
/// forced-fault recovery. Single-hart safe.
fn run_copy_fixture() -> bool {
    let Some(space) =
        build_domain_space(&[(VA_A, true), (VA_B, true), (VA_RO, false)])
    else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup space");
        return false;
    };
    fill_page(&space, VA_A, 0x1000);
    fill_page(&space, VA_B, 0x2000);
    fill_page(&space, VA_RO, 0x3000);
    let view = CopyView::Domain(space.clone());

    // ── Slice construction rejections (no execution at all) ──
    if UserReadSlice::new(0, 8, false).is_ok()
        || UserWriteSlice::new(usize::MAX - 8, 16, false).is_ok()
        || UserReadSlice::new(1 << 39, 8, false).is_ok()
        || UserWriteSlice::new((1usize << 38) - 4, 16, false).is_ok()
    {
        log::error!("S22-RV64-COPY: FAIL slice-construction accepted-hostile");
        return false;
    }
    if UserReadSlice::new(0, 0, true).is_err() {
        log::error!("S22-RV64-COPY: FAIL slice-construction rejected-empty");
        return false;
    }

    // ── Unmapped / peer pointer: probe-pass rejection, destination untouched ──
    let mut dst = sentinel_dst();
    let Ok(slice) = UserReadSlice::new(VA_HOLE, 64, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup hole-slice");
        return false;
    };
    if super::user_copy::copy_from_user(&view, slice, &mut dst)
        != Err(CopyError::InvalidAddress)
        || !dst_is_sentinel(&dst)
    {
        log::error!("S22-RV64-COPY: FAIL unmapped-read");
        return false;
    }

    // Kernel-range style pointer inside the canonical user half but unmapped in
    // the private root: same recoverable rejection.
    let Ok(slice) = UserReadSlice::new(VA_HOLE + PAGE_SIZE, 8, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup hole-slice-2");
        return false;
    };
    if super::user_copy::copy_from_user(&view, slice, &mut dst).is_ok() {
        log::error!("S22-RV64-COPY: FAIL kernel-range-read");
        return false;
    }
    if !dst_is_sentinel(&dst) {
        log::error!("S22-RV64-COPY: FAIL atomicity-unmapped");
        return false;
    }

    // ── No-perm write: read-only page rejects copy_to_user, byte untouched ──
    let Ok(wslice) = UserWriteSlice::new(VA_RO, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup ro-slice");
        return false;
    };
    if super::user_copy::copy_to_user(&view, wslice, &[0x77u8; 32])
        != Err(CopyError::InvalidAddress)
        || peek_byte(&space, VA_RO, 7) != Some(payload_byte(0x3000, 7))
    {
        log::error!("S22-RV64-COPY: FAIL ro-write-accepted-or-corrupted");
        return false;
    }

    // ── Read-only page is still readable ──
    let Ok(rslice) = UserReadSlice::new(VA_RO, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup ro-read-slice");
        return false;
    };
    let mut ro_dst = [0u8; 32];
    match super::user_copy::copy_from_user(&view, rslice, &mut ro_dst) {
        Ok(()) => {
            for (index, byte) in ro_dst.iter().enumerate() {
                if *byte != payload_byte(0x3000, index) {
                    log::error!("S22-RV64-COPY: FAIL ro-read-payload");
                    return false;
                }
            }
        }
        Err(_) => {
            log::error!("S22-RV64-COPY: FAIL ro-read-rejected");
            return false;
        }
    }

    // ── Cross-page movement across the A/B boundary ──
    let Ok(cross) = UserReadSlice::new(VA_A + PAGE_SIZE - 100, 200, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup cross-slice");
        return false;
    };
    let mut cross_dst = [0u8; 200];
    if super::user_copy::copy_from_user(&view, cross, &mut cross_dst).is_err() {
        log::error!("S22-RV64-COPY: FAIL cross-page-read");
        return false;
    }
    for (index, byte) in cross_dst.iter().enumerate() {
        let va_offset = PAGE_SIZE - 100 + index;
        let expected = if va_offset < PAGE_SIZE {
            payload_byte(0x1000, va_offset)
        } else {
            payload_byte(0x2000, va_offset - PAGE_SIZE)
        };
        if *byte != expected {
            log::error!("S22-RV64-COPY: FAIL cross-page-payload");
            return false;
        }
    }

    // ── copy_to_user success path through the boundary ──
    let Ok(wslice) = UserWriteSlice::new(VA_A + PAGE_SIZE - 16, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup cross-write-slice");
        return false;
    };
    if super::user_copy::copy_to_user(&view, wslice, &[0x33u8; 32]).is_err() {
        log::error!("S22-RV64-COPY: FAIL cross-page-write");
        return false;
    }

    // ── copy_to_user_scatter atomicity: valid-first / invalid-later (unmapped) ──
    // Refill VA_A with its known seed. A scatter write where the first iovec is
    // valid and the second is unmapped MUST fail and leave VA_A completely untouched.
    fill_page(&space, VA_A, 0x1000);
    let Ok(scatter_valid_1) = UserWriteSlice::new(VA_A, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup scatter-valid-1");
        return false;
    };
    let Ok(scatter_hole) = UserWriteSlice::new(VA_HOLE, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup scatter-hole");
        return false;
    };
    let scatter_fail_payload = [0x99u8; 32];
    if copy_to_user_scatter(
        &view,
        &[
            (scatter_valid_1, &scatter_fail_payload),
            (scatter_hole, &scatter_fail_payload),
        ],
    ) != Err(CopyError::InvalidAddress)
    {
        log::error!("S22-RV64-COPY: FAIL scatter-unmapped-accepted");
        return false;
    }
    for index in 0..32 {
        if peek_byte(&space, VA_A, index) != Some(payload_byte(0x1000, index)) {
            log::error!("S22-RV64-COPY: FAIL scatter-unmapped-mutated-prior");
            return false;
        }
    }

    // ── copy_to_user_scatter atomicity: valid-first / invalid-later (read-only) ──
    // Scatter write where the second iovec points to read-only page VA_RO.
    // VA_A must remain completely unmutated.
    let Ok(scatter_valid_2) = UserWriteSlice::new(VA_A, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup scatter-valid-2");
        return false;
    };
    let Ok(scatter_ro) = UserWriteSlice::new(VA_RO, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup scatter-ro");
        return false;
    };
    if copy_to_user_scatter(
        &view,
        &[
            (scatter_valid_2, &scatter_fail_payload),
            (scatter_ro, &scatter_fail_payload),
        ],
    ) != Err(CopyError::InvalidAddress)
    {
        log::error!("S22-RV64-COPY: FAIL scatter-ro-accepted");
        return false;
    }
    for index in 0..32 {
        if peek_byte(&space, VA_A, index) != Some(payload_byte(0x1000, index)) {
            log::error!("S22-RV64-COPY: FAIL scatter-ro-mutated-prior");
            return false;
        }
    }

    // ── copy_to_user_scatter success path: multiple valid destinations commit ──
    let Ok(scatter_w_a) = UserWriteSlice::new(VA_A, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup scatter-w-a");
        return false;
    };
    let Ok(scatter_w_b) = UserWriteSlice::new(VA_B, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup scatter-w-b");
        return false;
    };
    let payload_a = [0x5Au8; 32];
    let payload_b = [0xA5u8; 32];
    if copy_to_user_scatter(
        &view,
        &[(scatter_w_a, &payload_a), (scatter_w_b, &payload_b)],
    )
    .is_err()
    {
        log::error!("S22-RV64-COPY: FAIL scatter-all-valid-rejected");
        return false;
    }
    for index in 0..32 {
        if peek_byte(&space, VA_A, index) != Some(0x5A)
            || peek_byte(&space, VA_B, index) != Some(0xA5)
        {
            log::error!("S22-RV64-COPY: FAIL scatter-payload-mismatch");
            return false;
        }
    }

    // ── Forced-fault recovery: protocol-violation injection between the two
    // passes leaves the destination byte-identical and returns the recoverable
    // ABI error. The public API cannot produce this interleaving because the
    // reader pin blocks revocation; the injection bypasses the revoke
    // protocol deliberately. ──
    let fault_space =
        build_domain_space(&[(VA_A, true)]).expect("forced-fault fixture space");
    fill_page(&fault_space, VA_A, 0x4000);
    let mut ff_dst = sentinel_dst();
    let Ok(staged) = stage_domain_for_test(&fault_space, VA_A, 64, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup staged-probe");
        return false;
    };
    if fault_space
        .force_unmap_without_drain_for_test(VA_A)
        .is_err()
    {
        log::error!("S22-RV64-COPY: FAIL fixture-setup injection");
        return false;
    }
    if staged.commit(ff_dst.as_mut_ptr()) != Err(CopyError::InvalidAddress)
        || !dst_is_sentinel(&ff_dst)
    {
        log::error!("S22-RV64-COPY: FAIL forced-fault-not-recoverable");
        return false;
    }

    // ── Genuine guard-fault recovery: the injection above is caught by the
    // sticky PTE re-check before any byte moves, proving output atomicity.
    // This probe additionally drives a REAL page fault through an armed
    // window so the full trap path (hook claim + sepc rewind + landing pad)
    // is exercised end-to-end. ──
    if !super::user_copy::forced_guard_fault_recovers_for_test(VA_GAP) {
        log::error!("S22-RV64-COPY: FAIL guard-fault-trap-not-recovered");
        return false;
    }

    // ── Control: an identical transaction WITHOUT the injection commits. ──
    let control_space = build_domain_space(&[(VA_A, true)]).expect("control fixture space");
    fill_page(&control_space, VA_A, 0x5000);
    let Ok(control_slice) = UserReadSlice::new(VA_A, 64, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup control-slice");
        return false;
    };
    let mut control_dst = [0u8; 64];
    if super::user_copy::copy_from_user(
        &CopyView::Domain(control_space.clone()),
        control_slice,
        &mut control_dst,
    )
    .is_err()
    {
        log::error!("S22-RV64-COPY: FAIL control-copy-rejected");
        return false;
    }
    for (index, byte) in control_dst.iter().enumerate() {
        if *byte != payload_byte(0x5000, index) {
            log::error!("S22-RV64-COPY: FAIL control-payload");
            return false;
        }
    }

    // ── Sas view round-trip on a genuine USER-mapped page in KERNEL_ROOT. ──
    const SAS_USER_VA: usize = 0x0500_0000;
    let sas_frame = {
        let mut guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let alloc = guard.as_mut().expect("frame allocator");
        let frame = alloc.allocate_frame().expect("allocate frame for sas test");
        let flags = Flags::from_bits(Flags::VALID | Flags::USER | Flags::READ | Flags::WRITE);
        crate::memory::paging::map_page(alloc, SAS_USER_VA, frame, flags).expect("map sas user page");
        crate::hal::paging::flush_tlb_page(SAS_USER_VA);
        frame
    };

    let Ok(sas_w) = UserWriteSlice::new(SAS_USER_VA, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup sas-write-slice");
        return false;
    };
    if super::user_copy::copy_to_user(&CopyView::Sas, sas_w, &[0x42u8; 32]).is_err() {
        log::error!("S22-RV64-COPY: FAIL sas-roundtrip-write");
        return false;
    }
    let Ok(sas_r) = UserReadSlice::new(SAS_USER_VA, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup sas-read-slice");
        return false;
    };
    let mut sas_back = [0u8; 32];
    if super::user_copy::copy_from_user(&CopyView::Sas, sas_r, &mut sas_back).is_err()
        || sas_back != [0x42u8; 32]
    {
        log::error!("S22-RV64-COPY: FAIL sas-roundtrip-read");
        return false;
    }

    // Clean up the mapped test page.
    let _ = crate::memory::paging::unmap_page(SAS_USER_VA);
    crate::hal::paging::flush_tlb_page(SAS_USER_VA);
    if let Some(alloc) = crate::memory::frame::FRAME_ALLOCATOR.lock().as_mut() {
        alloc.deallocate_frame(sas_frame);
    }

    // ── Sas privilege-escalation rejection: S-only identity-mapped RAM ──
    // A kernel stack or identity-mapped RAM pointer lacks the Sv39 USER flag (U=0).
    // The probe MUST reject it before any byte moves; destination must remain sentinel.
    let mut kernel_s_buf = [0u8; 32];
    let Ok(s_only_slice) = UserReadSlice::new(kernel_s_buf.as_mut_ptr() as usize, 32, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup s-only-slice");
        return false;
    };
    let mut s_fail_dst = sentinel_dst();
    if super::user_copy::copy_from_user(&CopyView::Sas, s_only_slice, &mut s_fail_dst)
        != Err(CopyError::InvalidAddress)
        || !dst_is_sentinel(&s_fail_dst)
    {
        log::error!("S22-RV64-COPY: FAIL sas-s-only-not-rejected");
        return false;
    }

    // ── Sas unmapped VA gap rejection ──
    let Ok(sas_gap) = UserReadSlice::new(VA_GAP, 16, false) else {
        log::error!("S22-RV64-COPY: FAIL fixture-setup sas-gap-slice");
        return false;
    };
    let mut sas_gap_dst = sentinel_dst();
    if super::user_copy::copy_from_user(&CopyView::Sas, sas_gap, &mut sas_gap_dst)
        != Err(CopyError::InvalidAddress)
        || !dst_is_sentinel(&sas_gap_dst)
    {
        log::error!("S22-RV64-COPY: FAIL sas-gap-not-rejected");
        return false;
    }

    true
}

// ─── COPY-RACE: revoke blocks on the held CopyReader until drain ────────────

const RACE_VA_A: usize = 0x0400_0000;
const RACE_VA_B: usize = RACE_VA_A + PAGE_SIZE;
const RACE_CELL: u64 = 91_201;

static RACE_SPACE: crate::sync::Spinlock<Option<Arc<AddressSpace>>> =
    crate::sync::Spinlock::new(None);
static RACE_GO: AtomicUsize = AtomicUsize::new(0);
static RACE_DONE: AtomicUsize = AtomicUsize::new(0);
static RACE_READERS_SEEN: AtomicUsize = AtomicUsize::new(0);
static RACE_UNMAP_OK: AtomicUsize = AtomicUsize::new(0);
static RACE_WORKER_TID: AtomicUsize = AtomicUsize::new(0);

extern "C" fn race_revoker_entry() -> ! {
    let worker_tid = RACE_WORKER_TID.load(Ordering::Acquire);
    let Some(space) = RACE_SPACE.lock().clone() else {
        loop {
            core::hint::spin_loop();
        }
    };
    while RACE_GO.load(Ordering::Acquire) == 0 {
        core::hint::spin_loop();
    }
    // The hart-0 fixture already holds its staged reader, so the count MUST be
    // positive here; the unmap below can only complete once that reader drains.
    RACE_READERS_SEEN.store(space.copy_reader_count(), Ordering::Release);
    // Distinct, [selftest]-prefixed, NON-terminal progress line: only the
    // boot hart may emit the terminal COPY-RACE markers, and this line proves
    // in the run log that the revoker reached the drain handshake on HART_RT.
    log::info!("[selftest] S22-RV64-COPY-RACE: stage=revoker-readers-observed");
    RACE_UNMAP_OK.store(
        usize::from(space.unmap_private_page(RACE_VA_B).is_ok()),
        Ordering::Release,
    );
    RACE_DONE.store(1, Ordering::Release);
    // Self-retire so hart 1 returns to its idle context instead of spinning in
    // a dead task for the rest of the boot window.
    if let Some(scheduler) = super::SCHEDULER.lock().as_mut() {
        scheduler.exit_task(worker_tid, 0);
    }
    super::yield_cpu();
    loop {
        core::hint::spin_loop();
    }
}

fn run_race_fixture() -> bool {
    if !smp::is_rt_hart_online() {
        log::warn!("[selftest] S22-RV64-COPY-RACE: RUNTIME-GATED (hart 1 offline)");
        return true;
    }
    let Some(space) = build_domain_space(&[(RACE_VA_A, true), (RACE_VA_B, true)]) else {
        log::error!("S22-RV64-COPY-RACE: FAIL fixture-setup space");
        return false;
    };
    fill_page(&space, RACE_VA_A, 0x6000);
    fill_page(&space, RACE_VA_B, 0x7000);

    *RACE_SPACE.lock() = Some(space.clone());
    RACE_GO.store(0, Ordering::Relaxed);
    RACE_DONE.store(0, Ordering::Relaxed);
    RACE_READERS_SEEN.store(0, Ordering::Relaxed);
    RACE_UNMAP_OK.store(0, Ordering::Relaxed);

    // Stage the copy transaction (probe + pin) BEFORE waking the revoker so
    // its observed reader count is deterministically positive and the reader
    // pin is held across the revoker's unmap attempt.
    let staged_copy = match stage_domain_for_test(&space, RACE_VA_A + PAGE_SIZE - 64, 128, false) {
        Ok(copy) => copy,
        Err(_) => {
            log::error!("S22-RV64-COPY-RACE: FAIL fixture-setup staged-copy");
            return false;
        }
    };

    let spawn_result = {
        let mut guard = super::SCHEDULER.lock();
        let Some(scheduler) = guard.as_mut() else {
            log::error!("S22-RV64-COPY-RACE: FAIL setup scheduler");
            return false;
        };
        let spawned = (|| -> Result<usize, ViError> {
            let kernel_stack = super::stack::Stack::new_kernel(1)?;
            let user_stack = super::stack::Stack::new_user(1)?;
            Ok(scheduler.spawn_with_stacks(
                "user-copy-revoker",
                CellId(RACE_CELL),
                Vec::new(),
                kernel_stack,
                user_stack,
            ))
        })();
        let Ok(worker_tid) = spawned else {
            log::error!("S22-RV64-COPY-RACE: FAIL setup spawn");
            return false;
        };
        super::hart_local::ready::remove_from_all(worker_tid);
        let priority = {
            let Some(worker) = scheduler.tasks.get_mut(&worker_tid) else {
                log::error!("S22-RV64-COPY-RACE: FAIL setup worker-lost");
                return false;
            };
            worker.context.ra = race_revoker_entry as *const () as usize;
            worker.priority
        };
        if !super::hart_local::ready::reserve_test_dispatch_on_hart(smp::HART_RT, worker_tid) {
            log::error!("S22-RV64-COPY-RACE: FAIL setup dispatch-pin");
            return false;
        }
        super::hart_local::ready::push_on_hart(smp::HART_RT, worker_tid, priority);
        worker_tid
    };
    RACE_WORKER_TID.store(spawn_result, Ordering::Release);

    let Some((mask, base)) = smp::logical_sbi_target(smp::HART_RT) else {
        log::error!("S22-RV64-COPY-RACE: FAIL setup dispatch-target");
        return false;
    };
    if crate::hal::common::sbi::sbi_send_ipi(mask, base).is_err() {
        log::error!("S22-RV64-COPY-RACE: FAIL setup dispatch-ipi");
        return false;
    }

    RACE_GO.store(1, Ordering::Release);

    // Deterministic interleaving barrier: do not commit anything until the
    // revoker has OBSERVED the staged reader. Without this handshake the boot
    // hart can finish the copy and drop the staged lease before HART_RT is
    // first scheduled; the revoker then records count 0 (flags bit 2) and its
    // drain spin passes vacuously — proving nothing about revoke ordering.
    for _ in 0..400_000_000usize {
        if RACE_READERS_SEEN.load(Ordering::Acquire) >= 1 {
            break;
        }
        core::hint::spin_loop();
    }
    // Commit a cross-page copy while the revoker spins inside unmap's drain
    // wait. Payload integrity proves page B was alive for the whole window.
    let mut race_dst = [0u8; 128];
    let copy_result = staged_copy.commit(race_dst.as_mut_ptr());

    let mut drained = false;
    for _ in 0..400_000_000usize {
        if RACE_DONE.load(Ordering::Acquire) == 1 {
            drained = true;
            break;
        }
        core::hint::spin_loop();
    }
    // Terminal-marker hygiene: the console writer has no cross-hart line
    // lock, so let the revoker fully retire and hart 1 finish emitting its
    // last switch-boundary line BEFORE this hart logs any COPY-RACE marker.
    for _ in 0..400_000_000usize {
        if super::hart_local::ready::current_task_id_for(smp::HART_RT) == 0
            && !super::hart_local::ready::any_hart_running(spawn_result)
        {
            break;
        }
        core::hint::spin_loop();
    }
    // Settle margin: ownership publication precedes the boundary log write,
    // so quiescence alone does not prove the UART drained. A bounded spin is
    // orders of magnitude longer than one formatted line.
    for _ in 0..20_000_000usize {
        core::hint::spin_loop();
    }
    let payload_ok = copy_result.is_ok()
        && race_dst.iter().enumerate().all(|(index, byte)| {
            let va_offset = PAGE_SIZE - 64 + index;
            let expected = if va_offset < PAGE_SIZE {
                payload_byte(0x6000, va_offset)
            } else {
                payload_byte(0x7000, va_offset - PAGE_SIZE)
            };
            *byte == expected
        });
    let readers_seen = RACE_READERS_SEEN.load(Ordering::Acquire) >= 1;
    let unmap_ok = RACE_UNMAP_OK.load(Ordering::Acquire) == 1;
    let drained_all = space.copy_reader_count() == 0;
    let ledger_ok = space.ledger().len() == 1;
    let ok = drained && payload_ok && readers_seen && unmap_ok && drained_all && ledger_ok;
    if !ok {
        // Diagnostic bitfield only — markers never carry bytes or addresses.
        let flags = u8::from(!drained)
            | u8::from(!payload_ok) << 1
            | u8::from(!readers_seen) << 2
            | u8::from(!unmap_ok) << 3
            | u8::from(!drained_all) << 4
            | u8::from(!ledger_ok) << 5;
        log::error!("S22-RV64-COPY-RACE: FAIL flags={flags:#04x}");
    }
    let _ = RACE_SPACE.lock().take();
    ok
}

/// Boot hook entry. Emits exactly one COPY marker per boot; the RACE marker is
/// emitted only when hart 1 is online (two-hart runner cases).
pub(crate) fn run_primary() {
    let copy_ok = run_copy_fixture();
    if copy_ok {
        log::info!(
            "S22-RV64-COPY: PASS harts={}",
            super::smp::online_hart_count()
        );
    } else {
        log::error!("S22-RV64-COPY: FAIL harts={}", super::smp::online_hart_count());
    }
    let race_ok = run_race_fixture();
    if race_ok && smp::is_rt_hart_online() {
        log::info!("S22-RV64-COPY-RACE: PASS harts=2");
    } else if !race_ok {
        log::error!("S22-RV64-COPY-RACE: FAIL harts=2");
    }
}
