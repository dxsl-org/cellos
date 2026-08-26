//! Boot self-test for VFS per-request lease lifetime and owner death watch.
//!
//! Runs after `task::init()` and before real cells spawn, so synthetic tids and
//! VFS registration cannot collide with runtime state. Fake quarantined ranges
//! are inspected and discarded here only; they are never handed to the allocator.

use super::syscall::{handle_syscall, Syscall};
use super::tcb::Task;
use alloc::boxed::Box;
use alloc::vec;
use types::CellId;

const PAGE_SIZE: usize = 4096;
const VFS_OWNER_TID: usize = 9521;
const VFS_WORKER_TID: usize = 9522;
const CLIENT_OWNER_TID: usize = 9531;
const CLIENT_WORKER_TID: usize = 9532;
const OTHER_TID: usize = 9533;
const VFS_CELL_ID: u64 = 41;
const CLIENT_CELL_ID: u64 = 42;

const fn arena(n: usize) -> usize {
    0x5000_0000 + n * 0x10_0000
}

fn fail(step: &str, detail: &str) -> bool {
    log::error!("[selftest] VFS-LIFETIME/{step}: FAIL — {detail}");
    false
}

fn mk_task(tid: usize, cell: u64, name: &str) -> Box<Task> {
    Box::new(Task::new(tid, CellId(cell), name, vec![]))
}

fn insert(task: Box<Task>) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.tasks.insert(task.id, task);
    }
}

fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.tasks.remove(&tid);
    }
    super::hart_local::ready::remove_from_all(tid);
}

fn set_recv_waiting(tid: usize, mask: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.state = super::tcb::TaskState::Recv {
                mask,
                buf_ptr: 0,
                buf_len: 0,
                deadline: None,
            };
        }
    }
}

fn vfs_send_release_is_exact() -> bool {
    insert(mk_task(
        VFS_WORKER_TID,
        VFS_OWNER_TID as u64,
        "vfs-worker-send-selftest",
    ));
    insert(mk_task(
        CLIENT_OWNER_TID,
        CLIENT_OWNER_TID as u64,
        "client-owner-send-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_OWNER_TID as u64,
        "client-worker-send-selftest",
    ));
    insert(mk_task(OTHER_TID, OTHER_TID as u64, "other-send-selftest"));
    crate::fast_ipc::set_vfs_handler_cell(VFS_OWNER_TID);

    let owner_generation = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&CLIENT_WORKER_TID))
        .map(|task| task.cell_generation)
        .unwrap_or(0);
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&VFS_WORKER_TID) {
            task.set_current_caller_context(
                CLIENT_WORKER_TID,
                CLIENT_OWNER_TID as u64,
                owner_generation,
            );
        }
    }

    let grant_id = match handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::GrantRegister { size: PAGE_SIZE },
    ) {
        Ok(id) if id != 0 => id,
        _ => {
            crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
            remove(VFS_WORKER_TID);
            remove(CLIENT_OWNER_TID);
            remove(CLIENT_WORKER_TID);
            remove(OTHER_TID);
            return fail("send-release", "GrantRegister setup failed");
        }
    };
    if handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::GrantShare {
            grant_id,
            target_cell: VFS_WORKER_TID,
            perm: 0,
        },
    ) != Ok(0)
    {
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_OWNER_TID);
        remove(CLIENT_WORKER_TID);
        remove(OTHER_TID);
        return fail("send-release", "GrantShare setup failed");
    }
    let slice_result = handle_syscall(
        VFS_WORKER_TID,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: 0,
        },
    );
    if slice_result != Ok(grant_id) {
        log::error!(
            "[selftest] VFS-LIFETIME/send-release: GrantSlice result={slice_result:?}, grant_id={grant_id:#x}"
        );
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_OWNER_TID);
        remove(CLIENT_WORKER_TID);
        remove(OTHER_TID);
        return fail("send-release", "GrantSlice did not pin the VFS lease");
    }

    set_recv_waiting(OTHER_TID, VFS_WORKER_TID);
    if handle_syscall(
        VFS_WORKER_TID,
        Syscall::Send {
            target: OTHER_TID,
            msg_ptr: 0,
            msg_len: 0,
        },
    ) != Ok(0)
    {
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_OWNER_TID);
        remove(CLIENT_WORKER_TID);
        remove(OTHER_TID);
        return fail("send-release", "wrong-target Send failed");
    }
    let wrong_target_preserved = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&VFS_WORKER_TID).is_some_and(|task| {
            task.current_caller == Some(CLIENT_WORKER_TID)
                && task.current_caller_cell_id == CLIENT_OWNER_TID as u64
        })
    });
    if !wrong_target_preserved || crate::memory::pin::holder_of(grant_id, PAGE_SIZE).is_none() {
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_OWNER_TID);
        remove(CLIENT_WORKER_TID);
        remove(OTHER_TID);
        return fail(
            "send-release",
            "wrong-target Send cleared caller context or released the lease",
        );
    }

    set_recv_waiting(CLIENT_WORKER_TID, VFS_WORKER_TID);
    if handle_syscall(
        VFS_WORKER_TID,
        Syscall::Send {
            target: CLIENT_WORKER_TID,
            msg_ptr: 0,
            msg_len: 0,
        },
    ) != Ok(0)
    {
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_OWNER_TID);
        remove(CLIENT_WORKER_TID);
        remove(OTHER_TID);
        return fail("send-release", "matching Send failed");
    }
    let cleared = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&VFS_WORKER_TID).is_some_and(|task| {
            task.current_caller.is_none()
                && task.current_caller_cell_id == 0
                && task.current_caller_cell_generation == 0
                && task.current_caller_request_generation == 0
        })
    });

    crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_OWNER_TID);
    remove(CLIENT_WORKER_TID);
    remove(OTHER_TID);

    if !cleared {
        return fail(
            "send-release",
            "matching Send did not clear the caller context",
        );
    }
    if crate::memory::pin::holder_of(grant_id, PAGE_SIZE).is_some() {
        return fail("send-release", "matching Send did not release the lease");
    }
    true
}

