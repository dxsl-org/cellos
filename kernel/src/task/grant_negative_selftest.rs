//! Domain grant deferred-revoke witness (Phase 15).
//!
//! The body is architecture-neutral — it drives the production `handle_syscall`
//! entry points with real `TaskAddressSpace::Domain` tasks — and is wired into
//! the x86_64 boot fixtures, the lane that ports the RV64/AArch64 grant suite.
//!
//! **Deferred revoke** (`{ARCH_TAG}-GRANT-DEFERRED`): while the tag
//! acknowledgement is withheld, `GrantFree` must fail closed — the record stays
//! `Revoking`, its frames are retained, no slice or share resolves — after the
//! local tag invalidation (INVPCID type 1, or a CR3 reload without PCID, on x86)
//! has run. Once the acknowledgement resumes, the owner's exit hands the row to
//! the deferred sweep, which completes the revoke and frees the frames.
//!
//! The withheld window is bracketed by `[grant-neg] withhold-ack window OPEN` /
//! `CLOSED`: the tag invalidations it reports unconfirmed are its purpose, and
//! the lane excludes only the lines between those two markers from its
//! unconfirmed-invalidation rule.
//!
//! It also reports the longest lifecycle-gate holds the boot's grant fixtures
//! caused (`[grant] gate-held max publish=… drain=…`, risk R4), since the gate
//! holder runs with interrupts masked while it waits for remote
//! acknowledgements.

use super::domain_grant::{gate_hold, ARCH_TAG};
use super::syscall::{handle_syscall, Syscall, SyscallError};
use super::tcb::TaskAddressSpace;
use super::thread_cap_selftest::{insert, mk_task, remove};
use crate::memory::address_space::{AddressSpace, AddressSpaceBuilder, MappingKind};
use crate::memory::deferred_release;
use crate::memory::paging::{Flags, PAGE_SIZE};
use crate::memory::tlb_shootdown;
use alloc::sync::Arc;
use core::sync::atomic::Ordering;
use types::GrantPerm;

// Synthetic tids and cells clear of the sibling boot fixtures, removed before
// return.
const OWNER_TID: usize = 9801;
const RECEIVER_TID: usize = 9802;
const OWNER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 30) as u64;
const RECEIVER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 31) as u64;

/// Private-root VAs for each task's own page, above every lane machine's RAM
/// (grant pages are published at VA = PA) and below `hal::paging::USER_LIMIT`.
const OWNER_VA: usize = 0x30_1000_0000;
const RECEIVER_VA: usize = 0x30_1400_0000;

const GRANT_SIZE: usize = PAGE_SIZE;
/// How long the release half waits for the reaper once the acknowledgement
/// resumes. Generous: it bounds a fixture, not a syscall.
const RELEASE_BUDGET_NS: u64 = 2_000_000_000;

/// The domain tasks the fixture runs against.
struct Fixture {
    receiver: Arc<AddressSpace>,
}

/// Withholds every tag acknowledgement for the lifetime of the value, with the
/// lane's bracket markers around it.
struct WithheldAck;

impl WithheldAck {
    fn open() -> Self {
        log::info!("[grant-neg] withhold-ack window OPEN");
        tlb_shootdown::set_test_withhold_tag_ack(true);
        Self
    }
}

impl Drop for WithheldAck {
    fn drop(&mut self) {
        tlb_shootdown::set_test_withhold_tag_ack(false);
        log::info!("[grant-neg] withhold-ack window CLOSED");
    }
}

fn space(va: usize) -> Option<Arc<AddressSpace>> {
    let mut builder = AddressSpaceBuilder::new();
    builder
        .map_user_page(
            va,
            MappingKind::Private,
            Flags::from_bits(Flags::READ | Flags::WRITE),
        )
        .ok()?;
    builder.build().ok()
}

fn free_frames() -> usize {
    crate::memory::frame::FRAME_ALLOCATOR
        .lock()
        .as_ref()
        .map(|allocator| allocator.free_frames())
        .unwrap_or(0)
}

fn grant_alloc(tid: usize) -> Option<usize> {
    match handle_syscall(tid, Syscall::GrantAlloc { size: GRANT_SIZE }) {
        Ok(base) if base != 0 => Some(base),
        _ => None,
    }
}

fn grant_share(owner: usize, grant_id: usize, target: usize, perm: GrantPerm) -> bool {
    handle_syscall(
        owner,
        Syscall::GrantShare {
            grant_id,
            target_cell: target,
            perm: perm as usize,
        },
    )
    .is_ok()
}

/// `Some(base)` when the slice resolved, `None` for the refusal sentinel or an
/// error.
fn grant_slice(tid: usize, grant_id: usize) -> Option<usize> {
    match handle_syscall(
        tid,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: 0,
            expected_owner_tid: 0,
            flags: 0,
        },
    ) {
        Ok(base) if base != usize::MAX => Some(base),
        _ => None,
    }
}

fn grant_free(tid: usize, grant_id: usize) -> Result<usize, SyscallError> {
    handle_syscall(tid, Syscall::GrantFree { grant_id })
}

/// Does `space` hold any mapping at `va`?
fn maps(space: &AddressSpace, va: usize) -> bool {
    space
        .ledger()
        .into_iter()
        .any(|entry| entry.virtual_address == va)
}

