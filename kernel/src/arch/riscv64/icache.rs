// SPDX-License-Identifier: MPL-2.0
//! `icache`: publishing loaded code to instruction fetch on every online hart.
//!
//! RV64 instruction fetch is not coherent with stores, and the hart that
//! dequeues a freshly loaded task may not be the one that wrote its code: the
//! loader fences locally, asks every other online hart through SBI RFENCE
//! `remote_fence_i`, and falls back to an IPI epoch served with `fence.i`.

use crate::memory::paging::Flags;
use crate::sched::smp::{self, MAX_HARTS};
use core::sync::atomic::{AtomicUsize, Ordering};
use types::{VAddr, ViError, ViResult};

/// Instruction-cache sync epochs: the IPI fallback a code publisher takes when
/// SBI RFENCE `remote_fence_i` fails. Kept apart from the TLB epochs so a
/// routine invalidation never pays for an instruction-cache flush. Served on
/// the same trap path and in the same spin answers as the TLB request.
static ICACHE_SYNC_REQUEST: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];
static ICACHE_SYNC_COMPLETE: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];

/// How long the IPI fallback waits for every asked hart's `fence.i` before
/// refusing the spawn: the same 200 ms budget a TLB probe gets.
const ICACHE_SYNC_PROBE_TICKS: u64 = 20 * hal::common::timer::TICKS_PER_10MS;

/// Test-hooks: whether this boot already drove the IPI fallback once to emit
/// the lane's evidence marker.
#[cfg(feature = "test-hooks")]
static ICACHE_SYNC_WITNESSED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Make this hart's later instruction fetches observe every store it has
/// observed (RV64 does not keep instruction fetch coherent with stores).
#[inline]
fn local_fence_i() {
    // SAFETY: `fence.i` only orders this hart's later instruction fetches after
    // the stores it has observed; it writes no register or memory.
    unsafe { core::arch::asm!("fence.i", options(nostack, preserves_flags)) };
}

/// Ask `hart_id` to execute `fence.i` and return the epoch it must publish.
///
/// The Release half of the request orders the requester's code stores before
/// the target's Acquire of the epoch, so the target's `fence.i` covers them.
fn request_icache_sync(hart_id: usize) -> usize {
    if hart_id >= MAX_HARTS {
        return 0;
    }
    let epoch = ICACHE_SYNC_REQUEST[hart_id].fetch_add(1, Ordering::AcqRel) + 1;
    smp::send_ipi(hart_id);
    epoch
}

/// Is `epoch` known complete on `hart_id`? Epoch 0 means "nothing requested".
fn icache_sync_completed(hart_id: usize, epoch: usize) -> bool {
    epoch == 0
        || ICACHE_SYNC_COMPLETE
            .get(hart_id)
            .is_some_and(|complete| complete.load(Ordering::Acquire) >= epoch)
}

/// Serve one captured instruction-cache sync request on this hart. Called from
/// the TLB service path, after its `hart_id < MAX_HARTS` check.
pub(crate) fn service_sync_request(hart_id: usize) {
    let epoch = ICACHE_SYNC_REQUEST[hart_id].load(Ordering::Acquire);
    if ICACHE_SYNC_COMPLETE[hart_id].load(Ordering::Acquire) < epoch {
        local_fence_i();
        smp::publish_completion(&ICACHE_SYNC_COMPLETE[hart_id], epoch);
    }
}

/// Publish the loader's final stores to `pages` to instruction fetch on every
/// online hart, before the task can be registered: a no-op when none of them
/// is executable.
///
/// # Errors
/// `ViError::IO` when a hart asked through the fallback does not confirm.
pub(crate) fn sync_loaded_code(pages: &[(VAddr, Flags)], cell: &str) -> ViResult<()> {
    if pages
        .iter()
        .any(|(_, flags)| flags.bits() & Flags::EXECUTE != 0)
    {
        sync_instruction_caches(cell)?;
    }
    Ok(())
}

