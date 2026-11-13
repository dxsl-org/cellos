//! `GrantSlice` a3 `LEASE` host coverage (approved 2026-11-10).
//!
//! A leased slice must hold the Grant against its owner's `GrantFree` and
//! `GrantUnregister` until the holder's next receive, its next send to the
//! owner, or its death; the owner's death must quarantine rather than free or
//! transfer the frames; and every refusal must answer exactly as an unknown id.
//! Frames are never actually freed here: every path that would return them to
//! the allocator is checked at the pin-registry boundary instead.

use super::{
    grant_table_lock, handle_syscall, map_syscall, reclaim_owned_grants, reg_grant_table_lock,
    release_vfs_holder_leases, PageGrant, RegGrant, Syscall, SyscallError,
};
use crate::memory::pin;
use crate::sched::scheduler::Scheduler;
use crate::task::{tcb::Task, SCHEDULER};
use abi::syscall::{grant_slice::LEASE, ViSyscall};
use alloc::collections::BTreeMap;
use types::{CellId, GrantPerm};

const HOLDER_TID: usize = 53_001;
const OWNER_TID: usize = 53_002;
const OTHER_TID: usize = 53_003;
const PAGE_BASE: usize = 0x5b10_0000;
const REG_BASE: usize = 0x5b20_0000;
const SIZE: usize = 4096;

fn slice(grant_id: usize, expected_owner_tid: usize, flags: usize) -> usize {
    handle_syscall(
        HOLDER_TID,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: 0,
            expected_owner_tid,
            flags,
        },
    )
    .expect("a SAS GrantSlice refusal is the usize::MAX sentinel, never an error")
}

fn holds(base: usize) -> Option<u32> {
    pin::holder_of(base, SIZE).map(|held| held.holds)
}

fn task(tid: usize, cell: u64) -> alloc::boxed::Box<Task> {
    alloc::boxed::Box::new(Task::new(
        tid,
        CellId(cell),
        "grant-lease-test",
        alloc::vec::Vec::new(),
    ))
}

/// Run `f` with a live holder and owner (different Cells, neither receiving)
/// and one owner row shared to the holder in each grant table. Rows carry no
/// frames: a SAS slice only returns `base`, and nothing here frees them.
fn with_leasable_rows(f: impl FnOnce()) {
    let _guard = crate::TEST_STATE_LOCK.lock();
    let saved = SCHEDULER.lock().take();
    let mut scheduler = Scheduler::new();
    scheduler.tasks.insert(HOLDER_TID, task(HOLDER_TID, 81));
    scheduler.tasks.insert(OWNER_TID, task(OWNER_TID, 82));
    *SCHEDULER.lock() = Some(scheduler);

    let shared_to = Some((HOLDER_TID, GrantPerm::ReadOnly));
    grant_table_lock()
        .lock()
        .get_or_insert_with(BTreeMap::new)
        .insert(
            PAGE_BASE,
            PageGrant {
                base: PAGE_BASE,
                size: SIZE,
                owner: OWNER_TID,
                owner_cell: CellId(82),
                owner_generation: 1,
                shared_to,
                domain: None,
            },
        );
    reg_grant_table_lock()
        .lock()
        .get_or_insert_with(BTreeMap::new)
        .insert(
            REG_BASE,
            RegGrant {
                base: REG_BASE,
                size: SIZE,
                owner: OWNER_TID,
                owner_cell: CellId(82),
                owner_generation: 1,
                shared_to,
                domain: None,
            },
        );

    f();

    // Whatever a case left behind must not leak into the next one.
    let _ = pin::release_grant_holder_leases(HOLDER_TID);
    if let Some(map) = grant_table_lock().lock().as_mut() {
        map.remove(&PAGE_BASE);
    }
    if let Some(map) = reg_grant_table_lock().lock().as_mut() {
        map.remove(&REG_BASE);
    }
    *SCHEDULER.lock() = saved;
}