fn registered_grant_owner_death_reaps_leased_entry() -> bool {
    insert(mk_task(
        VFS_WORKER_TID,
        VFS_OWNER_TID as u64,
        "vfs-worker-reg-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_OWNER_TID as u64,
        "client-worker-reg-selftest",
    ));
    crate::fast_ipc::set_vfs_handler_cell(VFS_OWNER_TID);

    let owner_generation = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&CLIENT_WORKER_TID))
        .map(|task| task.cell_generation)
        .unwrap_or(0);
    let grant_id = match handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::GrantRegister { size: PAGE_SIZE },
    ) {
        Ok(id) if id != 0 => id,
        _ => {
            crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
            remove(VFS_WORKER_TID);
            remove(CLIENT_WORKER_TID);
            return fail("reg-reap", "GrantRegister setup failed");
        }
    };
    if handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::GrantShare {
            grant_id,
            target_cell: VFS_WORKER_TID,
            perm: 0,
        },
    ) != Ok(0)
    {
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_WORKER_TID);
        return fail("reg-reap", "GrantShare setup failed");
    }
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&VFS_WORKER_TID) {
            task.set_current_caller_context(
                CLIENT_WORKER_TID,
                CLIENT_OWNER_TID as u64,
                owner_generation,
            );
        }
    }
    let request_generation = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|sched| sched.tasks.get(&VFS_WORKER_TID))
        .map(|task| task.current_caller_request_generation)
        .unwrap_or(0);
    let slice_result = handle_syscall(
        VFS_WORKER_TID,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: 0,
        },
    );
    if slice_result != Ok(grant_id) {
        log::error!(
            "[selftest] VFS-LIFETIME/reg-reap: GrantSlice result={slice_result:?}, grant_id={grant_id:#x}"
        );
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_WORKER_TID);
        return fail(
            "reg-reap",
            "GrantSlice did not pin the shared registered grant",
        );
    }
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.exit_task(CLIENT_WORKER_TID, 33);
    }
    super::syscall::reap_grants_for_task(CLIENT_WORKER_TID);
    let removed = super::syscall::test_registered_grant_owner(grant_id).is_none();
    let holder = crate::memory::pin::holder_of(grant_id, PAGE_SIZE);
    let preserved = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&VFS_WORKER_TID).is_some_and(|task| {
            task.current_caller == Some(CLIENT_WORKER_TID)
                && task.current_caller_request_generation == request_generation
        })
    });
    let blocked_new_slice = handle_syscall(
        VFS_WORKER_TID,
        Syscall::GrantSlice {
            grant_id,
            size_out_ptr: 0,
        },
    ) == Ok(usize::MAX);
    let released = crate::memory::pin::release_vfs_lease(
        VFS_WORKER_TID,
        CLIENT_WORKER_TID,
        request_generation,
    );
    let lease_cleared =
        crate::memory::pin::find_vfs_lease(VFS_WORKER_TID, CLIENT_WORKER_TID, request_generation)
            .is_none();

    crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_WORKER_TID);

    if !removed {
        return fail(
            "reg-reap",
            "owner-dead registered grant was transferred instead of removed",
        );
    }
    if !matches!(
        holder,
        Some(holder)
            if holder.quarantined
                && holder.pending_revoke
                && holder.holder_tid == VFS_WORKER_TID
    ) {
        return fail(
            "reg-reap",
            "owner death unpinned the live holder instead of pending-revoking it",
        );
    }
    if !preserved {
        return fail(
            "reg-reap",
            "owner death cleared the holder context before it could drop GrantSlice",
        );
    }
    if !blocked_new_slice {
        return fail(
            "reg-reap",
            "pending-revoked holder could obtain a second GrantSlice",
        );
    }
    if released != vec![(grant_id, 1)] {
        return fail(
            "reg-reap",
            "holder completion did not release the exact quarantined registered grant",
        );
    }
    if !lease_cleared {
        return fail(
            "reg-reap",
            "holder completion left the pending-revoked lease registered",
        );
    }
    log::info!("[selftest] VFS-LIFETIME: stage=dead-owner-pending-revoke-exact-release");
    true
}