/// Make the loader's code stores fetchable on every online hart before the
/// task can be registered, so no hart can dequeue it and fetch stale bytes a
/// previous owner of the frame left in its instruction cache.
///
/// Local `fence.i`, then SBI RFENCE `remote_fence_i` to each other online hart.
/// A hart whose RFENCE fails is asked through the IPI epoch path instead and
/// must publish its own `fence.i` within the probe; a silent one refuses the
/// spawn. The set is the harts online now: a later hart never fetched these
/// bytes.
///
/// # Errors
/// `ViError::IO` when a hart asked through the fallback does not confirm.
fn sync_instruction_caches(cell: &str) -> ViResult<()> {
    #[cfg(feature = "test-hooks")]
    let mut remote = [false; MAX_HARTS];
    let mut fallback = [false; MAX_HARTS];
    let mut rfence_failed = false;
    // Masked so the local fence and "every other hart" name the same hart.
    let sstatus = hal::arch::save_and_disable_interrupts();
    let me = crate::sched::hart_local::current_hart_id();
    // Order the loader's stores before any remote hart is asked to fence.
    core::sync::atomic::fence(Ordering::SeqCst);
    local_fence_i();
    for hart in smp::online_harts().filter(|hart| *hart != me) {
        #[cfg(feature = "test-hooks")]
        {
            remote[hart] = true;
        }
        let sent = smp::logical_sbi_target(hart)
            .map(|(mask, base)| hal::sbi::sbi_remote_fence_i(mask, base));
        if !matches!(sent, Some(Ok(()))) {
            fallback[hart] = true;
            rfence_failed = true;
        }
    }
    // SAFETY: restores exactly the interrupt state captured above.
    unsafe { hal::arch::restore_sstatus(sstatus) };

    if rfence_failed {
        log::warn!(
            "[wx] '{}': RFENCE remote_fence_i failed; using the IPI fallback",
            cell
        );
    }
    // Once per boot the fallback also runs after a successful RFENCE, so the
    // lane witnesses both paths on every remote hart.
    #[cfg(feature = "test-hooks")]
    let harts = 1 + remote.iter().filter(|asked| **asked).count();
    #[cfg(feature = "test-hooks")]
    let witness = harts > 1 && !ICACHE_SYNC_WITNESSED.swap(true, Ordering::AcqRel);
    #[cfg(feature = "test-hooks")]
    if witness {
        fallback = remote;
    }

    let asked = smp::RemoteEpochs::broadcast(
        (0..MAX_HARTS).filter(|hart| fallback[*hart]),
        request_icache_sync,
    );
    let deadline = hal::common::timer::read_mtime() + ICACHE_SYNC_PROBE_TICKS;
    while let Some((hart, epoch)) = asked.first_outstanding(icache_sync_completed) {
        if hal::common::timer::read_mtime() > deadline {
            log::error!(
                "[wx] '{}': hart {} did not confirm fence.i epoch {} — refusing spawn",
                cell,
                hart,
                epoch
            );
            return Err(ViError::IO);
        }
        core::hint::spin_loop();
        // Answer peers' requests so two harts waiting on each other cannot
        // deadlock. The hart is re-read under the mask: with interrupts open
        // between passes this context may have moved, and answering for a
        // stale id would forge that hart's completion.
        let sstatus = hal::arch::save_and_disable_interrupts();
        smp::answer_tlb_flush_in_spin(crate::sched::hart_local::current_hart_id());
        // SAFETY: restores exactly the interrupt state captured above.
        unsafe { hal::arch::restore_sstatus(sstatus) };
    }

    #[cfg(feature = "test-hooks")]
    if witness {
        log::info!(
            "[wx] icache sync: local+remote PASS harts={} rfence={} ipi-fallback=PASS",
            harts,
            if rfence_failed { "failed" } else { "ok" }
        );
    }
    Ok(())
}
