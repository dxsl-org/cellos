//! H-27: `GrantSlice` a2 (`expected_owner_tid`) host coverage.
//!
//! A grantee such as the compositor sees every grant shared to it, so before
//! this ABI a client could attach a buffer another client shared but had not
//! yet attached. These cases pin the frozen semantics: a nonzero expectation
//! that does not name the owner answers exactly as an unknown id, the owner
//! (or zero) resolves as before, and both grant tables honor it.

use super::{grant_table_lock, handle_syscall, map_syscall, reg_grant_table_lock, Syscall};
use crate::memory::region::{Region, RegionOwner, RegionTable};
use crate::sched::scheduler::Scheduler;
use crate::task::{tcb::Task, SCHEDULER};
use abi::syscall::ViSyscall;
use types::{CellId, GrantPerm};

const GRANTEE_TID: usize = 52_001;
const OWNER_TID: usize = 52_002;
const OTHER_TID: usize = 52_003;
const PAGE_BASE: usize = 0x5a10_0000;
const REG_BASE: usize = 0x5a20_0000;
const UNKNOWN_ID: usize = 0x5a30_0000;
const SIZE: usize = 4096;

fn slice(grant_id: usize, size_out_ptr: usize, expected_owner_tid: usize) -> usize {
    handle_syscall(
        GRANTEE_TID,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr,
            expected_owner_tid,
            flags: 0,
        },
    )
    .expect("a SAS GrantSlice refusal is the usize::MAX sentinel, never an error")
}

/// An owner row at `base`, shared as `shared_to`. Its run names
/// synthetic frames (a SAS slice only returns `base`); the fixture discards it.
fn shared_row(base: usize, shared_to: Option<(usize, GrantPerm)>) -> Region {
    let mut row = Region::grant(
        RegionOwner::task(OWNER_TID, CellId(72), 1),
        super::grant_run(base, SIZE / 4096),
        SIZE,
        None,
    );
    row.set_shared_to(shared_to);
    row
}

/// Run `f` with a live SAS grantee task and one row shared to it in each
/// grant table ([`shared_row`]).
fn with_shared_rows(f: impl FnOnce()) {
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    let task = Task::new(
        GRANTEE_TID,
        CellId(71),
        "grant-owner-test",
        alloc::vec::Vec::new(),
    );
    scheduler
        .tasks
        .insert(GRANTEE_TID, alloc::boxed::Box::new(task));
    *SCHEDULER.lock() = Some(scheduler);

    let shared_to = Some((GRANTEE_TID, GrantPerm::ReadOnly));
    grant_table_lock()
        .lock()
        .get_or_insert_with(RegionTable::new)
        .insert(PAGE_BASE, shared_row(PAGE_BASE, shared_to));
    reg_grant_table_lock()
        .lock()
        .get_or_insert_with(RegionTable::new)
        .insert(REG_BASE, shared_row(REG_BASE, shared_to));

    f();

    let page_row = grant_table_lock()
        .lock()
        .as_mut()
        .and_then(|map| map.remove(&PAGE_BASE));
    page_row.map(Region::discard_synthetic);
    let reg_row = reg_grant_table_lock()
        .lock()
        .as_mut()
        .and_then(|map| map.remove(&REG_BASE));
    reg_row.map(Region::discard_synthetic);
    *SCHEDULER.lock() = saved;
}

#[test]
fn grant_slice_decodes_a2_as_expected_owner() {
    match map_syscall(ViSyscall::GrantSlice as usize, 7, 0x8000, OWNER_TID, 0) {
        Some(Syscall::GrantSlice {
            grant_id: 7,
            size_out_ptr: 0x8000,
            expected_owner_tid: OWNER_TID,
            flags: 0,
        }) => {}
        other => panic!("GrantSlice decoded as {other:?}"),
    }
}

#[test]
fn non_owner_expectation_is_indistinguishable_from_unknown_id() {
    with_shared_rows(|| {
        let unknown = slice(UNKNOWN_ID, 0, OTHER_TID);
        assert_eq!(unknown, usize::MAX);
        for base in [PAGE_BASE, REG_BASE] {
            assert_eq!(slice(base, 0, OTHER_TID), unknown, "{base:#x}");
            // An invalid output slot is never inspected for an unknown id; a
            // mismatch must not reveal the row by failing on it either.
            assert_eq!(slice(base, 1, OTHER_TID), slice(UNKNOWN_ID, 1, OTHER_TID));
        }
    });
}

#[test]
fn owner_expectation_and_zero_resolve_the_shared_grant() {
    with_shared_rows(|| {
        for base in [PAGE_BASE, REG_BASE] {
            assert_eq!(slice(base, 0, OWNER_TID), base, "{base:#x}");
            assert_eq!(slice(base, 0, 0), base, "{base:#x}");
        }
    });
}