fn registered_grant_without_lease_keeps_legacy_transfer() -> bool {
    insert(mk_task(
        VFS_WORKER_TID,
        VFS_OWNER_TID as u64,
        "vfs-worker-transfer-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_OWNER_TID as u64,
        "client-worker-transfer-selftest",
    ));

    let grant_id = match handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::GrantRegister { size: PAGE_SIZE },
    ) {
        Ok(id) if id != 0 => id,
        _ => {
            remove(VFS_WORKER_TID);
            remove(CLIENT_WORKER_TID);
            return fail("reg-transfer", "GrantRegister setup failed");
        }
    };
    if handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::GrantShare {
            grant_id,
            target_cell: VFS_WORKER_TID,
            perm: 0,
        },
    ) != Ok(0)
    {
        remove(VFS_WORKER_TID);
        remove(CLIENT_WORKER_TID);
        return fail("reg-transfer", "GrantShare setup failed");
    }
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.exit_task(CLIENT_WORKER_TID, 44);
    }
    super::syscall::reap_grants_for_task(CLIENT_WORKER_TID);
    let owner = super::syscall::test_registered_grant_owner(grant_id);
    let cleanup = handle_syscall(
        VFS_WORKER_TID,
        Syscall::GrantUnregister { reg_id: grant_id },
    );

    remove(VFS_WORKER_TID);
    remove(CLIENT_WORKER_TID);

    if owner != Some(VFS_WORKER_TID) {
        return fail(
            "reg-transfer",
            "non-leased registered grant did not preserve legacy transfer",
        );
    }
    if cleanup != Ok(0) {
        return fail(
            "reg-transfer",
            "transferred registered grant could not be unregistered by the grantee",
        );
    }
    true
}

#[derive(Clone, Copy)]
enum GrantRaceKind {
    Page,
    Registered,
}

fn allocate_race_grant(kind: GrantRaceKind) -> Option<usize> {
    let syscall = match kind {
        GrantRaceKind::Page => Syscall::GrantAlloc { size: PAGE_SIZE },
        GrantRaceKind::Registered => Syscall::GrantRegister { size: PAGE_SIZE },
    };
    match handle_syscall(CLIENT_WORKER_TID, syscall) {
        Ok(id) if id != 0 => Some(id),
        _ => None,
    }
}

fn teardown_race_grant(kind: GrantRaceKind, grant_id: usize) -> super::syscall::SyscallResult {
    handle_syscall(
        CLIENT_WORKER_TID,
        match kind {
            GrantRaceKind::Page => Syscall::GrantFree { grant_id },
            GrantRaceKind::Registered => Syscall::GrantUnregister { reg_id: grant_id },
        },
    )
}

