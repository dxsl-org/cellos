//! Boot self-test for selective grant reclaim (`.agents/260712-1901` P02).
//!
//! Runtime revocation must reclaim the grants a Cell **owns** without waiting for
//! it to die, and must leave the grants it merely **received** alone. The reaper's
//! owner-side pass is that implementation; this proves the three properties the
//! revoke path depends on:
//!
//!   1. an owned grant is removed and its frames return to the allocator;
//!   2. a grant the target received — owned by another Cell — survives, still
//!      owned by that Cell;
//!   3. an owned grant protected by an in-flight pin is quarantined instead of
//!      freed, and released once the driver acknowledges (the D36 ordering: an
//!      in-flight grant is never freed behind a live pin).
//!
//! Runs in the same single-hart boot window as the other trust self-tests, after
//! `task::init()` and before `smp::start_secondaries()`. Every synthetic task and
//! grant is torn down before it returns.

use super::syscall::{handle_syscall, reclaim_owned_grants, Syscall};
use super::thread_cap_selftest::{insert, mk_task, remove};
use types::GrantPerm;

// Synthetic tids above the range the boot sequence assigns; removed before
// return, so they never collide with a real cell.
const OWNER_TID: usize = 9201;
const GRANTEE_TID: usize = 9202;
const OTHER_TID: usize = 9203;
// Distinct cells, kept clear of the ids `thread_cap_selftest` uses.
const OWNER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 8) as u64;
const GRANTEE_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 9) as u64;
const OTHER_CELL: u64 = (crate::memory::cell_quota::MAX_CELLS - 10) as u64;

const PAGE_SIZE: usize = 4096;
const GRANT_SIZE: usize = PAGE_SIZE;

/// Frames currently available to the allocator.
fn free_frames() -> usize {
    crate::memory::frame::FRAME_ALLOCATOR
        .lock()
        .as_ref()
        .map(|allocator| allocator.free_frames())
        .unwrap_or(0)
}

/// Allocate a grant for `tid`, returning its id (the region's base address).
fn grant_alloc(tid: usize) -> Option<usize> {
    match handle_syscall(tid, Syscall::GrantAlloc { size: GRANT_SIZE }) {
        Ok(base) if base != 0 => Some(base),
        _ => None,
    }
}

fn share(owner_tid: usize, grant_id: usize, target_cell: u64) -> bool {
    handle_syscall(
        owner_tid,
        Syscall::GrantShare {
            grant_id,
            target_cell: target_cell as usize,
            perm: GrantPerm::ReadWrite as usize,
        },
    )
    .is_ok()
}

/// `true` while `tid` can still free `grant_id` — i.e. the entry exists and `tid`
/// owns it.
fn still_owned(tid: usize, grant_id: usize) -> bool {
    handle_syscall(tid, Syscall::GrantFree { grant_id }).is_ok()
}

/// Reclaim ownership without death, and prove the three properties above.
fn probe() -> (bool, bool, bool) {
    let (Some(owned), Some(received)) = (grant_alloc(OWNER_TID), grant_alloc(OTHER_TID)) else {
        return (false, false, false);
    };
    if !share(OWNER_TID, owned, GRANTEE_CELL) || !share(OTHER_TID, received, OWNER_CELL) {
        return (false, false, false);
    }

    let before = free_frames();
    reclaim_owned_grants(OWNER_TID);

    // 1. The owned grant is gone — its owner can no longer free it — and its
    //    frames are back with the allocator.
    let owned_reclaimed =
        !still_owned(OWNER_TID, owned) && free_frames() >= before + GRANT_SIZE / PAGE_SIZE;

    // 2. The grant the target received is untouched and still owned by its owner.
    let received_kept = still_owned(OTHER_TID, received) && !still_owned(OWNER_TID, received);

    // 3. A pinned owned grant is quarantined, not freed, until the driver
    //    acknowledges the teardown.
    let pinned_grant = grant_alloc(OWNER_TID);
    let pinned_ok = match pinned_grant {
        Some(base) if crate::memory::pin::pin(base, GRANT_SIZE, OWNER_TID).is_ok() => {
            let held = free_frames();
            reclaim_owned_grants(OWNER_TID);
            let quarantined = crate::memory::pin::quarantined_pages() > 0;
            let withheld = free_frames() == held;
            super::syscall::release_acked_frames(OWNER_TID);
            quarantined && withheld && free_frames() >= held + GRANT_SIZE / PAGE_SIZE
        }
        _ => false,
    };

    (owned_reclaimed, received_kept, pinned_ok)
}

/// Run the reclaim self-test, log one marker per property, and leave the
/// scheduler and grant tables as they were found.
pub(crate) fn self_test() -> bool {
    insert(mk_task(OWNER_TID, OWNER_CELL));
    insert(mk_task(GRANTEE_TID, GRANTEE_CELL));
    insert(mk_task(OTHER_TID, OTHER_CELL));

    let (owned, received, pinned) = probe();

    // Teardown: reclaim whatever the probe left behind, then drop the tasks.
    reclaim_owned_grants(OWNER_TID);
    reclaim_owned_grants(OTHER_TID);
    remove(OWNER_TID);
    remove(GRANTEE_TID);
    remove(OTHER_TID);

    report("GRANT-RECLAIM-OWNED", owned);
    report("GRANT-RECLAIM-RECEIVED", received);
    report("GRANT-RECLAIM-PINNED", pinned);

    owned && received && pinned
}

fn report(marker: &str, ok: bool) {
    if ok {
        log::info!("{marker}: PASS");
    } else {
        log::error!("{marker}: FAIL");
    }
}