#[test]
fn grant_slice_decodes_a3_as_flags() {
    match map_syscall(ViSyscall::GrantSlice as usize, 7, 0, OWNER_TID, LEASE) {
        Some(Syscall::GrantSlice {
            grant_id: 7,
            size_out_ptr: 0,
            expected_owner_tid: OWNER_TID,
            flags: LEASE,
        }) => {}
        other => panic!("GrantSlice decoded as {other:?}"),
    }
}

#[test]
fn lease_blocks_owner_free_and_unregister_and_re_lease_adds_a_hold() {
    with_leasable_rows(|| {
        for base in [PAGE_BASE, REG_BASE] {
            assert_eq!(slice(base, OWNER_TID, LEASE), base, "{base:#x}");
            assert_eq!(holds(base), Some(1));
            assert_eq!(slice(base, OWNER_TID, LEASE), base, "{base:#x}");
            assert_eq!(holds(base), Some(2), "same key folds into one entry");
        }
        assert_eq!(
            handle_syscall(
                OWNER_TID,
                Syscall::GrantFree {
                    grant_id: PAGE_BASE
                }
            ),
            Err(SyscallError::PermissionDenied)
        );
        assert_eq!(
            handle_syscall(OWNER_TID, Syscall::GrantUnregister { reg_id: REG_BASE }),
            Err(SyscallError::PermissionDenied)
        );
        // The refusal left both rows and both leases in place.
        assert!(grant_table_lock()
            .lock()
            .as_ref()
            .is_some_and(|map| map.contains_key(&PAGE_BASE)));
        assert!(reg_grant_table_lock()
            .lock()
            .as_ref()
            .is_some_and(|map| map.contains_key(&REG_BASE)));
        assert_eq!(holds(PAGE_BASE), Some(2));
        // An unleased slice still publishes nothing.
        assert!(pin::release_grant_holder_leases(HOLDER_TID).is_empty());
        assert_eq!(slice(PAGE_BASE, OWNER_TID, 0), PAGE_BASE);
        assert_eq!(holds(PAGE_BASE), None);
    });
}

#[test]
fn refusals_answer_as_an_unknown_id_and_publish_nothing() {
    with_leasable_rows(|| {
        let unknown = slice(0x5b30_0000, OWNER_TID, 0);
        assert_eq!(unknown, usize::MAX);
        for base in [PAGE_BASE, REG_BASE] {
            assert_eq!(slice(base, OWNER_TID, 1 << 1), unknown, "unknown bit");
            assert_eq!(slice(base, OWNER_TID, LEASE | 1 << 7), unknown);
            assert_eq!(slice(base, 0, LEASE), unknown, "LEASE without an owner");
            assert_eq!(slice(base, OTHER_TID, LEASE), unknown, "wrong owner");
            // Refused before `size_out` is inspected, like an unknown id.
            assert_eq!(
                handle_syscall(
                    HOLDER_TID,
                    Syscall::GrantSlice {
                        grant_id: base,
                        size_out_ptr: 1,
                        expected_owner_tid: 0,
                        flags: LEASE,
                    },
                ),
                Ok(usize::MAX)
            );
            assert_eq!(holds(base), None, "{base:#x}");
        }
    });
}

#[test]
fn a_full_holder_lease_table_refuses_new_keys_as_unknown() {
    with_leasable_rows(|| {
        // Fill the holder's table with keys on other (synthetic) Grants.
        for i in 0..pin::MAX_GRANT_LEASES_PER_HOLDER {
            let base = 0x5c00_0000 + i * SIZE;
            assert_eq!(
                pin::lease_grant(base, SIZE, OTHER_TID, HOLDER_TID, base),
                Ok(())
            );
        }
        assert_eq!(slice(PAGE_BASE, OWNER_TID, LEASE), usize::MAX);
        assert_eq!(holds(PAGE_BASE), None);
        // Without LEASE the same slice still resolves.
        assert_eq!(slice(PAGE_BASE, OWNER_TID, 0), PAGE_BASE);
        assert!(pin::release_grant_holder_leases(HOLDER_TID).is_empty());
        assert_eq!(slice(PAGE_BASE, OWNER_TID, LEASE), PAGE_BASE);
    });
}