fn renew_vfs_grant_context() -> Option<u64> {
    super::SCHEDULER.lock().as_mut().and_then(|sched| {
        let owner_generation = sched
            .tasks
            .get(&CLIENT_WORKER_TID)
            .map(|task| task.cell_generation)?;
        let holder = sched.tasks.get_mut(&VFS_WORKER_TID)?;
        holder.clear_current_caller_context();
        holder.set_current_caller_context(
            CLIENT_WORKER_TID,
            CLIENT_CELL_ID,
            owner_generation,
        );
        Some(holder.current_caller_request_generation)
    })
}

/// Exercise both outcomes around the grant-table linearization point.
///
/// The lease-first half proves teardown observes the pin published by
/// GrantSlice. The teardown-first half proves a removed entry cannot publish a
/// lease from stale grant fields.
fn grant_slice_and_teardown_are_linearized(kind: GrantRaceKind) -> bool {
    insert(mk_task(
        VFS_WORKER_TID,
        VFS_OWNER_TID as u64,
        "vfs-worker-grant-race-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_CELL_ID,
        "client-worker-grant-race-selftest",
    ));
    crate::fast_ipc::set_vfs_handler_cell(VFS_OWNER_TID);

    let lease_generation = renew_vfs_grant_context();
    let leased = allocate_race_grant(kind);
    let shared = leased.is_some_and(|grant_id| {
        handle_syscall(
            CLIENT_WORKER_TID,
            Syscall::GrantShare {
                grant_id,
                target_cell: VFS_WORKER_TID,
                perm: 0,
            },
        ) == Ok(0)
    });
    let slice = leased.map(|grant_id| {
        handle_syscall(
            VFS_WORKER_TID,
            Syscall::GrantSlice {
                grant_id,
                size_out_ptr: 0,
            },
        )
    });
    let teardown_refused = leased.is_some_and(|grant_id| {
        teardown_race_grant(kind, grant_id) == Err(super::syscall::SyscallError::PermissionDenied)
    });
    let lease_visible = leased.is_some_and(|grant_id| {
        crate::memory::pin::holder_of(grant_id, PAGE_SIZE).is_some()
            && slice == Some(Ok(grant_id))
    });
    if let Some(request_generation) = lease_generation {
        let _ = crate::memory::pin::release_vfs_lease(
            VFS_WORKER_TID,
            CLIENT_WORKER_TID,
            request_generation,
        );
    }
    let leased_cleanup =
        leased.is_some_and(|grant_id| teardown_race_grant(kind, grant_id) == Ok(0));

    let free_generation = renew_vfs_grant_context();
    let freed = allocate_race_grant(kind);
    let free_shared = freed.is_some_and(|grant_id| {
        handle_syscall(
            CLIENT_WORKER_TID,
            Syscall::GrantShare {
                grant_id,
                target_cell: VFS_WORKER_TID,
                perm: 0,
            },
        ) == Ok(0)
    });
    let teardown_won =
        freed.is_some_and(|grant_id| teardown_race_grant(kind, grant_id) == Ok(0));
    let stale_slice_denied = freed.is_some_and(|grant_id| {
        handle_syscall(
            VFS_WORKER_TID,
            Syscall::GrantSlice {
                grant_id,
                size_out_ptr: 0,
            },
        ) == Ok(usize::MAX)
            && free_generation.is_some_and(|request_generation| {
                crate::memory::pin::find_vfs_lease(
                    VFS_WORKER_TID,
                    CLIENT_WORKER_TID,
                    request_generation,
                )
                .is_none()
            })
    });
    let invalid_generation = renew_vfs_grant_context();
    let invalid = allocate_race_grant(kind);
    let invalid_shared = invalid.is_some_and(|grant_id| {
        handle_syscall(
            CLIENT_WORKER_TID,
            Syscall::GrantShare {
                grant_id,
                target_cell: VFS_WORKER_TID,
                perm: 0,
            },
        ) == Ok(0)
    });
    let invalid_slice = invalid.map(|grant_id| {
        handle_syscall(
            VFS_WORKER_TID,
            Syscall::GrantSlice {
                grant_id,
                size_out_ptr: 1,
            },
        )
    });
    let invalid_lease_absent = invalid_generation.is_some_and(|request_generation| {
        crate::memory::pin::find_vfs_lease(
            VFS_WORKER_TID,
            CLIENT_WORKER_TID,
            request_generation,
        )
        .is_none()
    });
    if let Some(request_generation) = invalid_generation {
        let _ = crate::memory::pin::release_vfs_lease(
            VFS_WORKER_TID,
            CLIENT_WORKER_TID,
            request_generation,
        );
    }
    let invalid_cleanup =
        invalid.is_some_and(|grant_id| teardown_race_grant(kind, grant_id) == Ok(0));
    let invalid_slice_leak_free =
        invalid_slice.is_some_and(|result| result.is_err()) && invalid_lease_absent;

    crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_WORKER_TID);

    if lease_generation.is_none()
        || !shared
        || !lease_visible
        || !teardown_refused
        || !leased_cleanup
        || free_generation.is_none()
        || !free_shared
        || !teardown_won
        || !stale_slice_denied
        || invalid_generation.is_none()
        || !invalid_shared
        || !invalid_slice_leak_free
        || !invalid_cleanup
    {
        return fail(
            "grant-table-race",
            "GrantSlice and grant teardown did not produce exactly one winning state",
        );
    }
    true
}

