use super::{
    check_allowlist, consumes_staged_spawn_argv, encode_syscall_result, map_syscall,
    page_grant_authorizes_dma, reg_grant_authorizes_dma, staged_spawn_argv_cleanup,
    supports_typed_oom, syscall_to_vi, withhold_or_free, PageGrant, RegGrant, Syscall,
    SyscallError,
};
use crate::sched::scheduler::Scheduler;
use crate::task::cap::CapSet;
use crate::task::{tcb::Task, SCHEDULER};
use abi::syscall::ViSyscall;
use types::CellId;

/// One live task holding the pre-annex rights (`LEGACY_OBSERVE_MUTATE`), so
/// the allowlist cases below test the allowlist bit alone.
fn with_scheduler_task<F>(allowlist: u64, f: F)
where
    F: FnOnce(usize),
{
    with_scheduler_task_caps(allowlist, CapSet::LEGACY_OBSERVE_MUTATE, f)
}

fn with_scheduler_task_caps<F>(allowlist: u64, caps: CapSet, f: F)
where
    F: FnOnce(usize),
{
    let _guard = crate::TEST_STATE_LOCK.lock();
    let mut previous = SCHEDULER.lock();
    let saved = previous.take();
    let mut scheduler = Scheduler::new();
    let tid = 1;
    let mut task = alloc::boxed::Box::new(Task::new(
        tid,
        CellId(7),
        "allowlist-test",
        alloc::vec::Vec::new(),
    ));
    task.syscall_allowlist = allowlist;
    caps.apply_to(&mut task);
    scheduler.tasks.insert(tid, task);
    *previous = Some(scheduler);
    drop(previous);

    f(tid);

    let mut restore = SCHEDULER.lock();
    *restore = saved;
}