#[test]
fn the_holders_next_receive_ends_its_leases() {
    with_leasable_rows(|| {
        assert_eq!(slice(PAGE_BASE, OWNER_TID, LEASE), PAGE_BASE);
        assert_eq!(slice(REG_BASE, OWNER_TID, LEASE), REG_BASE);
        // Another task's receive is not the holder's boundary.
        let _ = handle_syscall(
            OWNER_TID,
            Syscall::TryRecv {
                mask: 0,
                buf_ptr: 0,
                buf_len: 0,
                attest_caller: false,
            },
        );
        assert_eq!(holds(PAGE_BASE), Some(1));
        // A masked receive is a receive too.
        let _ = handle_syscall(
            HOLDER_TID,
            Syscall::TryRecv {
                mask: OTHER_TID,
                buf_ptr: 0,
                buf_len: 0,
                attest_caller: false,
            },
        );
        assert_eq!(holds(PAGE_BASE), None);
        assert_eq!(holds(REG_BASE), None);
    });
}

#[test]
fn a_send_to_the_owner_ends_its_leases_and_other_sends_do_not() {
    with_leasable_rows(|| {
        assert_eq!(slice(PAGE_BASE, OWNER_TID, LEASE), PAGE_BASE);
        let try_send = |target| {
            let _ = handle_syscall(
                HOLDER_TID,
                Syscall::TrySend {
                    target,
                    msg_ptr: 0,
                    msg_len: 0,
                },
            );
        };
        try_send(OTHER_TID);
        assert_eq!(
            holds(PAGE_BASE),
            Some(1),
            "a send elsewhere keeps the lease"
        );
        try_send(OWNER_TID);
        assert_eq!(holds(PAGE_BASE), None, "a send to the owner ends it");
    });
}

#[test]
fn holder_death_ends_its_leases() {
    with_leasable_rows(|| {
        assert_eq!(slice(REG_BASE, OWNER_TID, LEASE), REG_BASE);
        release_vfs_holder_leases(HOLDER_TID);
        assert_eq!(holds(REG_BASE), None);
        assert!(!pin::grant_leases_live());
    });
}

#[test]
fn owner_death_quarantines_leased_rows_until_the_lease_ends() {
    with_leasable_rows(|| {
        let before = pin::quarantined_pages();
        assert_eq!(slice(PAGE_BASE, OWNER_TID, LEASE), PAGE_BASE);
        assert_eq!(slice(REG_BASE, OWNER_TID, LEASE), REG_BASE);
        assert_eq!(reclaim_owned_grants(OWNER_TID), 2);
        // Neither row survives, and the registered row did not pass to the
        // live grantee: both sets of frames are withheld under the lease.
        assert!(grant_table_lock()
            .lock()
            .as_ref()
            .is_none_or(|map| !map.contains_key(&PAGE_BASE)));
        assert!(reg_grant_table_lock()
            .lock()
            .as_ref()
            .is_none_or(|map| !map.contains_key(&REG_BASE)));
        assert_eq!(pin::quarantined_pages(), before + 2);
        let held = pin::holder_of(REG_BASE, SIZE).expect("lease outlives its owner");
        assert!(held.quarantined && held.pending_revoke);
        // The holder's release hands exactly those frames back for freeing.
        let mut released = pin::release_grant_holder_leases(HOLDER_TID);
        released.sort_by_key(|range| range.base);
        assert_eq!(released, alloc::vec![(PAGE_BASE, 1), (REG_BASE, 1)]);
        assert_eq!(pin::quarantined_pages(), before);
    });
}