/// Deterministically interleave the two SMP contenders at their scheduler
/// linearization point: snapshot a valid VFS context, retire the owner, then
/// attempt every slot's worth of stale installs. None may create a lease.
fn stale_context_install_is_denied_without_capacity_loss() -> bool {
    insert(mk_task(
        VFS_WORKER_TID,
        VFS_OWNER_TID as u64,
        "vfs-worker-stale-install-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_CELL_ID,
        "client-worker-stale-install-selftest",
    ));
    crate::fast_ipc::set_vfs_handler_cell(VFS_OWNER_TID);

    let request_generation = super::SCHEDULER.lock().as_mut().and_then(|sched| {
        let owner_generation = sched
            .tasks
            .get(&CLIENT_WORKER_TID)
            .map(|task| task.cell_generation)?;
        let holder = sched.tasks.get_mut(&VFS_WORKER_TID)?;
        holder.set_current_caller_context(CLIENT_WORKER_TID, CLIENT_CELL_ID, owner_generation);
        Some(holder.current_caller_request_generation)
    });
    let Some(request_generation) = request_generation else {
        crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
        remove(VFS_WORKER_TID);
        remove(CLIENT_WORKER_TID);
        return fail(
            "stale-install",
            "could not establish the VFS holder context",
        );
    };
    let snapshot = match super::syscall::current_vfs_grant_lookup(VFS_WORKER_TID) {
        super::syscall::VfsGrantLookup::Active(context) => context,
        _ => {
            crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
            remove(VFS_WORKER_TID);
            remove(CLIENT_WORKER_TID);
            return fail("stale-install", "could not snapshot the live VFS context");
        }
    };

    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.exit_task(CLIENT_WORKER_TID, 77);
    }

    let stale_install_succeeded = (0..crate::memory::pin::MAX_VFS_LEASES).any(|slot| {
        super::syscall::test_install_vfs_lease_if_context_live(
            VFS_WORKER_TID,
            snapshot,
            arena(20 + slot),
            PAGE_SIZE,
            0x9000 + slot,
        )
    });
    let stale_lease_absent =
        crate::memory::pin::find_vfs_lease(VFS_WORKER_TID, CLIENT_WORKER_TID, request_generation)
            .is_none();

    let mut capacity_available = true;
    for slot in 0..crate::memory::pin::MAX_VFS_LEASES {
        if crate::memory::pin::pin_vfs_lease(
            arena(60 + slot),
            PAGE_SIZE,
            0xA000 + slot,
            0xB000 + slot,
            0xC000 + slot,
            0xD000 + slot as u64,
        )
        .is_err()
        {
            capacity_available = false;
        }
    }
    let table_full_only_after_capacity = matches!(
        crate::memory::pin::pin_vfs_lease(arena(100), PAGE_SIZE, 0xE000, 0xF000, 0x10000, 0x11000,),
        Err(crate::memory::pin::VfsLeaseError::TableFull)
    );
    for slot in 0..crate::memory::pin::MAX_VFS_LEASES {
        let _ = crate::memory::pin::release_vfs_lease(
            0xB000 + slot,
            0xA000 + slot,
            0xD000 + slot as u64,
        );
    }

    crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_WORKER_TID);

    if stale_install_succeeded || !stale_lease_absent {
        return fail(
            "stale-install",
            "retired owner installed a non-pending VFS lease from a stale context",
        );
    }
    if !capacity_available || !table_full_only_after_capacity {
        return fail(
            "stale-install",
            "stale install consumed VFS lease capacity before live leases filled the table",
        );
    }
    log::info!("[selftest] VFS-LIFETIME: stage=smp-stale-context-denied-capacity-preserved");
    true
}

