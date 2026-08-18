//! Boot self-test for VFS per-request lease lifetime and owner death watch.
//!
//! Runs after `task::init()` and before real cells spawn, so synthetic tids and
//! VFS registration cannot collide with runtime state. Fake quarantined ranges
//! are inspected and discarded here only; they are never handed to the allocator.

use super::syscall::{handle_syscall, Syscall, SyscallError};
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
    let quarantined = crate::memory::pin::holder_of(grant_id, PAGE_SIZE)
        .is_some_and(|holder| holder.quarantined && holder.holder_tid == VFS_WORKER_TID);
    let released = crate::memory::pin::release_vfs_lease(
        VFS_WORKER_TID,
        CLIENT_WORKER_TID,
        request_generation,
    );

    crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_WORKER_TID);

    if !removed {
        return fail(
            "reg-reap",
            "owner-dead registered grant was transferred instead of removed",
        );
    }
    if !quarantined {
        return fail(
            "reg-reap",
            "owner-dead registered grant was not quarantined under the VFS lease",
        );
    }
    if released != vec![(grant_id, 1)] {
        return fail(
            "reg-reap",
            "exact VFS release did not return the quarantined registered grant",
        );
    }
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
        || crate::memory::pin::quarantine_task(702) != 1
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
    insert(mk_task(
        VFS_OWNER_TID,
        VFS_OWNER_TID as u64,
        "vfs-owner-selftest",
    ));
    insert(mk_task(
        VFS_WORKER_TID,
        VFS_OWNER_TID as u64,
        "vfs-worker-selftest",
    ));
    insert(mk_task(
        CLIENT_OWNER_TID,
        CLIENT_OWNER_TID as u64,
        "client-owner-selftest",
    ));
    insert(mk_task(
        CLIENT_WORKER_TID,
        CLIENT_OWNER_TID as u64,
        "client-worker-selftest",
    ));
    insert(mk_task(OTHER_TID, OTHER_TID as u64, "other-selftest"));
    crate::fast_ipc::set_vfs_handler_cell(VFS_OWNER_TID);

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
        }
        if let Some(task) = sched.tasks.get_mut(&VFS_WORKER_TID) {
            task.set_current_caller_context(OTHER_TID, OTHER_TID as u64, owner_generation);
            task.begin_receive_context(0);
        }
    }
    set_recv_waiting(VFS_WORKER_TID, CLIENT_WORKER_TID);
    let initial_delivery = handle_syscall(
        CLIENT_WORKER_TID,
        Syscall::Send {
            target: VFS_WORKER_TID,
            msg_ptr: message.as_ptr() as usize,
            msg_len: message.len(),
        },
    );
    set_recv_waiting(VFS_WORKER_TID, OTHER_TID);
    let nested_delivery = handle_syscall(
        OTHER_TID,
        Syscall::Send {
            target: VFS_WORKER_TID,
            msg_ptr: message.as_ptr() as usize,
            msg_len: message.len(),
        },
    );
    let outer_context_preserved = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched.tasks.get(&VFS_WORKER_TID).is_some_and(|task| {
            task.current_caller == Some(CLIENT_WORKER_TID)
                && task.current_caller_cell_id == CLIENT_OWNER_TID as u64
                && task.current_caller_cell_generation == owner_generation
        })
    });

    let allowed = handle_syscall(
        VFS_WORKER_TID,
        Syscall::NotifyOnExit {
            watched: CLIENT_OWNER_TID,
        },
    );
    let denied = handle_syscall(
        VFS_WORKER_TID,
        Syscall::NotifyOnExit {
            watched: CLIENT_WORKER_TID,
        },
    );

    let (worker_exit_clean, owner_death_delivered) = {
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
        let owner_death_delivered = sched.tasks.get_mut(&VFS_WORKER_TID).is_some_and(|task| {
            task.pending_deaths.as_slice() == [(CLIENT_OWNER_TID, 9)] && {
                task.pending_deaths.clear();
                true
            }
        });
        (worker_exit_clean, owner_death_delivered)
    };

    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&VFS_WORKER_TID) {
            task.set_current_caller_context(
                CLIENT_WORKER_TID,
                CLIENT_OWNER_TID as u64,
                owner_generation,
            );
        }
    }
    let already_dead = handle_syscall(
        VFS_WORKER_TID,
        Syscall::NotifyOnExit {
            watched: CLIENT_OWNER_TID,
        },
    );
    let synthetic = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
        sched
            .tasks
            .get(&VFS_WORKER_TID)
            .is_some_and(|task| task.pending_deaths.as_slice() == [(CLIENT_OWNER_TID, 0)])
    });

    crate::fast_ipc::clear_vfs_if_cell(VFS_OWNER_TID);
    remove(VFS_OWNER_TID);
    remove(VFS_WORKER_TID);
    remove(CLIENT_OWNER_TID);
    remove(CLIENT_WORKER_TID);
    remove(OTHER_TID);

    if initial_delivery != Ok(0) || nested_delivery != Ok(0) {
        return fail("watch", "nested IPC delivery setup failed");
    }
    if !outer_context_preserved {
        return fail("watch", "nested IPC replaced the outer VFS caller context");
    }
    if allowed != Ok(0) {
        return fail(
            "watch",
            "VFS worker could not subscribe to the caller owner",
        );
    }
    if !matches!(denied, Err(SyscallError::PermissionDenied)) {
        return fail("watch", "arbitrary watched tid was not denied");
    }
    if !worker_exit_clean {
        return fail(
            "watch",
            "caller worker exit did not clear current_caller context",
        );
    }
    if !owner_death_delivered {
        return fail(
            "watch",
            "owner exit did not deliver a one-shot death to VFS",
        );
    }
    if already_dead != Ok(0) {
        return fail("watch", "already-dead subscribe did not succeed");
    }
    if !synthetic {
        return fail(
            "watch",
            "already-dead subscribe did not queue synthetic death",
        );
    }
    true
}

pub fn self_test() -> bool {
    let ok = exact_lease_release()
        & quarantine_waits_for_exact_release()
        & holder_death_is_selective()
        & vfs_owner_watch()
        & vfs_send_release_is_exact()
        & registered_grant_owner_death_reaps_leased_entry()
        & registered_grant_without_lease_keeps_legacy_transfer();
    if ok {
        log::info!(
            "[selftest] VFS-LIFETIME: PASS (exact lease + quarantine + cell-owner death watch)"
        );
    } else {
        log::error!("[selftest] VFS-LIFETIME: FAIL");
    }
    ok
}