/// Annex S2-T/S2-W §6: the allowlist bit is necessary but no longer
/// sufficient — a task that declares GetProcs (14), GetProcs2 (55) and
/// MemInfo (56), even permit-all, is refused when its edge withheld X/M.
/// Phase 06 S6-1: `TelemetrySnapshot` (bit 55) is gated by X exactly like
/// `GetProcs2`; M alone never admits it.
#[test]
fn telemetry_syscalls_need_the_observe_rights_beyond_the_allowlist_bit() {
    let declared = (1u64 << 14) | (1u64 << 55) | (1u64 << 56);
    let tasks = [
        ViSyscall::GetProcs,
        ViSyscall::GetProcs2,
        ViSyscall::TelemetrySnapshot,
    ];
    for allowlist in [declared, u64::MAX] {
        with_scheduler_task_caps(allowlist, CapSet::EMPTY, |tid| {
            for sc in tasks.into_iter().chain([ViSyscall::MemInfo]) {
                assert_eq!(
                    check_allowlist(sc as usize, tid),
                    Err(SyscallError::PermissionDenied),
                    "{sc:?} with allowlist {allowlist:#x}"
                );
            }
            // Unrelated opcodes are not gated by the rights.
            assert_eq!(check_allowlist(ViSyscall::Yield as usize, tid), Ok(()));
        });
        // X alone admits exactly the task-observation pair.
        let x_only = CapSet {
            observe_tasks: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(allowlist, x_only, |tid| {
            for sc in tasks {
                assert_eq!(check_allowlist(sc as usize, tid), Ok(()), "{sc:?}");
            }
            assert_eq!(
                check_allowlist(ViSyscall::MemInfo as usize, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
        // M alone admits exactly MemInfo.
        let m_only = CapSet {
            observe_memory: true,
            ..CapSet::EMPTY
        };
        with_scheduler_task_caps(allowlist, m_only, |tid| {
            for sc in tasks {
                assert_eq!(
                    check_allowlist(sc as usize, tid),
                    Err(SyscallError::PermissionDenied),
                    "{sc:?}"
                );
            }
            assert_eq!(check_allowlist(ViSyscall::MemInfo as usize, tid), Ok(()));
        });
    }
    // The right never stands in for an undeclared bit.
    with_scheduler_task(0, |tid| {
        for sc in tasks.into_iter().chain([ViSyscall::MemInfo]) {
            assert_eq!(
                check_allowlist(sc as usize, tid),
                Err(SyscallError::PermissionDenied),
                "{sc:?}"
            );
        }
    });
    // Kernel context (tid 0) has no task and is not gated.
    for sc in tasks.into_iter().chain([ViSyscall::MemInfo]) {
        let _guard = crate::TEST_STATE_LOCK.lock();
        assert_eq!(check_allowlist(sc as usize, 0), Ok(()), "{sc:?}");
    }
}

/// Annex S2-T/S2-W §6: the attested `CALLER_FLAG_VFS_MUTATE` needs bit 63
/// in an explicit allowlist AND the `vfs_mutate` right.
#[test]
fn vfs_mutate_attestation_needs_the_right_and_the_declaration() {
    use abi::caller_identity::CALLER_FLAG_VFS_MUTATE;
    let declared = (1u64 << 62) | (1u64 << 63);
    let w_only = CapSet {
        vfs_mutate: true,
        ..CapSet::EMPTY
    };
    for (allowlist, caps, want) in [
        (
            declared,
            CapSet::LEGACY_OBSERVE_MUTATE,
            CALLER_FLAG_VFS_MUTATE,
        ),
        (declared, w_only, CALLER_FLAG_VFS_MUTATE),
        (
            declared,
            CapSet::LEGACY_OBSERVE_MUTATE.without_observe_mutate(),
            0,
        ),
        (
            declared,
            CapSet {
                vfs_mutate: false,
                ..CapSet::LEGACY_OBSERVE_MUTATE
            },
            0,
        ),
        (1u64 << 62, CapSet::LEGACY_OBSERVE_MUTATE, 0),
        (u64::MAX, CapSet::LEGACY_OBSERVE_MUTATE, 0),
    ] {
        with_scheduler_task_caps(allowlist, caps, |tid| {
            {
                let mut guard = SCHEDULER.lock();
                let sched = guard.as_mut().expect("test scheduler installed");
                let task = sched.tasks.get_mut(&tid).expect("test task");
                task.cell_generation = 1;
                task.root_tid = tid;
                assert!(sched
                    .publish_live_cell_owner(abi::cell_owner::CellOwner::new(7, 1, tid as u64)));
            }
            let identity = super::attested_identity_of(tid).expect("live task has an identity");
            assert_eq!(
                identity.flags & CALLER_FLAG_VFS_MUTATE,
                want,
                "allowlist {allowlist:#x}, caps {caps:?}"
            );
        });
    }
}

/// Phase 01 CP1 (Q): `CALLER_FLAG_CONFIG_WRITE` is attested exactly when the
/// Cell's root holds `config_write`, independent of the allowlist, and it
/// never appears for a Cell without the right.
#[test]
fn config_write_attestation_follows_the_cell_right_only() {
    use abi::caller_identity::{CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_NET_RIGHTS_MASK};
    let q_only = CapSet {
        config_write: true,
        ..CapSet::EMPTY
    };
    let shell = CapSet {
        config_write: true,
        ..CapSet::LEGACY_OBSERVE_MUTATE
    };
    for (allowlist, caps, want) in [
        (1u64 << 62, q_only, CALLER_FLAG_CONFIG_WRITE),
        (u64::MAX, shell, CALLER_FLAG_CONFIG_WRITE),
        (1u64 << 62, CapSet::LEGACY_OBSERVE_MUTATE, 0),
        (
            u64::MAX,
            CapSet {
                net_resolve: true,
                net_connect: true,
                net_listen: true,
                net_l2: true,
                ..CapSet::LEGACY_OBSERVE_MUTATE
            },
            0,
        ),
    ] {
        with_scheduler_task_caps(allowlist, caps, |tid| {
            {
                let mut guard = SCHEDULER.lock();
                let sched = guard.as_mut().expect("test scheduler installed");
                let task = sched.tasks.get_mut(&tid).expect("test task");
                task.cell_generation = 1;
                task.root_tid = tid;
                assert!(sched
                    .publish_live_cell_owner(abi::cell_owner::CellOwner::new(7, 1, tid as u64)));
            }
            let identity = super::attested_identity_of(tid).expect("live task has an identity");
            assert_eq!(
                identity.flags & CALLER_FLAG_CONFIG_WRITE,
                want,
                "caps {caps:?}"
            );
            // Q never leaks into or out of the network flags.
            assert_eq!(
                identity.flags & CALLER_FLAG_NET_RIGHTS_MASK,
                caps.net_caller_flags(),
                "caps {caps:?}"
            );
        });
    }
}

/// KSSH K3: `CALLER_FLAG_KMS_SSH_SIGN` / `CALLER_FLAG_KMS_SSH_ADMIN` are
/// attested exactly when the Cell's root holds the matching right,
/// independent of the allowlist, never for a Cell without it, and without
/// leaking into (or out of) the NET, CONFIG or VFS bits. A thread reports the
/// right of its Cell root.
#[test]
fn kms_ssh_attestation_follows_the_cell_right_only() {
    use abi::caller_identity::{
        CALLER_FLAG_CONFIG_WRITE, CALLER_FLAG_KMS_SSH_ADMIN, CALLER_FLAG_KMS_SSH_SIGN,
        CALLER_FLAG_NET_RIGHTS_MASK, CALLER_FLAG_VFS_MUTATE,
    };
    let sign_only = CapSet {
        kms_ssh_sign: true,
        ..CapSet::EMPTY
    };
    let admin_only = CapSet {
        kms_ssh_admin: true,
        ..CapSet::EMPTY
    };
    let ssh_cell = CapSet {
        kms_ssh_sign: true,
        net_resolve: true,
        net_connect: true,
        ..CapSet::LEGACY_OBSERVE_MUTATE
    };
    let shell = CapSet {
        config_write: true,
        ..CapSet::LEGACY_OBSERVE_MUTATE
    };
    let declared = (1u64 << 62) | (1u64 << 63);
    for (allowlist, caps, want) in [
        (1u64 << 62, sign_only, CALLER_FLAG_KMS_SSH_SIGN),
        (u64::MAX, sign_only, CALLER_FLAG_KMS_SSH_SIGN),
        (declared, ssh_cell, CALLER_FLAG_KMS_SSH_SIGN),
        (1u64 << 62, admin_only, CALLER_FLAG_KMS_SSH_ADMIN),
        (u64::MAX, admin_only, CALLER_FLAG_KMS_SSH_ADMIN),
        (1u64 << 62, CapSet::EMPTY, 0),
        (u64::MAX, CapSet::LEGACY_OBSERVE_MUTATE, 0),
        (u64::MAX, shell, 0),
        (
            u64::MAX,
            CapSet {
                net_resolve: true,
                net_connect: true,
                net_listen: true,
                net_l2: true,
                config_write: true,
                ..CapSet::LEGACY_OBSERVE_MUTATE
            },
            0,
        ),
    ] {
        with_scheduler_task_caps(allowlist, caps, |tid| {
            {
                let mut guard = SCHEDULER.lock();
                let sched = guard.as_mut().expect("test scheduler installed");
                let task = sched.tasks.get_mut(&tid).expect("test task");
                task.cell_generation = 1;
                task.root_tid = tid;
                assert!(sched
                    .publish_live_cell_owner(abi::cell_owner::CellOwner::new(7, 1, tid as u64)));
                // A thread of the same Cell generation holds no CapSet of its
                // own; it must report whatever its Cell root holds.
                let mut thread = alloc::boxed::Box::new(Task::new(
                    tid + 1,
                    CellId(7),
                    "kms-attestation-thread",
                    alloc::vec::Vec::new(),
                ));
                thread.cell_generation = 1;
                thread.root_tid = tid;
                sched.tasks.insert(tid + 1, thread);
            }
            let kms = CALLER_FLAG_KMS_SSH_SIGN | CALLER_FLAG_KMS_SSH_ADMIN;
            let thread_identity =
                super::attested_identity_of(tid + 1).expect("live thread has an identity");
            assert_eq!(thread_identity.flags & kms, want, "thread of caps {caps:?}");
            let identity = super::attested_identity_of(tid).expect("live task has an identity");
            assert_eq!(identity.flags & kms, want, "caps {caps:?}");
            assert_eq!(
                identity.flags & kms,
                caps.kms_caller_flags(),
                "caps {caps:?}"
            );
            // The KMS bits never leak into or out of the other attested flags.
            assert_eq!(
                identity.flags & CALLER_FLAG_NET_RIGHTS_MASK,
                caps.net_caller_flags(),
                "caps {caps:?}"
            );
            assert_eq!(
                identity.flags & CALLER_FLAG_CONFIG_WRITE,
                caps.config_caller_flags(),
                "caps {caps:?}"
            );
            assert_eq!(
                identity.flags & CALLER_FLAG_VFS_MUTATE,
                super::vfs_mutate_caller_flag(allowlist, caps.vfs_mutate),
                "caps {caps:?}"
            );
        });
    }
}

/// Architecture review H-1: a revoke aimed at one thread removes the right
/// from the whole Cell incarnation — the root and every sibling — so no
/// thread keeps authority the Cell lost.
#[test]
fn cap_revoke_on_a_worker_removes_the_right_from_every_thread_of_the_cell() {
    use abi::syscall::cap_mask as CM;
    const CONTROLLER: usize = 201;
    const ROOT: usize = 202;
    const WORKER: usize = 203;
    const SIBLING: usize = 204;
    let _guard = crate::TEST_STATE_LOCK.lock();
    let mut previous = SCHEDULER.lock();
    let saved = previous.take();
    let mut scheduler = Scheduler::new();
    let mut controller = Task::new(CONTROLLER, CellId(20), "init", alloc::vec::Vec::new());
    super::super::cap::CapSet {
        spawn: true,
        ..super::super::cap::CapSet::EMPTY
    }
    .apply_to(&mut controller);
    scheduler
        .tasks
        .insert(CONTROLLER, alloc::boxed::Box::new(controller));
    let caps = super::super::cap::CapSet {
        spawn: true,
        supervisor: true,
        ..super::super::cap::CapSet::EMPTY
    };
    for tid in [ROOT, WORKER, SIBLING] {
        let mut task = Task::new(tid, CellId(21), "cell", alloc::vec::Vec::new());
        task.root_tid = ROOT;
        task.cell_generation = 0; // one incarnation: (cell, 0)
        caps.apply_to(&mut task);
        scheduler.tasks.insert(tid, alloc::boxed::Box::new(task));
    }
    *previous = Some(scheduler);
    drop(previous);

    assert!([ROOT, WORKER, SIBLING]
        .iter()
        .all(|&tid| super::caller_has_spawn(tid)));
    // The CapRevoke arm minus its teardown (none for these bits) and the
    // victim notification, which needs a live mailbox.
    let mask = CM::SPAWN | CM::SUPERVISOR;
    {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler");
        assert_eq!(
            super::cap_revoke_gate(sched, CONTROLLER, WORKER, mask),
            Ok(())
        );
    }
    assert_eq!(
        super::commit_cap_revoke(CONTROLLER, WORKER, ROOT, (CellId(21), 0), mask),
        Ok(true)
    );
    for tid in [ROOT, WORKER, SIBLING] {
        assert!(!super::caller_has_spawn(tid), "tid {tid} kept SpawnCap");
        assert!(
            !super::caller_has_supervisor(tid),
            "tid {tid} kept SupervisorCap"
        );
    }

    let mut restore = SCHEDULER.lock();
    *restore = saved;
}

/// Review of Phase 07: the record is narrowed even when the targeted thread
/// exits during the teardown, and while the teardown runs the pending mask
/// already withholds the right from every task of the Cell.
#[test]
fn cap_revoke_pending_mask_holds_and_survives_target_exit() {
    use abi::syscall::cap_mask as CM;
    const CONTROLLER: usize = 211;
    const ROOT: usize = 212;
    const WORKER: usize = 213;
    let _guard = crate::TEST_STATE_LOCK.lock();
    let mut previous = SCHEDULER.lock();
    let saved = previous.take();
    let mut scheduler = Scheduler::new();
    let mut controller = Task::new(CONTROLLER, CellId(22), "init", alloc::vec::Vec::new());
    super::super::cap::CapSet {
        spawn: true,
        ..super::super::cap::CapSet::EMPTY
    }
    .apply_to(&mut controller);
    scheduler
        .tasks
        .insert(CONTROLLER, alloc::boxed::Box::new(controller));
    for tid in [ROOT, WORKER] {
        let mut task = Task::new(tid, CellId(23), "cell", alloc::vec::Vec::new());
        task.root_tid = ROOT;
        task.cell_generation = 0; // one incarnation: (cell, 0)
        super::super::cap::CapSet {
            pcie_driver: true,
            supervisor: true,
            ..super::super::cap::CapSet::EMPTY
        }
        .apply_to(&mut task);
        scheduler.tasks.insert(tid, alloc::boxed::Box::new(task));
    }
    *previous = Some(scheduler);
    drop(previous);

    let mask = CM::PCIE_DRIVER;
    let pending = super::PendingRevoke {
        root_tid: ROOT,
        incarnation: (CellId(23), 0),
        mask,
    };
    SCHEDULER
        .lock()
        .as_mut()
        .and_then(|s| s.tasks.get_mut(&ROOT))
        .expect("root")
        .revoke_pending |= mask;
    // During the teardown: withheld from both tasks, the rest kept.
    assert!(!super::caller_has_pcie_driver(WORKER) && !super::caller_has_pcie_driver(ROOT));
    assert!(super::caller_has_supervisor(ROOT));
    // The targeted worker exits before the commit.
    SCHEDULER
        .lock()
        .as_mut()
        .expect("sched")
        .tasks
        .remove(&WORKER);
    assert_eq!(
        super::commit_cap_revoke(CONTROLLER, WORKER, ROOT, (CellId(23), 0), mask),
        Ok(false)
    );
    drop(pending);
    assert!(
        !super::caller_has_pcie_driver(ROOT),
        "root kept the revoked right"
    );
    assert!(super::caller_has_supervisor(ROOT));

    // A failed teardown withdraws only the pending bits.
    let failed = super::PendingRevoke {
        root_tid: ROOT,
        incarnation: (CellId(23), 0),
        mask: CM::SUPERVISOR,
    };
    SCHEDULER
        .lock()
        .as_mut()
        .and_then(|s| s.tasks.get_mut(&ROOT))
        .expect("root")
        .revoke_pending |= CM::SUPERVISOR;
    assert!(!super::caller_has_supervisor(ROOT));
    drop(failed);
    assert!(
        super::caller_has_supervisor(ROOT),
        "failed revoke left the right withheld"
    );

    let mut restore = SCHEDULER.lock();
    *restore = saved;
}

/// Raw opcode 502 (power-off) is accepted only from a Cell whose root image
/// is signed with the program name `shutdown`. A task merely *named*
/// `shutdown` (the basename of a spawner-chosen path, e.g. `/home/x/shutdown`)
/// is denied: this is the architecture review C-1 regression check.
#[test]
fn raw_shutdown_is_accepted_only_for_the_signed_shutdown_image() {
    use abi::cell_image::{ImageIdentity, ANCHOR_DEV};
    let _guard = crate::TEST_STATE_LOCK.lock();
    let mut previous = SCHEDULER.lock();
    let saved = previous.take();
    let mut scheduler = Scheduler::new();
    // tids ≥ 100: other tests latch tid 1 as trusted init for the process.
    // (tid, task name, signed image name, kernel-resolved install path)
    for (tid, name, image, named) in [
        (101usize, "shutdown", Some("shutdown"), true),
        (102, "shutdown", None, true), // unsigned bytes at …/shutdown
        (103, "shutdown", Some("cellbox"), true), // another signed program
        (104, "cellbox", Some("cellbox"), true),
        (105, "shutdown", Some("shutdown"), false), // caller-supplied ELF / sandbox / alias
        (106, "supervisor", Some("supervisor"), true),
    ] {
        let mut task = Task::new(tid, CellId(7), name, alloc::vec::Vec::new());
        match image {
            Some(program) => {
                let signed = ImageIdentity::signed([tid as u8; 32], ANCHOR_DEV, program.as_bytes())
                    .expect("valid image");
                assert!(task.bind_image(signed));
            }
            None => assert!(task.bind_image(ImageIdentity::unsigned([tid as u8; 32]))),
        }
        task.launch_named = named;
        scheduler.tasks.insert(tid, alloc::boxed::Box::new(task));
    }
    *previous = Some(scheduler);
    drop(previous);

    assert_eq!(super::authorize_raw_shutdown(101), Ok(()));
    for tid in [102usize, 103, 104, 105, 106, 199] {
        assert_eq!(
            super::authorize_raw_shutdown(tid),
            Err(SyscallError::PermissionDenied),
            "tid {tid}"
        );
    }
    // The dispatcher refuses the denied callers itself, before any platform code.
    for tid in [102usize, 103, 104, 105, 106] {
        assert_eq!(
            super::handle_syscall(tid, Syscall::Shutdown),
            Err(SyscallError::PermissionDenied)
        );
    }
    // The same identity keys the launch edges: a task's own name never
    // counts, and a signed image counts only from a kernel-resolved path.
    for (tid, expected) in [(102usize, ""), (103, "cellbox"), (105, "")] {
        let (identity, _, _) = super::caller_launch_state(tid).expect("live task");
        assert_eq!(identity, expected, "tid {tid}");
    }

    let mut restore = SCHEDULER.lock();
    *restore = saved;
}

/// A staged command line belongs to the caller: it survives every syscall
/// that is not a launch, and a launch attempt always clears it — including a
/// refusal that never reaches the launch boundary.
///
/// Both halves are load-bearing. The shell stages an argv, reads the cell
/// through VFS, and only then spawns; a syscall that clears the staging on
/// the way loses the command line (`httpd 9091 /tmp/resp.txt` listened on
/// :8080). And a refused attempt must not leave a command line behind for a
/// later child.
#[test]
fn staged_argv_outlives_other_syscalls_and_a_launch_attempt_clears_it() {
    with_scheduler_task(0, |tid| {
        let non_spawn = Syscall::GetTime { op: 0 };
        let spawn = Syscall::SpawnFromPath {
            path_ptr: 0,
            path_len: 0,
        };

        crate::cell::state_stash::discard_spawn_argv(tid);
        assert_eq!(crate::cell::state_stash::stage_spawn_argv(tid, b"argv"), 4);
        assert!(!consumes_staged_spawn_argv(&non_spawn));
        assert!(staged_spawn_argv_cleanup(&non_spawn, tid).is_none());
        assert_eq!(
            crate::cell::state_stash::take_spawn_argv(tid),
            Some(b"argv".to_vec())
        );

        assert_eq!(crate::cell::state_stash::stage_spawn_argv(tid, b"argv"), 4);
        assert!(consumes_staged_spawn_argv(&spawn));
        drop(staged_spawn_argv_cleanup(&spawn, tid));
        assert_eq!(crate::cell::state_stash::take_spawn_argv(tid), None);
    });
}

/// Staging is task-local, so it reaches no live record but its own: a task
/// id with no task behind it stores nothing.
#[test]
fn staging_an_argv_without_a_live_task_stores_nothing() {
    const ABSENT_TID: usize = 0xDEAD_0BEE;
    crate::cell::state_stash::discard_spawn_argv(ABSENT_TID);
    assert_eq!(
        crate::cell::state_stash::stage_spawn_argv(ABSENT_TID, b"argv"),
        0
    );
    assert_eq!(crate::cell::state_stash::take_spawn_argv(ABSENT_TID), None);
}

#[test]
fn telemetry_snapshot_maps_all_four_args_and_validates_them() {
    use crate::task::telemetry::request_is_valid;
    use abi::telemetry::{request, HEADER_LEN};
    let syscall = map_syscall(ViSyscall::TelemetrySnapshot as usize, 0x3000, 4096, 3, 0)
        .expect("TelemetrySnapshot must decode");
    match syscall {
        Syscall::TelemetrySnapshot {
            out_ptr,
            out_len,
            flags,
            reserved,
        } => {
            assert_eq!((out_ptr, out_len, flags, reserved), (0x3000, 4096, 3, 0));
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::TelemetrySnapshot));
    // A transient allocation failure reaches the caller as the typed OOM code.
    assert!(supports_typed_oom(&syscall));
    assert!(request_is_valid(0x3000, HEADER_LEN, request::ALL, 0));
    assert!(
        !request_is_valid(0x3000, HEADER_LEN - 1, 0, 0),
        "short buffer"
    );
    assert!(!request_is_valid(0x3004, 4096, 0, 0), "misaligned buffer");
    assert!(!request_is_valid(0x3000, 4096, 1 << 2, 0), "unknown flag");
    assert!(!request_is_valid(0x3000, 4096, 0, 1), "nonzero a3");
    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::TelemetrySnapshot as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
}

#[test]
fn wait_completion_refuses_mask_bits_above_31_instead_of_dropping_them() {
    use abi::completion::source::{IRQ, TIMER};
    let decode = |a0: usize| match map_syscall(ViSyscall::WaitCompletion as usize, a0, 5, 0, 0x6000)
    {
        Some(Syscall::WaitCompletion { mask, out_ptr, .. }) => (mask, out_ptr),
        other => panic!("decoded wrong syscall variant: {other:?}"),
    };
    let (mask, out_ptr) = decode((IRQ | TIMER) as usize);
    assert_eq!((mask, out_ptr), (IRQ | TIMER, 0x6000));
    assert!(super::super::completion_wait::source_is_valid(
        mask,
        Some(1)
    ));
    #[cfg(target_pointer_width = "64")]
    {
        let (high, _) = decode((1usize << 32) | TIMER as usize);
        assert!(!super::super::completion_wait::source_is_valid(
            high,
            Some(1)
        ));
    }
}

#[test]
fn get_procs2_maps_args_and_dispatch_variant() {
    let syscall =
        map_syscall(ViSyscall::GetProcs2 as usize, 0x1000, 7, 0, 0).expect("GetProcs2 must decode");
    match syscall {
        Syscall::GetProcs2 { buf_ptr, buf_len } => {
            assert_eq!(buf_ptr, 0x1000);
            assert_eq!(buf_len, 7);
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::GetProcs2));
}

#[test]
fn get_procs2_allowlist_denies_without_bit_55() {
    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::GetProcs2 as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
}

#[test]
fn get_procs2_allowlist_allows_with_bit_55() {
    with_scheduler_task(1u64 << 55, |tid| {
        assert_eq!(check_allowlist(ViSyscall::GetProcs2 as usize, tid), Ok(()));
    });
}

#[test]
fn syscall_result_encoding_preserves_generic_and_additive_oom_codes() {
    assert_eq!(encode_syscall_result(Ok(9), usize::MAX, false), 9);
    assert_eq!(
        encode_syscall_result(Err(SyscallError::InvalidInput), usize::MAX, true),
        usize::MAX
    );
    assert_eq!(
        encode_syscall_result(Err(SyscallError::OutOfMemory), usize::MAX, true),
        usize::MAX - 1
    );
    assert_eq!(
        encode_syscall_result(Err(SyscallError::OutOfMemory), u32::MAX as usize, true),
        u32::MAX as usize - 1
    );
    assert_eq!(
        encode_syscall_result(Err(SyscallError::OutOfMemory), usize::MAX, false),
        usize::MAX
    );
    assert!(supports_typed_oom(&Syscall::SpawnFromPath {
        path_ptr: 0,
        path_len: 0,
    }));
    assert!(!supports_typed_oom(&Syscall::MemInfo {
        out_ptr: 0,
        out_len: 0,
    }));
}

/// Phase 09B K1: `QueryRetirement` (431) decodes a0..a2, is admitted past
/// an empty allowlist, and its refusals are typed: bad length, no record,
/// not a watcher. A watcher passes the gate and reaches the user copy.
#[test]
fn query_retirement_is_gated_by_the_verdict_with_typed_errors() {
    use crate::cell::retirement_verdict;
    let syscall = map_syscall(ViSyscall::QueryRetirement as usize, 77, 0x5000, 96, 0)
        .expect("QueryRetirement must decode");
    match syscall {
        Syscall::QueryRetirement {
            root_tid,
            out_ptr,
            out_len,
        } => {
            assert_eq!((root_tid, out_ptr, out_len), (77, 0x5000, 96));
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    // No allowlist bit: admitted past an empty allowlist; dispatch decides.
    assert_eq!(
        syscall_to_vi(&syscall).and_then(ViSyscall::allowlist_bit),
        None
    );
    assert!(super::has_typed_errors(&syscall));
    let len = abi::retirement::RETIREMENT_RECORD_LEN;
    let query = |tid, root_tid, out_ptr, out_len| {
        super::handle_syscall(
            tid,
            Syscall::QueryRetirement {
                root_tid,
                out_ptr,
                out_len,
            },
        )
    };
    with_scheduler_task_caps(0, CapSet::EMPTY, |tid| {
        retirement_verdict::tests::reset();
        assert_eq!(
            check_allowlist(ViSyscall::QueryRetirement as usize, tid),
            Ok(())
        );
        let watched = abi::cell_owner::CellOwner::new(0x431, 3, 77);
        let foreign = abi::cell_owner::CellOwner::new(0x432, 3, 78);
        assert!(retirement_verdict::open(watched, &[tid]));
        assert!(retirement_verdict::open(foreign, &[tid + 1]));

        let bad_len = query(tid, 77, 0x5000, len - 1);
        assert_eq!(bad_len, Err(SyscallError::InvalidInput));
        assert_eq!(super::encode_ipc_result(bad_len), usize::MAX - 3);
        let missing = query(tid, 79, 0x5000, len);
        assert_eq!(missing, Err(SyscallError::FileNotFound));
        assert_eq!(super::encode_ipc_result(missing), usize::MAX - 2);
        let denied = query(tid, 78, 0x5000, len);
        assert_eq!(denied, Err(SyscallError::PermissionDenied));
        assert_eq!(super::encode_ipc_result(denied), usize::MAX);
        assert_eq!(
            query(tid, 77, 0, len),
            Err(SyscallError::InvalidInput),
            "gate passed"
        );
        retirement_verdict::tests::reset();
    });
}

/// Inserts a live task into the scheduler installed by `with_scheduler_task_caps`.
fn insert_block_route_task(tid: usize, cell: u64, generation: u64, caps: CapSet) {
    let mut task = alloc::boxed::Box::new(Task::new(
        tid,
        CellId(cell),
        "block-route-test",
        alloc::vec::Vec::new(),
    ));
    task.cell_generation = generation;
    caps.apply_to(&mut task);
    SCHEDULER
        .lock()
        .as_mut()
        .expect("scheduler")
        .tasks
        .insert(tid, task);
}

fn block_routes_snapshot(
) -> [Option<abi::block_route::ViBlockRouteV1>; abi::block_route::MAX_BLOCK_ROUTES] {
    core::array::from_fn(crate::dev::driver_cell::block_route_record)
}

/// Phase 09C D4: `RegisterBlockDriver` (416) decodes a0..a3 with typed
/// errors, and binds only the route of the device the caller owns.
#[test]
fn register_block_driver_binds_only_the_callers_owned_device_route() {
    use crate::dev::driver_cell::{deregister_all_for, reset_block_routes};
    use crate::resource_registry::{claim_bdf_owner, release_bdfs_for};
    use abi::block_route::MAX_BLOCK_ROUTES;
    const BDF: usize = 0x4160;
    const FOREIGN_BDF: usize = 0x4161;
    const BASE: usize = 0x4_1600_0000;
    let syscall = map_syscall(ViSyscall::RegisterBlockDriver as usize, 1, 0xFA, 2, 3)
        .expect("RegisterBlockDriver must decode");
    match syscall {
        Syscall::RegisterBlockDriver { kind, id, reserved } => {
            assert_eq!((kind, id, reserved), (1, 0xFA, (2, 3)));
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    assert!(super::has_typed_errors(&syscall));

    let register = |tid, kind, id, reserved| {
        super::handle_syscall(tid, Syscall::RegisterBlockDriver { kind, id, reserved })
    };
    let driver = CapSet {
        pcie_driver: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(u64::MAX, driver, |tid| {
        reset_block_routes();
        SCHEDULER
            .lock()
            .as_mut()
            .expect("scheduler")
            .tasks
            .get_mut(&tid)
            .expect("caller")
            .cell_generation = 3;
        insert_block_route_task(2, 8, 5, driver);
        insert_block_route_task(3, 9, 1, CapSet::EMPTY);
        insert_block_route_task(
            4,
            10,
            1,
            CapSet {
                usb_driver: true,
                ..CapSet::EMPTY
            },
        );
        insert_block_route_task(6, 11, 1, driver);
        SCHEDULER
            .lock()
            .as_mut()
            .expect("scheduler")
            .tasks
            .get_mut(&6)
            .expect("retiring task")
            .state = crate::task::tcb::TaskState::Retiring;
        insert_block_route_task(
            5,
            10,
            1,
            CapSet {
                usb_driver: true,
                ..CapSet::EMPTY
            },
        );
        assert!(claim_bdf_owner(BDF as u32, tid));
        assert!(claim_bdf_owner(FOREIGN_BDF as u32, 2));
        // `RequestMmio` records the claiming task id as the window owner.
        assert!(crate::resource_registry::request_mmio_unchecked(4, BASE, 0x1000).is_ok());
        let empty = block_routes_snapshot();

        for (kind, id, reserved) in [
            (0, BDF, (0, 0)),
            (3, BDF, (0, 0)),
            (0x1_0001, BDF, (0, 0)),
            (1, 0x1_0000, (0, 0)),
            (2, 0, (0, 0)),
            (1, BDF, (1, 0)),
            (1, BDF, (0, 1)),
        ] {
            let malformed = register(tid, kind, id, reserved);
            assert_eq!(malformed, Err(SyscallError::InvalidInput), "{kind} {id:#x}");
            assert_eq!(super::encode_ipc_result(malformed), usize::MAX - 3);
        }
        for (caller, kind, id) in [(tid, 1, FOREIGN_BDF), (tid, 1, 0x4162), (tid, 2, BASE)] {
            let not_owner = register(caller, kind, id, (0, 0));
            assert_eq!(not_owner, Err(SyscallError::FileNotFound), "{kind} {id:#x}");
            assert_eq!(super::encode_ipc_result(not_owner), usize::MAX - 2);
        }
        assert!(claim_bdf_owner(0x4163, 3));
        assert!(claim_bdf_owner(0x4164, 6));
        for (caller, bdf) in [(3, 0x4163), (6, 0x4164)] {
            let denied = register(caller, 1, bdf, (0, 0));
            assert_eq!(
                denied,
                Err(SyscallError::PermissionDenied),
                "no cap / retiring"
            );
            assert_eq!(super::encode_ipc_result(denied), usize::MAX);
        }
        assert_eq!(
            block_routes_snapshot(),
            empty,
            "refusals leave the table untouched"
        );

        assert_eq!(register(tid, 1, BDF, (0, 0)), Ok(0), "first bind appends");
        assert_eq!(register(tid, 1, BDF, (0, 0)), Ok(0), "repeat is idempotent");
        assert_eq!(
            register(4, 2, BASE, (0, 0)),
            Ok(0),
            "second device, second slot"
        );
        let routes = block_routes_snapshot();
        let first = routes[0].expect("slot 0");
        assert_eq!(
            (first.kind, first.id, first.provider()),
            (1, BDF as u64, Some((1, 7, 3)))
        );
        let second = routes[1].expect("slot 1");
        assert_eq!(
            (second.kind, second.id, second.provider()),
            (2, BASE as u64, Some((4, 10, 1)))
        );
        assert!(routes[2].is_none());

        assert_eq!(
            register(5, 2, BASE, (0, 0)),
            Err(SyscallError::FileNotFound),
            "another task of the same Cell does not own the window task 4 claimed"
        );
        assert_eq!(block_routes_snapshot(), routes);

        deregister_all_for(tid);
        let released = block_routes_snapshot()[0].expect("key kept");
        assert_eq!(
            (released.kind, released.id, released.provider()),
            (1, BDF as u64, None)
        );
        release_bdfs_for(tid);
        assert!(claim_bdf_owner(BDF as u32, 2));
        assert_eq!(
            register(2, 1, BDF, (0, 0)),
            Ok(0),
            "new owner regains the same slot"
        );
        assert_eq!(
            block_routes_snapshot()[0].and_then(|r| r.provider()),
            Some((2, 8, 5))
        );
        assert_eq!(
            block_routes_snapshot()[1],
            routes[1],
            "sibling route untouched"
        );

        for bdf in 0x4170..0x4170 + (MAX_BLOCK_ROUTES - 2) {
            assert!(claim_bdf_owner(bdf as u32, 2));
            assert_eq!(register(2, 1, bdf, (0, 0)), Ok(0));
        }
        assert!(claim_bdf_owner(0x4180, 2));
        let full = block_routes_snapshot();
        let refused = register(2, 1, 0x4180, (0, 0));
        assert_eq!(refused, Err(SyscallError::PermissionDenied), "table full");
        assert_eq!(super::encode_ipc_result(refused), usize::MAX);
        assert_eq!(block_routes_snapshot(), full);

        assert_eq!(
            crate::cell::service_registry::lookup(abi::syscall::service::BLOCK_DRIVER),
            None
        );

        for owner in [1, 2, 3, 4, 5, 6] {
            release_bdfs_for(owner);
            deregister_all_for(owner);
        }
        crate::resource_registry::release_mmio_for(4);
        reset_block_routes();
    });
}

/// Phase 09C D4: `LookupBlockRoute` (432) is admitted past the allowlist,
/// gated on BlockIoCap or SpawnCap, and its refusals are typed.
#[test]
fn lookup_block_route_is_gated_on_block_io_or_spawn_with_typed_errors() {
    use crate::dev::driver_cell::{
        block_route_record, deregister_all_for, publish_block_route, reset_block_routes,
    };
    use abi::block_route::{DeviceKey, ViBlockRouteV1, BLOCK_DEVICE_PCI, BLOCK_ROUTE_RECORD_LEN};
    let syscall = map_syscall(ViSyscall::LookupBlockRoute as usize, 3, 0x5000, 40, 0)
        .expect("LookupBlockRoute must decode");
    match syscall {
        Syscall::LookupBlockRoute {
            index,
            out_ptr,
            out_len,
            reserved,
        } => {
            assert_eq!((index, out_ptr, out_len, reserved), (3, 0x5000, 40, 0));
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    // No allowlist bit: admitted past an empty allowlist; dispatch decides.
    assert_eq!(
        syscall_to_vi(&syscall).and_then(ViSyscall::allowlist_bit),
        None
    );
    assert!(super::has_typed_errors(&syscall));
    let len = BLOCK_ROUTE_RECORD_LEN;
    let lookup = |tid, index, out_ptr, out_len, reserved| {
        super::handle_syscall(
            tid,
            Syscall::LookupBlockRoute {
                index,
                out_ptr,
                out_len,
                reserved,
            },
        )
    };
    let key = DeviceKey::new(BLOCK_DEVICE_PCI, 0xFA).expect("valid BDF");

    let block_io = CapSet {
        block_io: true,
        ..CapSet::EMPTY
    };
    let spawn = CapSet {
        spawn: true,
        ..CapSet::EMPTY
    };
    for caps in [CapSet::EMPTY, block_io, spawn] {
        with_scheduler_task_caps(0, caps, |tid| {
            reset_block_routes();
            assert_eq!(publish_block_route(key, (9, 90, 2)), Some(0));
            assert_eq!(
                check_allowlist(ViSyscall::LookupBlockRoute as usize, tid),
                Ok(())
            );
            if caps == CapSet::EMPTY {
                let denied = lookup(tid, 0, 0x5000, len, 0);
                assert_eq!(denied, Err(SyscallError::PermissionDenied));
                assert_eq!(super::encode_ipc_result(denied), usize::MAX);
                reset_block_routes();
                return;
            }
            for (out_len, reserved) in [(len - 1, 0), (len + 1, 0), (len, 1)] {
                let bad = lookup(tid, 0, 0x5000, out_len, reserved);
                assert_eq!(bad, Err(SyscallError::InvalidInput));
                assert_eq!(super::encode_ipc_result(bad), usize::MAX - 3);
            }
            for index in [1, 8, usize::MAX] {
                let missing = lookup(tid, index, 0x5000, len, 0);
                assert_eq!(missing, Err(SyscallError::FileNotFound), "index {index}");
                assert_eq!(super::encode_ipc_result(missing), usize::MAX - 2);
            }
            assert_eq!(
                lookup(tid, 0, 0, len, 0),
                Err(SyscallError::InvalidInput),
                "gate passed"
            );

            let record = block_route_record(0).expect("bound slot");
            let decoded = ViBlockRouteV1::decode(&record.encode()).expect("valid record bytes");
            assert_eq!(decoded.device(), key);
            assert_eq!(decoded.provider(), Some((9, 90, 2)));
            deregister_all_for(9);
            let released = block_route_record(0).expect("key kept");
            let decoded = ViBlockRouteV1::decode(&released.encode()).expect("valid record bytes");
            assert_eq!(
                (decoded.device(), decoded.provider(), decoded.flags),
                (key, None, 0)
            );
            reset_block_routes();
        });
    }
}

/// Phase 09C R3: `BLOCK_DRIVER` is retired; not even a SpawnCap holder may
/// register it.
#[test]
fn register_service_refuses_the_retired_block_driver_id() {
    let spawn = CapSet {
        spawn: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(0, spawn, |tid| {
        let refused = super::handle_syscall(
            tid,
            Syscall::RegisterService {
                service_id: abi::syscall::service::BLOCK_DRIVER,
                tid,
                flags: 0,
            },
        );
        assert_eq!(refused, Err(SyscallError::PermissionDenied));
        assert_eq!(
            crate::cell::service_registry::lookup(abi::syscall::service::BLOCK_DRIVER),
            None
        );
    });
}

/// ADR-0029 §3: `QueryMachineFacts` (433) is admitted past the allowlist,
/// gated on SpawnCap, validates its arguments before computing anything,
/// and reports "no facts" while no x86 profile is bound.
#[test]
fn query_machine_facts_is_gated_on_spawn_with_typed_errors() {
    use abi::machine_facts::MACHINE_FACTS_CAPACITY as CAP;
    let syscall = map_syscall(ViSyscall::QueryMachineFacts as usize, 0x5000, CAP, 0, 0)
        .expect("QueryMachineFacts must decode");
    match syscall {
        Syscall::QueryMachineFacts {
            out_ptr,
            out_len,
            a2,
            a3,
        } => {
            assert_eq!((out_ptr, out_len, a2, a3), (0x5000, CAP, 0, 0));
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    // No allowlist bit: admitted past an empty allowlist; dispatch decides.
    assert_eq!(
        syscall_to_vi(&syscall).and_then(ViSyscall::allowlist_bit),
        None
    );
    assert!(super::has_typed_errors(&syscall));
    let query = |tid, out_len, a2, a3| {
        super::handle_syscall(
            tid,
            Syscall::QueryMachineFacts {
                out_ptr: 0x5000,
                out_len,
                a2,
                a3,
            },
        )
    };
    let spawn = CapSet {
        spawn: true,
        ..CapSet::EMPTY
    };
    for caps in [CapSet::EMPTY, spawn] {
        with_scheduler_task_caps(0, caps, |tid| {
            assert_eq!(
                check_allowlist(ViSyscall::QueryMachineFacts as usize, tid),
                Ok(())
            );
            if caps == CapSet::EMPTY {
                let denied = query(tid, CAP, 0, 0);
                assert_eq!(denied, Err(SyscallError::PermissionDenied));
                assert_eq!(super::encode_ipc_result(denied), usize::MAX);
                return;
            }
            for (out_len, a2, a3) in [
                (CAP - 1, 0, 0),
                (CAP + 1, 0, 0),
                (0, 0, 0),
                (CAP, 1, 0),
                (CAP, 0, 1),
            ] {
                let bad = query(tid, out_len, a2, a3);
                assert_eq!(bad, Err(SyscallError::InvalidInput), "{out_len} {a2} {a3}");
                assert_eq!(super::encode_ipc_result(bad), usize::MAX - 3);
            }
            assert!(
                crate::board::x86_machine_inputs().is_none(),
                "host tests bind no profile"
            );
            let missing = query(tid, CAP, 0, 0);
            assert_eq!(missing, Err(SyscallError::FileNotFound));
            assert_eq!(super::encode_ipc_result(missing), usize::MAX - 2);
        });
    }
}

/// ADR-0031 CP1 §6: with two functions of one vendor:device, the first
/// driver claims A, a second driver gets B, a third gets nothing, and an
/// owner's re-query still reaches its own function.
#[test]
fn vendor_lookup_skips_functions_owned_by_another_driver() {
    use crate::dev::pcie_ecam::{self, Bar, PciDevice};
    use crate::resource_registry::{claim_bdf_owner, release_bdfs_for};
    const VENDOR: u16 = 0x1b36;
    const DEVICE: u16 = 0xfa04;
    let function = |fun| PciDevice {
        bdf: (0xee, 3, fun),
        vendor_id: VENDOR,
        device_id: DEVICE,
        class: 0xff,
        subclass: 0,
        prog_if: 0,
        bars: [Bar::None; 6],
        msix: None,
        pm: None,
    };
    pcie_ecam::test_insert_devices(&[function(0), function(1)]);
    let (first, second, third) = (0x9001, 0x9002, 0x9003);
    let find = |tid| {
        pcie_ecam::find_vendor_device(VENDOR, DEVICE, |bdf| super::pcie_usable_by(tid, bdf))
            .map(|d| d.canonical_bdf())
    };
    let a = find(first).expect("function A offered");
    assert!(claim_bdf_owner(a, first));
    let b = find(second).expect("function B offered to a second driver");
    assert_ne!(a, b);
    assert!(claim_bdf_owner(b, second));
    assert_eq!(find(third), None);
    assert_eq!(find(second), Some(b));
    release_bdfs_for(first);
    assert_eq!(
        find(third),
        Some(a),
        "a reaped owner's function is offered again"
    );
    release_bdfs_for(second);
    pcie_ecam::test_remove_devices(VENDOR);
}

/// 434/435 refuse a caller without PcieDriverCap, and their sentinels stay
/// distinct so a driver can tell "keep polling" from "not yours".
#[test]
fn pcie_function_calls_are_gated_and_typed() {
    use abi::pcie_function as pf;
    let query = Syscall::QueryPcieFunction {
        bdf: 0x18,
        out_ptr: 0x5000,
        out_len: pf::PCIE_FUNCTION_RECORD_LEN,
        reserved: 0,
    };
    let alloc = Syscall::AllocPcieIrq {
        bdf: 0x18,
        want: 1,
        mode: pf::IRQ_MODE_ALLOW_MSIX,
        reserved: 0,
    };
    assert!(super::is_pcie_function_call(&query) && super::is_pcie_function_call(&alloc));
    for call in [query, alloc] {
        let result = super::handle_syscall(0x9abc, call);
        assert!(matches!(result, Err(SyscallError::PermissionDenied)));
        assert_eq!(
            super::encode_pcie_function_result(result),
            pf::ERR_PERMISSION_DENIED
        );
    }
    let encode = |e| super::encode_pcie_function_result(Err(e));
    assert_eq!(encode(SyscallError::NotSupported), pf::ERR_NOT_SUPPORTED);
    assert_eq!(encode(SyscallError::OutOfMemory), pf::ERR_NO_VECTORS);
    assert_eq!(encode(SyscallError::FileNotFound), pf::ERR_NOT_FOUND);
    assert_eq!(encode(SyscallError::InvalidInput), pf::ERR_INVALID_INPUT);
}

/// 442 decodes with its reserved words, uses the typed sentinels, refuses a
/// caller without PcieDriverCap, and on a host (no VirtIO MMIO) answers a
/// PcieDriverCap holder `NotSupported` once the arguments are well formed.
#[test]
fn probe_virtio_mmio_is_gated_and_typed() {
    use abi::syscall::virtio_probe as vp;
    let decoded = super::map_syscall(442, 0x1000_1000, 0, 0, 7);
    assert!(matches!(
        decoded,
        Some(Syscall::ProbeVirtioMmio {
            base: 0x1000_1000,
            reserved: (0, 0, 7)
        })
    ));
    assert_eq!(
        decoded.as_ref().and_then(super::syscall_to_vi),
        Some(abi::syscall::ViSyscall::ProbeVirtioMmio)
    );
    let call = |reserved| Syscall::ProbeVirtioMmio {
        base: 0x1000_1000,
        reserved,
    };
    assert!(super::has_typed_errors(&call((0, 0, 0))));
    assert!(matches!(
        super::handle_syscall(0x9abc, call((0, 0, 0))),
        Err(SyscallError::PermissionDenied)
    ));
    let encode = |e| super::encode_ipc_result(Err(e));
    assert_eq!(encode(SyscallError::FileNotFound), vp::ERR_NOT_FOUND);
    assert_eq!(encode(SyscallError::InvalidInput), vp::ERR_INVALID_INPUT);
    let driver = CapSet {
        pcie_driver: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(u64::MAX, driver, |tid| {
        assert!(matches!(
            super::handle_syscall(tid, call((0, 1, 0))),
            Err(SyscallError::InvalidInput)
        ));
        assert!(matches!(
            super::handle_syscall(tid, call((0, 0, 0))),
            Err(SyscallError::NotSupported)
        ));
    });
}

/// CP1 §5 (D-4): 436 decodes to `SystemPower` with bit 49 and is refused
/// with `PermissionDenied` to any caller without `SupervisorCap`, before
/// the action is looked at, including holders of other authority.
#[test]
fn system_power_is_refused_without_supervisor_cap() {
    use abi::syscall::system_power as sp;
    let decoded = super::map_syscall(436, sp::ACTION_POWEROFF, 0, 0, 0);
    assert!(matches!(
        decoded,
        Some(Syscall::SystemPower {
            action: sp::ACTION_POWEROFF,
            a1: 0,
            a2: 0,
            a3: 0
        })
    ));
    assert_eq!(
        decoded.as_ref().and_then(super::syscall_to_vi),
        Some(abi::syscall::ViSyscall::SystemPower)
    );
    let call = |action| Syscall::SystemPower {
        action,
        a1: 0,
        a2: 0,
        a3: 0,
    };
    for action in [sp::ACTION_REBOOT, sp::ACTION_POWEROFF] {
        let result = super::handle_syscall(0x9abc, call(action));
        assert!(matches!(result, Err(SyscallError::PermissionDenied)));
        assert_eq!(
            super::encode_system_power_result(result),
            sp::ERR_PERMISSION_DENIED
        );
    }
    let other_authority = CapSet {
        spawn: true,
        pcie_driver: true,
        block_io: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(u64::MAX, other_authority, |tid| {
        for action in [sp::ACTION_REBOOT, sp::ACTION_POWEROFF] {
            assert!(matches!(
                super::handle_syscall(tid, call(action)),
                Err(SyscallError::PermissionDenied)
            ));
        }
    });
}

/// With `SupervisorCap`, an unknown action or a non-zero a1..a3 is
/// `InvalidInput`, and a poweroff with no recorded ACPI facts (no
/// `\_S5_`) is `NotSupported`; each keeps its own sentinel.
#[test]
fn system_power_checks_action_and_arguments_behind_the_gate() {
    use abi::syscall::system_power as sp;
    let supervisor = CapSet {
        supervisor: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(u64::MAX, supervisor, |tid| {
        let run = |action, a1, a2, a3| {
            super::encode_system_power_result(super::handle_syscall(
                tid,
                Syscall::SystemPower { action, a1, a2, a3 },
            ))
        };
        for action in [0, 3, usize::MAX] {
            assert_eq!(run(action, 0, 0, 0), sp::ERR_INVALID_INPUT);
        }
        assert_eq!(run(sp::ACTION_POWEROFF, 1, 0, 0), sp::ERR_INVALID_INPUT);
        assert_eq!(run(sp::ACTION_REBOOT, 0, 0, 1), sp::ERR_INVALID_INPUT);
        assert_eq!(run(sp::ACTION_POWEROFF, 0, 0, 0), sp::ERR_NOT_SUPPORTED);
    });
}

/// ADR-0029 §3 (CP1 Q9): only trusted init, naming itself, may register
/// `MACHINE_PIN`; a SpawnCap holder that is not init is refused.
#[test]
fn register_service_admits_machine_pin_only_from_trusted_init_itself() {
    use abi::syscall::service::MACHINE_PIN;
    let spawn = CapSet {
        spawn: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(0, spawn, |tid| {
        let register = |tid, target| {
            super::handle_syscall(
                tid,
                Syscall::RegisterService {
                    service_id: MACHINE_PIN,
                    tid: target,
                    flags: 0,
                },
            )
        };
        if super::TRUSTED_INIT_TID.load(core::sync::atomic::Ordering::Acquire) != tid {
            assert_eq!(
                register(tid, tid),
                Err(SyscallError::PermissionDenied),
                "not init"
            );
            assert_eq!(crate::cell::service_registry::lookup(MACHINE_PIN), None);
        }
        super::publish_trusted_init(tid);
        assert!(super::caller_is_trusted_init(tid));
        assert_eq!(
            register(tid, tid + 1),
            Err(SyscallError::PermissionDenied),
            "not itself"
        );
        assert_eq!(
            register(tid, 0),
            Err(SyscallError::PermissionDenied),
            "not itself"
        );
        assert_eq!(register(tid, tid), Ok(0));
        assert_eq!(
            crate::cell::service_registry::lookup(MACHINE_PIN),
            Some(tid)
        );
        crate::cell::service_registry::clear_tid(tid);
    });
}

/// H-11: SpawnCap alone cannot rebind a well-known id (the USB host driver
/// holds spawn for its LAN child); a SupervisorCap holder still rebinds it,
/// and the spawn-only cell keeps publishing its own private id.
#[test]
fn register_service_refuses_well_known_hijack_by_a_spawn_only_cell() {
    use abi::syscall::service::VFS;
    const HIJACKER: usize = 0x5_1D01;
    const SUPERVISOR: usize = 0x5_1D02;
    const PRIVATE_ID: u16 = 0x7A5E;
    with_scheduler_task_caps(0, CapSet::EMPTY, |_| {
        for (tid, caps) in [
            (
                HIJACKER,
                CapSet {
                    spawn: true,
                    usb_driver: true,
                    ..CapSet::EMPTY
                },
            ),
            (
                SUPERVISOR,
                CapSet {
                    spawn: true,
                    supervisor: true,
                    ..CapSet::EMPTY
                },
            ),
        ] {
            let mut task = alloc::boxed::Box::new(Task::new(
                tid,
                CellId(tid as u64),
                "register-service-test",
                alloc::vec::Vec::new(),
            ));
            caps.apply_to(&mut task);
            SCHEDULER
                .lock()
                .as_mut()
                .expect("test scheduler")
                .tasks
                .insert(tid, task);
        }
        let register_flagged = |caller, service_id, tid, flags| {
            super::handle_syscall(
                caller,
                Syscall::RegisterService {
                    service_id,
                    tid,
                    flags,
                },
            )
        };
        let register = |caller, service_id, tid| register_flagged(caller, service_id, tid, 0);
        assert_eq!(
            register(HIJACKER, VFS, HIJACKER),
            Err(SyscallError::PermissionDenied)
        );
        assert_ne!(crate::cell::service_registry::lookup(VFS), Some(HIJACKER));
        assert_eq!(register(SUPERVISOR, VFS, SUPERVISOR), Ok(0));
        assert_eq!(crate::cell::service_registry::lookup(VFS), Some(SUPERVISOR));
        // D1: flags are the namespace owner's, and undefined bits are refused.
        let flags = abi::syscall::service_flags::ALL;
        assert_eq!(
            register_flagged(HIJACKER, PRIVATE_ID, HIJACKER, flags),
            Err(SyscallError::PermissionDenied)
        );
        assert_eq!(
            register_flagged(SUPERVISOR, VFS, SUPERVISOR, flags + 1),
            Err(SyscallError::InvalidInput)
        );
        assert_eq!(register_flagged(SUPERVISOR, VFS, SUPERVISOR, flags), Ok(0));
        let generation = SCHEDULER
            .lock()
            .as_ref()
            .and_then(|sched| sched.tasks.get(&SUPERVISOR))
            .map(|task| task.cell_generation)
            .expect("supervisor task");
        assert!(crate::cell::service_registry::retains_outer_caller(
            SUPERVISOR as u64,
            generation
        ));
        assert_eq!(register(HIJACKER, PRIVATE_ID, HIJACKER), Ok(0));
        crate::cell::service_registry::clear_tid(HIJACKER);
        crate::cell::service_registry::clear_tid(SUPERVISOR);
    });
}

#[test]
fn mem_info_maps_args_and_requires_bit_56() {
    let syscall =
        map_syscall(ViSyscall::MemInfo as usize, 0x2000, 32, 0, 0).expect("MemInfo must decode");
    match syscall {
        Syscall::MemInfo { out_ptr, out_len } => {
            assert_eq!(out_ptr, 0x2000);
            assert_eq!(out_len, 32);
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::MemInfo));
    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::MemInfo as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task(1u64 << 56, |tid| {
        assert_eq!(check_allowlist(ViSyscall::MemInfo as usize, tid), Ok(()));
    });
}

#[test]
fn get_cpu_count_maps_and_is_always_permitted() {
    let syscall =
        map_syscall(ViSyscall::GetCpuCount as usize, 0, 0, 0, 0).expect("GetCpuCount must decode");
    assert!(matches!(syscall, Syscall::GetCpuCount));
    assert_eq!(syscall_to_vi(&syscall), Some(ViSyscall::GetCpuCount));
    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::GetCpuCount as usize, tid),
            Ok(())
        );
    });
}

/// Without `bench-counters`, raw 504 is exactly as unknown as before: no
/// decode, and the allowlist gate denies it even with MemInfo's bit.
#[cfg(not(feature = "bench-counters"))]
#[test]
fn bench_counters_opcode_is_unknown_without_the_feature() {
    assert!(map_syscall(504, 0x2000, 64, 0, 0).is_none());
    with_scheduler_task(1u64 << 56, |tid| {
        assert_eq!(
            check_allowlist(504, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
}

/// With `bench-counters`, raw 504 decodes outside `ViSyscall` and is gated
/// exactly like MemInfo: allowlist bit 56 plus the `observe_memory` right.
#[cfg(feature = "bench-counters")]
#[test]
fn bench_counters_opcode_maps_and_is_gated_like_mem_info() {
    let syscall = map_syscall(504, 0x2000, 64, 0, 0).expect("504 must decode");
    match syscall {
        Syscall::BenchCounters { out_ptr, out_len } => {
            assert_eq!(out_ptr, 0x2000);
            assert_eq!(out_len, 64);
        }
        other => panic!("decoded wrong syscall variant: {other:?}"),
    }
    assert_eq!(syscall_to_vi(&syscall), None);
    assert_eq!(ViSyscall::from(504), ViSyscall::Unknown);
    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(504, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task(1u64 << 55, |tid| {
        assert_eq!(
            check_allowlist(504, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task(1u64 << 56, |tid| {
        assert_eq!(check_allowlist(504, tid), Ok(()));
        // A short buffer is refused before any user access.
        assert_eq!(
            super::handle_syscall(
                tid,
                Syscall::BenchCounters {
                    out_ptr: 0x2000,
                    out_len: 63
                }
            ),
            Err(SyscallError::InvalidInput)
        );
    });
    // The bit alone is not enough: the edge must grant observe_memory.
    for allowlist in [1u64 << 56, u64::MAX] {
        with_scheduler_task_caps(allowlist, CapSet::EMPTY, |tid| {
            assert_eq!(
                check_allowlist(504, tid),
                Err(SyscallError::PermissionDenied)
            );
        });
    }
    let m_only = CapSet {
        observe_memory: true,
        ..CapSet::EMPTY
    };
    with_scheduler_task_caps(1u64 << 56, m_only, |tid| {
        assert_eq!(check_allowlist(504, tid), Ok(()));
    });
    // Kernel context (tid 0) is not gated, as for MemInfo.
    let _guard = crate::TEST_STATE_LOCK.lock();
    assert_eq!(check_allowlist(504, 0), Ok(()));
}

#[test]
fn cwd_syscalls_map_args_and_require_bit_60() {
    let chdir = map_syscall(ViSyscall::Chdir as usize, 0x3000, 4, 0, 0).expect("Chdir must decode");
    assert!(matches!(
        chdir,
        Syscall::ChDir {
            path_ptr: 0x3000,
            path_len: 4
        }
    ));
    assert_eq!(syscall_to_vi(&chdir), Some(ViSyscall::Chdir));

    let getcwd =
        map_syscall(ViSyscall::Getcwd as usize, 0x4000, 64, 0, 0).expect("Getcwd must decode");
    assert!(matches!(
        getcwd,
        Syscall::GetCwd {
            buf_ptr: 0x4000,
            buf_len: 64
        }
    ));
    assert_eq!(syscall_to_vi(&getcwd), Some(ViSyscall::Getcwd));

    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::Chdir as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
        assert_eq!(
            check_allowlist(ViSyscall::Getcwd as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task(1u64 << 60, |tid| {
        assert_eq!(check_allowlist(ViSyscall::Chdir as usize, tid), Ok(()));
        assert_eq!(check_allowlist(ViSyscall::Getcwd as usize, tid), Ok(()));
    });
}

#[test]
fn fstat_maps_args_requires_bit_61_and_leaves_106_as_seek() {
    let fstat =
        map_syscall(ViSyscall::Fstat as usize, 7, 0x5000, 32, 0).expect("Fstat must decode");
    assert!(matches!(
        fstat,
        Syscall::Fstat {
            fd: 7,
            out_ptr: 0x5000,
            out_len: 32
        }
    ));
    assert_eq!(syscall_to_vi(&fstat), Some(ViSyscall::Fstat));

    let seek = map_syscall(106, 7, usize::MAX, 2, 0).expect("Seek 106 must decode");
    assert!(matches!(
        seek,
        Syscall::Seek {
            fd: 7,
            offset: -1,
            whence: 2
        }
    ));
    let rename = map_syscall(ViSyscall::Rename as usize, 0x1000, 10, 0x2000, 20)
        .expect("Rename must decode");
    assert!(matches!(
        rename,
        Syscall::Rename {
            old_ptr: 0x1000,
            old_len: 10,
            new_ptr: 0x2000,
            new_len: 20
        }
    ));
    assert_eq!(syscall_to_vi(&rename), Some(ViSyscall::Rename));

    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::Rename as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task(1u64 << 62, |tid| {
        assert_eq!(check_allowlist(ViSyscall::Rename as usize, tid), Ok(()));
    });
    with_scheduler_task(1u64 << 63, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::Rename as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task((1u64 << 62) | (1u64 << 63), |tid| {
        assert_eq!(check_allowlist(ViSyscall::Rename as usize, tid), Ok(()));
    });

    with_scheduler_task(0, |tid| {
        assert_eq!(
            check_allowlist(ViSyscall::Fstat as usize, tid),
            Err(SyscallError::PermissionDenied)
        );
    });
    with_scheduler_task(1u64 << 61, |tid| {
        assert_eq!(check_allowlist(ViSyscall::Fstat as usize, tid), Ok(()));
    });
}

#[test]
fn dma_authority_requires_live_owner_binding_and_contained_range() {
    const OWNER: usize = 77;
    const BASE: usize = 0x5100_0000;
    const SIZE: usize = 0x4000;
    let binding = (CellId(9), 3);
    let page_grant = PageGrant {
        base: BASE,
        size: SIZE,
        owner: OWNER,
        owner_cell: binding.0,
        owner_generation: binding.1,
        shared_to: None,
        domain: None,
    };
    assert!(page_grant_authorizes_dma(
        &page_grant,
        OWNER,
        binding,
        BASE + 0x1000,
        0x2000
    ));
    assert!(!page_grant_authorizes_dma(
        &page_grant,
        OWNER + 1,
        binding,
        BASE,
        0x1000
    ));
    assert!(!page_grant_authorizes_dma(
        &page_grant,
        OWNER,
        (binding.0, binding.1 + 1),
        BASE,
        0x1000
    ));
    assert!(!page_grant_authorizes_dma(
        &page_grant,
        OWNER,
        binding,
        BASE + SIZE - 0x1000,
        0x2000
    ));

    let reg_grant = RegGrant {
        base: BASE,
        size: SIZE,
        owner: OWNER,
        owner_cell: binding.0,
        owner_generation: binding.1,
        shared_to: None,
        domain: None,
    };
    assert!(reg_grant_authorizes_dma(
        &reg_grant, OWNER, binding, BASE, SIZE
    ));
    assert!(!reg_grant_authorizes_dma(
        &reg_grant,
        OWNER,
        binding,
        BASE - 0x1000,
        0x2000
    ));
}

#[test]
fn vfs_holder_release_before_reap_frees_without_orphaning_quarantine() {
    let _guard = crate::TEST_STATE_LOCK.lock();
    const BASE: usize = 0x4f00_0000;
    const HOLDER: usize = 30_601;
    const OWNER: usize = 30_602;
    const GENERATION: u64 = 41;

    let before = crate::memory::pin::quarantined_pages();
    assert_eq!(
        crate::memory::pin::pin_vfs_lease(BASE, 4096, OWNER, HOLDER, 1, GENERATION),
        Ok(())
    );
    assert!(crate::memory::pin::mark_vfs_lease_pending_revoke(
        HOLDER, OWNER, GENERATION
    ));
    assert!(crate::memory::pin::release_vfs_lease(HOLDER, OWNER, GENERATION).is_empty());

    assert!(!withhold_or_free(BASE, 1));
    assert_eq!(crate::memory::pin::quarantined_pages(), before);
    assert!(crate::memory::pin::holder_of(BASE, 4096).is_none());
}

// ---- NET-OD: owner-death watch grant and TryRecv delivery -----------------

const NET_TID: usize = 41_001;
const NET_CELL: u64 = 44;
const NET_GEN: u64 = 5;
const CLIENT_TID: usize = 41_002;
const CLIENT_CELL: u64 = 45;
const CLIENT_GEN: u64 = 3;
const OTHER_TID: usize = 41_003;
const OTHER_CELL: u64 = 46;
const OTHER_GEN: u64 = 2;
/// A provider that is not the watcher.
const FOREIGN_TID: usize = 41_009;

/// The watching cell (receive context already set to the client, as the
/// kernel does on delivery) and two live, published client principals.
fn owner_watch_scheduler() -> Scheduler {
    let mut sched = Scheduler::new();
    let mut watcher = alloc::boxed::Box::new(Task::new(
        NET_TID,
        CellId(NET_CELL),
        "net-watcher",
        alloc::vec::Vec::new(),
    ));
    watcher.cell_generation = NET_GEN;
    watcher.root_tid = NET_TID;
    watcher.caller.cell_id = CLIENT_CELL;
    watcher.caller.cell_generation = CLIENT_GEN;
    sched.tasks.insert(NET_TID, watcher);
    for (tid, cell, generation) in [
        (CLIENT_TID, CLIENT_CELL, CLIENT_GEN),
        (OTHER_TID, OTHER_CELL, OTHER_GEN),
    ] {
        let mut client = alloc::boxed::Box::new(Task::new(
            tid,
            CellId(cell),
            "net-client",
            alloc::vec::Vec::new(),
        ));
        client.cell_generation = generation;
        client.root_tid = tid;
        sched.tasks.insert(tid, client);
        assert!(
            sched.publish_live_cell_owner(abi::cell_owner::CellOwner::new(
                cell, generation, tid as u64
            ))
        );
    }
    sched
}

/// One `watch_live_cell_owner` call by the watcher cell with exactly
/// `registry` published (and `pause` applied) for the duration of the call
/// only: the global registry is shared with tests that do not take
/// `TEST_STATE_LOCK`.
/// The services these cases register with `OWNER_WATCH` (init's policy for
/// them; which services init flags is pinned in `ostd::service`).
const OWNER_WATCH_ROWS: [u16; 3] = [
    abi::syscall::service::NET,
    abi::syscall::service::COMPOSITOR,
    abi::syscall::service::XE_GPU,
];

fn owner_watch(
    sched: &mut Scheduler,
    registry: &[(u16, usize, u64, u64)],
    pause: Option<(u16, usize)>,
    cell: u64,
    generation: u64,
) -> Option<(abi::cell_owner::CellOwner, u64)> {
    for &(service, tid, cell_id, gen) in registry {
        let flags = if OWNER_WATCH_ROWS.contains(&service) {
            abi::syscall::service_flags::OWNER_WATCH
        } else {
            0
        };
        assert_eq!(
            crate::cell::service_registry::register_as(
                crate::cell::service_registry::Registrar::Namespace,
                service,
                tid,
                cell_id,
                gen,
                flags,
            ),
            Ok(())
        );
    }
    if let Some((service, tid)) = pause {
        assert!(crate::cell::service_registry::pause(service, tid));
    }
    let result = sched.watch_live_cell_owner(NET_TID, CellId(cell), generation);
    for &(_, tid, _, _) in registry {
        crate::cell::service_registry::clear_tid(tid);
    }
    result
}

fn assert_owner_watch(
    label: &str,
    want: bool,
    registry: &[(u16, usize, u64, u64)],
    pause: Option<(u16, usize)>,
    mutate: impl FnOnce(&mut Scheduler),
    cell: u64,
    generation: u64,
) {
    let mut sched = owner_watch_scheduler();
    mutate(&mut sched);
    let granted = owner_watch(&mut sched, registry, pause, cell, generation);
    assert_eq!(granted.is_some(), want, "{label}");
    // A refusal installs nothing.
    assert_eq!(sched.cell_owner_watch_count(), usize::from(want), "{label}");
}

/// Every service id the registry can bind that these cases register unflagged.
fn services_outside_owner_watch_table() -> alloc::vec::Vec<u16> {
    use abi::syscall::service as s;
    [
        s::VFS,
        s::NET,
        s::INPUT,
        s::CONFIG,
        s::COMPOSITOR,
        s::SILO,
        s::HOTSWAP_DEMO,
        s::NET_BROKER,
        s::BLOCK_DRIVER,
        s::NIC_DRIVER,
        s::SUPERVISOR,
        s::GPU_DRIVER,
        s::KMS,
        s::AI,
        s::OCEL_JS,
    ]
    .into_iter()
    .filter(|id| !OWNER_WATCH_ROWS.contains(id))
    .collect()
}

/// The grant for one table service: its Active registry-bound provider
/// cell serving exactly the principal it names. `service` is the only
/// registration in play, so the case proves that service's own entry.
fn assert_owner_watch_grant_for(service: u16) {
    let _guard = crate::TEST_STATE_LOCK.lock();
    let bound = [(service, NET_TID, NET_CELL, NET_GEN)];
    let nothing = |_: &mut Scheduler| {};

    // Granted: the Active binding, the attested serving context, a live
    // client generation. The watch names the client root.
    let mut sched = owner_watch_scheduler();
    let (owner, token) = owner_watch(&mut sched, &bound, None, CLIENT_CELL, CLIENT_GEN)
        .expect("registered watcher serving the client");
    assert_eq!(
        owner,
        abi::cell_owner::CellOwner::new(CLIENT_CELL, CLIENT_GEN, CLIENT_TID as u64)
    );
    assert!(token > 0);
    assert_eq!(sched.cell_owner_watch_count(), 1);
    // Only the watcher's own token is cancellable, and only by that token.
    sched.cancel_cell_owner_watch(CLIENT_TID, token);
    sched.cancel_cell_owner_watch(OTHER_TID, token);
    assert_eq!(sched.cell_owner_watch_count(), 1);
    sched.cancel_cell_owner_watch(NET_TID, token + 1);
    assert_eq!(sched.cell_owner_watch_count(), 1);
    sched.cancel_cell_owner_watch(NET_TID, token);
    assert_eq!(sched.cell_owner_watch_count(), 0);

    assert_owner_watch(
        "baseline",
        true,
        &bound,
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // Unregistered: refused.
    assert_owner_watch(
        "unregistered",
        false,
        &[],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // Hot-swap quiesce: a Paused binding is refused.
    assert_owner_watch(
        "paused",
        false,
        &bound,
        Some((service, NET_TID)),
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // Re-registered to another provider (tid, cell, generation).
    assert_owner_watch(
        "re-registered to another provider",
        false,
        &[(service, FOREIGN_TID, 47, 6)],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // Same CellId, other generation: the stale watcher generation cannot hold it.
    assert_owner_watch(
        "stale watcher generation",
        false,
        &[(service, NET_TID, NET_CELL, NET_GEN + 1)],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    assert_owner_watch(
        "other watcher cell",
        false,
        &[(service, NET_TID, NET_CELL + 1, NET_GEN)],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // Cell 0 is the kernel's allocation, never a provider binding.
    assert_owner_watch(
        "cell 0",
        false,
        &[(service, NET_TID, 0, NET_GEN)],
        None,
        |sched| {
            sched.tasks.get_mut(&NET_TID).expect("watcher").cell_id = CellId(0);
        },
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // The attested serving context must name the principal asked for.
    assert_owner_watch(
        "serving another cell",
        false,
        &bound,
        None,
        |sched| {
            sched
                .tasks
                .get_mut(&NET_TID)
                .expect("watcher")
                .caller
                .cell_id = OTHER_CELL;
        },
        CLIENT_CELL,
        CLIENT_GEN,
    );
    assert_owner_watch(
        "asking for another cell",
        false,
        &bound,
        None,
        nothing,
        OTHER_CELL,
        OTHER_GEN,
    );
    assert_owner_watch(
        "no serving context",
        false,
        &bound,
        None,
        |sched| {
            sched
                .tasks
                .get_mut(&NET_TID)
                .expect("watcher")
                .clear_current_caller_context();
        },
        CLIENT_CELL,
        CLIENT_GEN,
    );
    // Stale client generation: asked for, or recorded, but never live.
    assert_owner_watch(
        "stale client generation",
        false,
        &bound,
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN + 1,
    );
    assert_owner_watch(
        "serving a dead generation",
        false,
        &bound,
        None,
        |sched| {
            sched
                .tasks
                .get_mut(&NET_TID)
                .expect("watcher")
                .caller
                .cell_generation = CLIENT_GEN + 1;
        },
        CLIENT_CELL,
        CLIENT_GEN + 1,
    );
    // Dead client root.
    assert_owner_watch(
        "dead client root",
        false,
        &bound,
        None,
        |sched| {
            assert!(sched.tasks.remove(&CLIENT_TID).is_some());
        },
        CLIENT_CELL,
        CLIENT_GEN,
    );
}

/// The grant is the registry-bound live provider cell of a table service
/// serving exactly the principal it names; everything else (and the VFS
/// rule) is as before.
#[test]
fn owner_watch_grant_matrix() {
    use abi::syscall::service::{COMPOSITOR, NET};
    for &service in &OWNER_WATCH_ROWS {
        assert_owner_watch_grant_for(service);
    }

    let _guard = crate::TEST_STATE_LOCK.lock();
    let nothing = |_: &mut Scheduler| {};
    // Another service's provider with the same Cell identity is not a table
    // service, whichever table service the matrix is exercising.
    for service in services_outside_owner_watch_table() {
        assert_owner_watch(
            "other service provider",
            false,
            &[(service, NET_TID, NET_CELL, NET_GEN)],
            None,
            nothing,
            CLIENT_CELL,
            CLIENT_GEN,
        );
    }
    // A watcher bound to neither table service (both bound elsewhere) is
    // refused; a binding of one table service gives no right to the cell
    // that holds another service's binding.
    assert_owner_watch(
        "every table service bound to another provider",
        false,
        &[
            (NET, FOREIGN_TID, 47, 6),
            (COMPOSITOR, FOREIGN_TID + 1, 48, 7),
        ],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    assert_owner_watch(
        "NET bound to the watcher, COMPOSITOR bound elsewhere",
        true,
        &[
            (NET, NET_TID, NET_CELL, NET_GEN),
            (COMPOSITOR, FOREIGN_TID, 47, 6),
        ],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    assert_owner_watch(
        "COMPOSITOR bound to the watcher, NET bound elsewhere",
        true,
        &[
            (COMPOSITOR, NET_TID, NET_CELL, NET_GEN),
            (NET, FOREIGN_TID, 47, 6),
        ],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    assert_owner_watch(
        "both bound to the watcher, one paused",
        true,
        &[
            (NET, NET_TID, NET_CELL, NET_GEN),
            (COMPOSITOR, NET_TID, NET_CELL, NET_GEN),
        ],
        Some((NET, NET_TID)),
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );

    // The grant follows the row's flag, not the service id: a VFS row its
    // registrar flagged grants exactly like the rows above, and is held to the
    // same serving-context proof.
    assert_eq!(
        crate::cell::service_registry::register_as(
            crate::cell::service_registry::Registrar::Namespace,
            abi::syscall::service::VFS,
            NET_TID,
            NET_CELL,
            NET_GEN,
            abi::syscall::service_flags::ALL,
        ),
        Ok(())
    );
    assert_owner_watch(
        "flagged vfs row",
        true,
        &[],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
    assert_owner_watch(
        "flagged vfs row, other principal",
        false,
        &[],
        None,
        nothing,
        OTHER_CELL,
        OTHER_GEN,
    );
    crate::cell::service_registry::clear_tid(NET_TID);
    assert_owner_watch(
        "vfs row cleared",
        false,
        &[],
        None,
        nothing,
        CLIENT_CELL,
        CLIENT_GEN,
    );
}

/// `TryRecv` takes a queued owner death only for the wildcard mask, FIFO,
/// one exact record at a time, and never a `NotifyOnExit` death.
#[test]
fn try_recv_takes_owner_death_only_for_mask_zero() {
    use super::{peek_owner_death_for_try_recv as peek, remove_owner_death as remove};
    let mut task = Task::new(41_010, CellId(NET_CELL), "try-recv", alloc::vec::Vec::new());
    assert_eq!(peek(&task, 0), None);

    task.pending_owner_deaths.push((11, 101, 0xDEAD));
    task.pending_owner_deaths.push((12, 102, 7));
    task.pending_deaths.push((900, 5));

    // A masked (dependency) receive never sees an owner death.
    for mask in [1usize, 101, 102, 900, usize::MAX] {
        assert_eq!(peek(&task, mask), None, "mask {mask}");
    }
    assert_eq!(task.pending_owner_deaths.len(), 2);

    // Wildcard: the oldest record, and peeking removes nothing.
    assert_eq!(peek(&task, 0), Some((11, 101, 0xDEAD)));
    assert_eq!(peek(&task, 0), Some((11, 101, 0xDEAD)));
    assert_eq!(task.pending_owner_deaths.len(), 2);

    // Commit removes exactly the delivered record; an unknown token is a no-op.
    remove(&mut task, 99);
    assert_eq!(task.pending_owner_deaths.len(), 2);
    remove(&mut task, 11);
    assert_eq!(peek(&task, 0), Some((12, 102, 7)));
    remove(&mut task, 12);
    assert_eq!(peek(&task, 0), None);
    assert!(task.pending_owner_deaths.is_empty());

    // NotifyOnExit deaths are untouched and never offered by TryRecv.
    assert_eq!(task.pending_deaths, alloc::vec![(900, 5)]);
}

/// CP1 #4: a nonzero `WaitIrq` base is admitted only when it is the exact
/// base of an MMIO window the caller was granted; zero stays valid.
#[test]
fn wait_irq_base_requires_the_callers_exact_granted_window() {
    use super::wait_irq_base_permitted;
    const OWNER: usize = 0x7E1A;
    const BASE: usize = 0x7E1A_0000_0000;
    let _guard = crate::TEST_STATE_LOCK.lock();
    assert!(crate::resource_registry::request_mmio_unchecked(OWNER, BASE, 0x1000).is_ok());

    assert!(wait_irq_base_permitted(OWNER, 0));
    assert!(wait_irq_base_permitted(OWNER, BASE));
    // Another caller, an interior address and an ungranted base are refused.
    assert!(!wait_irq_base_permitted(OWNER + 1, BASE));
    assert!(!wait_irq_base_permitted(OWNER, BASE + 0x200));
    assert!(!wait_irq_base_permitted(OWNER, BASE + 0x1000));

    crate::resource_registry::release_mmio_for(OWNER);
    assert!(!wait_irq_base_permitted(OWNER, BASE));
    assert!(wait_irq_base_permitted(OWNER, 0));
}

/// CP1 #10: an `ShmAlloc` frame maps only for the incarnation that
/// allocated it; another Cell or another generation of the same Cell is
/// refused, as is a handle that was never issued.
#[test]
fn shm_frame_is_owned_by_exactly_the_allocating_incarnation() {
    use super::{shm_owned_by, shm_register, shm_take_for};
    use crate::cell::state_stash::Incarnation;
    const FRAME: usize = 0x5A0_0000;
    let owner = Incarnation {
        cell: 0x5A1,
        generation: 3,
    };
    shm_register(FRAME, owner, 4096, None, 0x3);

    assert!(shm_owned_by(FRAME, owner));
    assert!(!shm_owned_by(
        FRAME,
        Incarnation {
            cell: 0x5A2,
            generation: 3
        }
    ));
    assert!(!shm_owned_by(
        FRAME,
        Incarnation {
            cell: 0x5A1,
            generation: 4
        }
    ));
    assert!(!shm_owned_by(FRAME + 0x1000, owner));

    assert_eq!(shm_take_for(owner), (alloc::vec![FRAME], 4096));
    assert!(!shm_owned_by(FRAME, owner));
}

/// CP1 #10: root-retirement reclaim removes the dead incarnation's records
/// and nothing else — not its successor generation's, not a peer's.
#[test]
fn shm_reclaim_takes_only_the_dead_incarnations_records() {
    use super::{shm_owned_by, shm_register, shm_take_for};
    use crate::cell::state_stash::Incarnation;
    let dead = Incarnation {
        cell: 0x5B1,
        generation: 2,
    };
    let successor = Incarnation {
        cell: 0x5B1,
        generation: 3,
    };
    let peer = Incarnation {
        cell: 0x5B2,
        generation: 2,
    };
    shm_register(0x5B0_0000, dead, 4096, None, 0x3);
    shm_register(0x5B0_1000, successor, 4096, None, 0x3);
    shm_register(0x5B0_2000, dead, 4096, None, 0x3);
    shm_register(0x5B0_3000, peer, 4096, None, 0x3);

    assert_eq!(
        shm_take_for(dead),
        (alloc::vec![0x5B0_0000, 0x5B0_2000], 8192)
    );
    assert!(!shm_owned_by(0x5B0_0000, dead));
    assert!(!shm_owned_by(0x5B0_2000, dead));
    assert!(shm_owned_by(0x5B0_1000, successor));
    assert!(shm_owned_by(0x5B0_3000, peer));
    assert_eq!(shm_take_for(dead), (alloc::vec![], 0));

    assert_eq!(shm_take_for(successor), (alloc::vec![0x5B0_1000], 4096));
    assert_eq!(shm_take_for(peer), (alloc::vec![0x5B0_3000], 4096));
}

#[test]
fn shm_accessible_by_owner_and_authorized_recipient() {
    use super::{shm_accessible_by, shm_register};
    use crate::cell::state_stash::Incarnation;
    const FRAME: usize = 0x5C0_0000;
    let owner = Incarnation {
        cell: 10,
        generation: 1,
    };
    shm_register(FRAME, owner, 4096, Some(20), 0x3);

    assert!(shm_accessible_by(FRAME, 10, 1).is_some());
    assert!(shm_accessible_by(FRAME, 20, 1).is_some());
    assert!(shm_accessible_by(FRAME, 30, 1).is_none());
    assert!(shm_accessible_by(FRAME, 10, 2).is_none()); // Different generation
}

/// One Cell's posts may fill only `POST_QUOTA_PER_SENDER` slots of a target's
/// mailbox, so a client posting every frame cannot crowd out other clients;
/// another Cell and the kernel still get in.
#[test]
fn a_cell_posts_into_a_bounded_share_of_a_mailbox() {
    use crate::task::tcb::POST_QUOTA_PER_SENDER;
    const TARGET: usize = 2;
    const OTHER: usize = 3;
    with_scheduler_task(0, |poster| {
        {
            let mut guard = SCHEDULER.lock();
            let sched = guard.as_mut().expect("test scheduler");
            for (tid, cell) in [(TARGET, 8), (OTHER, 9)] {
                sched.tasks.insert(
                    tid,
                    alloc::boxed::Box::new(Task::new(
                        tid,
                        CellId(cell),
                        "post-quota",
                        alloc::vec::Vec::new(),
                    )),
                );
            }
        }
        for _ in 0..POST_QUOTA_PER_SENDER {
            assert_eq!(crate::task::ipc_post_nonblock(poster, TARGET, b"d"), Ok(()));
        }
        assert_eq!(
            crate::task::ipc_post_nonblock(poster, TARGET, b"d"),
            Err(())
        );
        assert_eq!(crate::task::ipc_post_nonblock(OTHER, TARGET, b"r"), Ok(()));
        assert_eq!(crate::task::ipc_post_nonblock(0, TARGET, b"k"), Ok(()));
    });
}

#[test]
fn recv_timeout_bits_select_a_timed_receive_that_keeps_attestation() {
    use abi::caller_identity::{recv_flags, RECV_ATTEST_CALLER};
    let recv = |a3: usize| map_syscall(ViSyscall::Recv as usize, 7, 0x4000, 4096, a3);
    // Untimed: the flag word every pre-existing caller passes.
    assert!(matches!(
        recv(0),
        Some(Syscall::Recv {
            attest_caller: false,
            ..
        })
    ));
    assert!(matches!(
        recv(RECV_ATTEST_CALLER),
        Some(Syscall::Recv {
            attest_caller: true,
            ..
        })
    ));
    // Bits 1-7 stay ignored.
    assert!(matches!(
        recv(0xFE),
        Some(Syscall::Recv {
            attest_caller: false,
            ..
        })
    ));
    let before = crate::task::system_ticks() as u64;
    match recv(recv_flags(true, 50)) {
        Some(Syscall::RecvTimeout {
            mask: 7,
            buf_len: 4096,
            deadline,
            attest_caller: true,
            timeout_is_error: true,
            ..
        }) => {
            assert!(deadline >= before + 50 && deadline <= crate::task::system_ticks() as u64 + 50)
        }
        other => panic!("timed attested Recv decoded as {other:?}"),
    }
    // `RecvTimeout` itself keeps its historical shape: no trailer, Ok(0) on expiry.
    assert!(matches!(
        map_syscall(ViSyscall::RecvTimeout as usize, 0, 0x4000, 4096, 50),
        Some(Syscall::RecvTimeout {
            attest_caller: false,
            timeout_is_error: false,
            ..
        })
    ));
}