fn exact_lease_release() -> bool {
    let base = arena(1);
    if crate::memory::pin::pin_vfs_lease(base, PAGE_SIZE, 701, 801, 17, 11).is_err() {
        return fail("exact-lease", "pin_vfs_lease setup failed");
    }
    if !crate::memory::pin::release_vfs_lease(801, 999, 11).is_empty() {
        return fail("exact-lease", "wrong grant owner released the lease");
    }
    if crate::memory::pin::holder_of(base, PAGE_SIZE).is_none() {
        return fail(
            "exact-lease",
            "lease vanished after wrong grant owner release",
        );
    }
    if !crate::memory::pin::release_vfs_lease(801, 701, 12).is_empty() {
        return fail("exact-lease", "wrong request generation released the lease");
    }
    if crate::memory::pin::holder_of(base, PAGE_SIZE).is_none() {
        return fail(
            "exact-lease",
            "lease vanished after wrong request generation",
        );
    }
    if !crate::memory::pin::release_vfs_lease(801, 701, 11).is_empty() {
        return fail(
            "exact-lease",
            "exact release unexpectedly returned quarantined frames",
        );
    }
    if crate::memory::pin::holder_of(base, PAGE_SIZE).is_some() {
        return fail("exact-lease", "exact release did not clear the lease");
    }
    true
}

fn quarantine_waits_for_exact_release() -> bool {
    let base = arena(2);
    let before = crate::memory::pin::quarantined_pages();
    if crate::memory::pin::pin_vfs_lease(base, PAGE_SIZE, 702, 802, 18, 21).is_err()
        || !crate::memory::pin::mark_vfs_lease_pending_revoke(802, 702, 21)
        || !crate::memory::pin::withhold_vfs_frames(base, 1, 802, 702, 21)
    {
        let _ = crate::memory::pin::release_vfs_lease(802, 702, 21);
        return fail("quarantine", "setup failed");
    }
    let held = crate::memory::pin::holder_of(base, PAGE_SIZE);
    let wrong = crate::memory::pin::release_vfs_lease(802, 702, 22);
    if !matches!(held, Some(holder) if holder.quarantined) {
        return fail(
            "quarantine",
            "owner death did not leave the lease quarantined",
        );
    }
    if !wrong.is_empty() {
        return fail(
            "quarantine",
            "wrong request generation released quarantined frames",
        );
    }
    if crate::memory::pin::quarantined_pages() != before + 1 {
        return fail(
            "quarantine",
            "quarantined page count changed before exact release",
        );
    }
    let exact = crate::memory::pin::release_vfs_lease(802, 702, 21);
    if exact != vec![(base, 1)] {
        return fail(
            "quarantine",
            "exact release did not return only the quarantined lease",
        );
    }
    if crate::memory::pin::quarantined_pages() != before {
        return fail("quarantine", "quarantine count did not return to baseline");
    }
    if crate::memory::pin::holder_of(base, PAGE_SIZE).is_some() {
        return fail("quarantine", "exact release left the lease registered");
    }
    true
}