/// Allocate one grant and publish it to `receiver_tid` with `perm`, resolved.
fn shared_grant(receiver_tid: usize, receiver: &AddressSpace, perm: GrantPerm) -> Option<usize> {
    let base = grant_alloc(OWNER_TID)?;
    if grant_share(OWNER_TID, base, receiver_tid, perm)
        && grant_slice(receiver_tid, base) == Some(base)
        && maps(receiver, base)
    {
        Some(base)
    } else {
        let _ = grant_free(OWNER_TID, base);
        None
    }
}

/// Spin until `done` holds or [`RELEASE_BUDGET_NS`] passes, reaping the
/// deferred-release queue on every pass.
fn within_release_budget(mut done: impl FnMut() -> bool) -> bool {
    let Some(start) = crate::arch::clock::monotonic_ns() else {
        return false;
    };
    loop {
        deferred_release::reap_deferred_releases();
        if done() {
            return true;
        }
        match crate::arch::clock::monotonic_ns() {
            Some(now) if now.saturating_sub(start) < RELEASE_BUDGET_NS => core::hint::spin_loop(),
            _ => return false,
        }
    }
}

/// What the deferred revoke observed, in the order the marker reports it.
#[derive(Default)]
struct Deferred {
    refused: bool,
    tag_flushed: bool,
    retained: bool,
    revoking: bool,
    receiver_unmapped: bool,
    swept: bool,
    released: bool,
}

impl Deferred {
    fn all(&self) -> bool {
        self.refused
            && self.tag_flushed
            && self.retained
            && self.revoking
            && self.receiver_unmapped
            && self.swept
            && self.released
    }
}

fn deferred_revoke(fx: &Fixture) -> Deferred {
    let mut seen = Deferred::default();
    let Some(base) = shared_grant(RECEIVER_TID, &fx.receiver, GrantPerm::ReadWrite) else {
        return seen;
    };
    let receiver_tag = fx.receiver.asid();
    let (_, flushes_before) = crate::hal::domain::switch_counters();
    let free_before = free_frames();
    {
        let _window = WithheldAck::open();
        tlb_shootdown::begin_test_flush_observation();
        seen.refused = grant_free(OWNER_TID, base) == Err(SyscallError::PermissionDenied);
        tlb_shootdown::finish_test_flush_observation();
        // The local half ran — the receiver tag was invalidated through the HAL
        // — and only the acknowledgement is missing.
        seen.tag_flushed = tlb_shootdown::test_tag_flush_observed(receiver_tag)
            && crate::hal::domain::switch_counters().1 > flushes_before;
        seen.retained = free_frames() <= free_before;
        seen.revoking = grant_slice(OWNER_TID, base).is_none()
            && grant_slice(RECEIVER_TID, base).is_none()
            && !grant_share(OWNER_TID, base, RECEIVER_TID, GrantPerm::ReadWrite);
        seen.receiver_unmapped = !maps(&fx.receiver, base);
        // The owner exits while the acknowledgement is still missing: its row
        // loses its owner and waits for the deferred sweep, frames retained.
        super::syscall::reclaim_owned_grants(OWNER_TID);
        seen.retained &= free_frames() <= free_before;
    }
    // Acknowledgement resumes: the next exit's sweep completes the revoke and
    // returns the frames.
    seen.swept = within_release_budget(|| {
        super::syscall::reap_grants_for_task(OWNER_TID);
        free_frames() > free_before
    });
    seen.released = !deferred_release::tag_invalidation_unconfirmed(receiver_tag)
        && crate::memory::address_space::quarantined_frame_count() == 0;
    seen
}

/// Run the deferred revoke, then leave the scheduler and grant tables as they
/// were found and report the longest gate hold of the boot's grant fixtures.
pub(crate) fn run_primary() {
    let (Some(owner), Some(receiver)) = (space(OWNER_VA), space(RECEIVER_VA)) else {
        log::error!("{ARCH_TAG}-GRANT-DEFERRED: FAIL fixture-setup");
        return;
    };
    for (tid, cell, root) in [
        (OWNER_TID, OWNER_CELL, &owner),
        (RECEIVER_TID, RECEIVER_CELL, &receiver),
    ] {
        let mut task = mk_task(tid, cell);
        task.address_space = TaskAddressSpace::Domain(Arc::clone(root));
        insert(task);
    }
    let fx = Fixture { receiver };

    let deferred = deferred_revoke(&fx);
    super::syscall::reclaim_owned_grants(OWNER_TID);
    for tid in [OWNER_TID, RECEIVER_TID] {
        remove(tid);
    }
    if deferred.all() {
        log::info!("{ARCH_TAG}-GRANT-DEFERRED: PASS");
    } else {
        log::error!(
            "{ARCH_TAG}-GRANT-DEFERRED: FAIL refused={} tag_flushed={} retained={} revoking={} \
             receiver_unmapped={} swept={} released={}",
            deferred.refused,
            deferred.tag_flushed,
            deferred.retained,
            deferred.revoking,
            deferred.receiver_unmapped,
            deferred.swept,
            deferred.released
        );
    }
    log::info!(
        "[grant] gate-held max publish={}us drain={}us cpus={}",
        gate_hold::PUBLISH_MAX_NS.load(Ordering::Acquire) / 1_000,
        gate_hold::DRAIN_MAX_NS.load(Ordering::Acquire) / 1_000,
        crate::sched::smp::online_hart_count()
    );
}