fn holder_death_is_selective() -> bool {
    let first = arena(3);
    let second = arena(4);
    let before = crate::memory::pin::quarantined_pages();
    if crate::memory::pin::pin_vfs_lease(first, PAGE_SIZE, 703, 803, 19, 31).is_err()
        || crate::memory::pin::pin_vfs_lease(second, PAGE_SIZE, 704, 804, 20, 32).is_err()
        || !crate::memory::pin::withhold_vfs_frames(first, 1, 803, 703, 31)
        || !crate::memory::pin::withhold_vfs_frames(second, 1, 804, 704, 32)
    {
        let _ = crate::memory::pin::release_vfs_holder_leases(803);
        let _ = crate::memory::pin::release_vfs_holder_leases(804);
        return fail("holder-death", "setup failed");
    }
    let released = crate::memory::pin::release_vfs_holder_leases(803);
    let still_second = crate::memory::pin::holder_of(second, PAGE_SIZE).is_some();
    let released_second = crate::memory::pin::release_vfs_holder_leases(804);
    if released != vec![(first, 1)] {
        return fail(
            "holder-death",
            "dead holder did not release only its own lease",
        );
    }
    if crate::memory::pin::holder_of(first, PAGE_SIZE).is_some() {
        return fail("holder-death", "released holder lease remained registered");
    }
    if !still_second {
        return fail(
            "holder-death",
            "other holder lease was touched by the first release",
        );
    }
    if released_second != vec![(second, 1)] {
        return fail(
            "holder-death",
            "second holder release did not return its quarantined lease",
        );
    }
    if crate::memory::pin::quarantined_pages() != before {
        return fail(
            "holder-death",
            "quarantine count did not return to baseline",
        );
    }
    true
}

fn vfs_owner_watch() -> bool {
    insert(mk_task(VFS_OWNER_TID, VFS_CELL_ID, "vfs-owner-selftest"));
    insert(mk_task(VFS_WORKER_TID, VFS_CELL_ID, "vfs-worker-selftest"));
    insert(mk_task(
        CLIENT_OWNER_TID,
        CLIENT_CELL_ID,
        "client-owner-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_CELL_ID,
        "client-worker-selftest",
    ));
    insert(mk_task(OTHER_TID, 43, "other-selftest"));
    crate::fast_ipc::set_vfs_handler_cell(VFS_CELL_ID as usize);

    let owner_generation = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|sched| sched.tasks.get(&CLIENT_OWNER_TID))
            .map(|task| task.cell_generation)
            .unwrap_or(0)
    };
    let message = [0u8; 1];
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&CLIENT_WORKER_TID) {
            task.cell_generation = owner_generation;
            task.root_tid = CLIENT_OWNER_TID;
        }
        let owner = api::cell_owner::CellOwner::new(
            CLIENT_CELL_ID,
            owner_generation,
            CLIENT_OWNER_TID as u64,
        );
        if !sched.publish_live_cell_owner(owner) {
            return fail("watch", "could not publish disjoint root-owner fixture");
        }
        if let Some(task) = sched.tasks.get_mut(&VFS_WORKER_TID) {
            task.set_current_caller_context(OTHER_TID, OTHER_TID as u64, owner_generation);
            let _ = task.begin_receive_context(0);
        }
    }
    set_recv_waiting(VFS_WORKER_TID, CLIENT_WORKER_TID);
    let initial_delivery = super::ipc_send(
        CLIENT_WORKER_TID,
        VFS_WORKER_TID,
        message.as_ptr() as usize,
        message.len(),
    );
    let initial_recv = handle_syscall(
        VFS_WORKER_TID,
        Syscall::Recv {
            mask: 0,
            buf_ptr: 0,
            buf_len: 0,
            attest_caller: false,
        },
    );
    let nested_delivery = super::ipc_send(
        OTHER_TID,
        VFS_WORKER_TID,
        message.as_ptr() as usize,
        message.len(),
    );
    let outer_context_preserved = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&VFS_WORKER_TID).is_some_and(|task| {
            task.current_caller == Some(CLIENT_WORKER_TID)
                && task.current_caller_cell_id == CLIENT_CELL_ID
                && task.current_caller_cell_generation == owner_generation
        })
    });

    let (owner_watch_token, denied) =
        super::SCHEDULER
            .lock()
            .as_mut()
            .map_or((None, true), |sched| {
                let owner_watch_token = sched
                    .watch_live_cell_owner(VFS_WORKER_TID, CellId(CLIENT_CELL_ID), owner_generation)
                    .map(|(_, token)| token);
                let denied = sched
                    .watch_live_cell_owner(
                        VFS_WORKER_TID,
                        CellId(OTHER_TID as u64),
                        owner_generation,
                    )
                    .is_none();
                (owner_watch_token, denied)
            });

    let (worker_exit_clean, masked_backend_preserved, owner_death_delivered) = {
        let mut guard = super::SCHEDULER.lock();
        let Some(sched) = guard.as_mut() else {
            return fail("watch", "scheduler unavailable during owner watch");
        };
        sched.exit_task(CLIENT_WORKER_TID, 7);
        let worker_exit_clean = sched
            .tasks
            .get(&VFS_WORKER_TID)
            .is_some_and(|task| task.current_caller.is_none());
        sched.exit_task(CLIENT_OWNER_TID, 9);
        let (masked_backend_preserved, owner_death_delivered) = sched
            .tasks
            .get_mut(&VFS_WORKER_TID)
            .map_or((false, false), |task| {
                // A nested backend receive must return its matching reply without
                // consuming the tokenized owner event reserved for VFS's public
                // wildcard receive.
                let masked = matches!(
                    super::syscall::take_resume_delivery(task, OTHER_TID),
                    super::syscall::ResumeDelivery::Message(message)
                        if message.sender_tid == OTHER_TID
                ) && owner_watch_token.is_some_and(|token| {
                    task.pending_owner_deaths.as_slice() == [(token, CLIENT_OWNER_TID, 9)]
                });
                let public = matches!(
                    super::syscall::take_resume_delivery(task, 0),
                    super::syscall::ResumeDelivery::Death {
                        sender_tid: CLIENT_OWNER_TID,
                        reason: 9
                    }
                ) && task.pending_owner_deaths.is_empty();
                (masked, public)
            });
        (
            worker_exit_clean,
            masked_backend_preserved,
            owner_death_delivered,
        )
    };

    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&VFS_WORKER_TID) {
            task.set_current_caller_context(CLIENT_WORKER_TID, CLIENT_CELL_ID, owner_generation);
        }
    }
    let already_dead = super::SCHEDULER.lock().as_mut().is_some_and(|sched| {
        sched
            .watch_live_cell_owner(VFS_WORKER_TID, CellId(CLIENT_CELL_ID), owner_generation)
            .is_none()
    });

    crate::fast_ipc::clear_vfs_if_cell(VFS_CELL_ID as usize);
    remove(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_OWNER_TID);
    remove(CLIENT_WORKER_TID);
    remove(OTHER_TID);

    if initial_delivery != Ok(0)
        || initial_recv != Ok(CLIENT_WORKER_TID)
        || nested_delivery != Ok(1)
    {
        return fail("watch", "nested IPC delivery setup failed");
    }
    if !outer_context_preserved {
        return fail("watch", "nested IPC replaced the outer VFS caller context");
    }
    if owner_watch_token.is_none() {
        return fail("watch", "VFS worker could not subscribe to the caller root");
    }
    if !denied {
        return fail("watch", "mismatched principal owner watch was not denied");
    }
    if !worker_exit_clean {
        return fail(
            "watch",
            "caller worker exit did not clear current_caller context",
        );
    }
    if !masked_backend_preserved {
        return fail(
            "watch",
            "masked backend receive did not retain the exact tokenized owner event",
        );
    }
    if !owner_death_delivered {
        return fail(
            "watch",
            "owner exit did not deliver a one-shot death to VFS",
        );
    }
    if !already_dead {
        return fail("watch", "root death raced owner watch without denial");
    }
    true
}

pub fn self_test() -> bool {
    let ok = exact_lease_release()
        & quarantine_waits_for_exact_release()
        & holder_death_is_selective()
        & grant_slice_and_teardown_are_linearized(GrantRaceKind::Page)
        & grant_slice_and_teardown_are_linearized(GrantRaceKind::Registered)
        & stale_context_install_is_denied_without_capacity_loss()
        & vfs_owner_watch()
        & vfs_send_release_is_exact()
        & registered_grant_owner_death_reaps_leased_entry()
        & registered_grant_without_lease_keeps_legacy_transfer();
    if ok {
        log::info!(
            "[selftest] VFS-LIFETIME: PASS (atomic grant-table lease + teardown orders + exact quarantine + owner watch)"
        );
    } else {
        log::error!("[selftest] VFS-LIFETIME: FAIL");
    }
    ok
}
