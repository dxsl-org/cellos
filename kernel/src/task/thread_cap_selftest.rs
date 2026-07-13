//! Boot self-tests for two trust-model fixes (2026-07-13):
//!   #7 — spawned threads inherit the parent cell's identity (CellId + CapSet +
//!        syscall_allowlist + PKU domain), closing the `CellId(0)` quota escape.
//!   #5 — `sys_cap_revoke` refuses ambient-authority bits (HYPERVISOR, MMIO) with
//!        `NotSupported` instead of silently clearing a field it cannot truly revoke.
//!
//! Runs in the single-hart window AFTER `task::init()` but BEFORE
//! `smp::start_secondaries()` (see `main.rs`), so a synthetic thread inserted into
//! the boot hart's ready queue can never be picked up by another hart before this
//! test removes it. All synthetic tasks are created here and torn down before the
//! function returns — the scheduler is left exactly as it was found.
//!
//! Invariant proven, not assumed: after the fix the spawned thread's `cell_id`
//! equals the parent's (not `CellId(0)`), which is what routes its allocations to
//! the parent's quota (the charge path keys off `cell_id`; the cell-spawn path in
//! `loader.rs` already relies on this identity).

use super::cap::CapSet;
use super::syscall::{handle_syscall, Syscall, SyscallError};
use super::tcb::Task;
use api::syscall::cap_mask as CM;
use types::CellId;

// Synthetic tids outside any range the boot sequence has assigned yet. Removed
// before return, so they never collide with real cells.
const PARENT_TID: usize = 9001;
const TARGET_TID: usize = 9002;
const CTRL_TID: usize = 9003;

// Distinctive sentinels so an inherited value is unambiguously the parent's.
const TEST_CELL_ID: u64 = 0x00C0_FFEE;
// A restricted allowlist that still PERMITS the Spawn syscall (so the global
// allowlist gate in handle_syscall lets the call through) yet differs from the
// Task::new default of u64::MAX — bit 63 is unassigned (real syscalls use bits
// ≤54), so clearing it is behaviourally inert but proves the field was inherited
// rather than left at the permit-all default.
const TEST_ALLOWLIST: u64 = !(1u64 << 63);
const TEST_PKU_KEY: u8 = 2;
const TEST_PKU_VALUE: u32 = 0xABCD_1234;

/// Build a bare task with the given tid/cell and no caps. Only the fields this
/// test reads/writes need to be meaningful; the task is never scheduled.
fn mk_task(tid: usize, cell: u64) -> alloc::boxed::Box<Task> {
    alloc::boxed::Box::new(Task::new(tid, CellId(cell), "selftest", alloc::vec::Vec::new()))
}

/// Insert a task into the scheduler map (no ready-queue push — never scheduled).
fn insert(task: alloc::boxed::Box<Task>) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.tasks.insert(task.id, task);
    }
}

/// Remove a tid from the scheduler map AND every hart's ready queue.
fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        sched.tasks.remove(&tid);
    }
    super::hart_local::ready::remove_from_all(tid);
}

/// Returns true iff both fixes behave as specified. Logs a decisive serial line.
///
/// Transparent to the boot sequence: the real thread spawn it performs advances
/// `next_task_id`, so the counter is snapshotted on entry and restored on exit —
/// the first real cell (Platform) still gets the tid it would have without this
/// test, keeping CellId assignment stable whether or not the test is compiled in.
pub fn self_test() -> bool {
    let mut ok = true;

    let saved_next_tid = super::SCHEDULER.lock().as_ref().map(|s| s.next_task_id);

    // ── #7: thread identity inheritance ────────────────────────────────────────
    // Parent cell: a distinctive CellId, a restricted allowlist, a non-zero PKU
    // domain, and one transferable cap (network) + SpawnCap (needed to spawn).
    {
        let mut parent = mk_task(PARENT_TID, TEST_CELL_ID);
        parent.network_cap = Some(super::cap::NetworkCap::new());
        parent.spawn_cap = Some(super::cap::SpawnCap::new());
        parent.syscall_allowlist = TEST_ALLOWLIST;
        parent.pku_key = TEST_PKU_KEY;
        parent.pku_value = TEST_PKU_VALUE;
        let parent_caps = CapSet::of_task(&parent);
        insert(parent);

        // Invoke the REAL Spawn path as if the parent called it. entry/arg are
        // placeholders; the thread is removed before it can run.
        let spawned = handle_syscall(PARENT_TID, Syscall::Spawn { entry: 0x1000, arg: 0 });

        match spawned {
            Ok(thread_tid) if thread_tid != 0 => {
                if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                    if let Some(t) = sched.tasks.get(&thread_tid) {
                        let cell_ok = t.cell_id.0 == TEST_CELL_ID; // NOT CellId(0)
                        let caps_ok = CapSet::of_task(t) == parent_caps;
                        let allow_ok = t.syscall_allowlist == TEST_ALLOWLIST; // NOT u64::MAX
                        let pku_ok = t.pku_key == TEST_PKU_KEY && t.pku_value == TEST_PKU_VALUE;
                        if !(cell_ok && caps_ok && allow_ok && pku_ok) {
                            ok = false;
                            log::error!("[selftest] THREAD-INHERIT: FAIL \
                                cell={} caps={} allow={} pku={}",
                                cell_ok, caps_ok, allow_ok, pku_ok);
                        }
                    } else {
                        ok = false;
                        log::error!("[selftest] THREAD-INHERIT: FAIL — thread tid absent");
                    }
                }
                remove(thread_tid);
            }
            _ => {
                ok = false;
                log::error!("[selftest] THREAD-INHERIT: FAIL — spawn returned {:?}", spawned);
            }
        }
        remove(PARENT_TID);
    }

    // ── #5: honest revoke (ambient bits refused, lazy bits still work) ──────────
    {
        // Caller holds SpawnCap (Gate 1). Target holds an ambient cap (hypervisor)
        // and an MMIO device bit — neither is truly revocable yet, so revoke must
        // REFUSE with NotSupported and leave the fields intact.
        let mut caller = mk_task(PARENT_TID, TEST_CELL_ID);
        caller.spawn_cap = Some(super::cap::SpawnCap::new());
        insert(caller);

        let mut target = mk_task(TARGET_TID, 0x1111);
        target.hypervisor_cap = Some(super::cap::HypervisorCap::new());
        target.mmio_devices = crate::resource_registry::DEV_GPIO;
        insert(target);

        // (a) HYPERVISOR bit → NotSupported, cap untouched.
        let r_hyp = handle_syscall(PARENT_TID,
            Syscall::CapRevoke { target_tid: TARGET_TID, cap_mask: CM::HYPERVISOR });
        // (b) MMIO bits → NotSupported, device mask untouched.
        let r_mmio = handle_syscall(PARENT_TID,
            Syscall::CapRevoke { target_tid: TARGET_TID, cap_mask: CM::MMIO_MASK });

        let refused = matches!(r_hyp, Err(SyscallError::NotSupported))
            && matches!(r_mmio, Err(SyscallError::NotSupported));
        let intact = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
            sched.tasks.get(&TARGET_TID).map(|t|
                t.hypervisor_cap.is_some() && t.mmio_devices == crate::resource_registry::DEV_GPIO
            ).unwrap_or(false)
        } else { false };
        if !(refused && intact) {
            ok = false;
            log::error!("[selftest] REVOKE-REFUSE: FAIL refused={} intact={} (hyp={:?} mmio={:?})",
                refused, intact, r_hyp, r_mmio);
        }

        // (c) Positive control: SPAWN is a lazy (syscall-gated) bit — revoke of a
        // non-system target must still SUCCEED and clear the field.
        let mut ctrl = mk_task(CTRL_TID, 0x2222);
        ctrl.spawn_cap = Some(super::cap::SpawnCap::new());
        insert(ctrl);
        let r_spawn = handle_syscall(PARENT_TID,
            Syscall::CapRevoke { target_tid: CTRL_TID, cap_mask: CM::SPAWN });
        let cleared = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
            sched.tasks.get(&CTRL_TID).map(|t| t.spawn_cap.is_none()).unwrap_or(false)
        } else { false };
        if !(r_spawn.is_ok() && cleared) {
            ok = false;
            log::error!("[selftest] REVOKE-ALLOW: FAIL r={:?} cleared={}", r_spawn, cleared);
        }

        remove(PARENT_TID);
        remove(TARGET_TID);
        remove(CTRL_TID);
    }

    // Restore the spawn counter so the test leaves no trace on tid assignment.
    if let (Some(sched), Some(n)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = n;
    }

    if ok {
        log::info!("[selftest] THREAD-CAP: PASS (thread-inherit + honest-revoke)");
    } else {
        log::error!("[selftest] THREAD-CAP: FAIL");
    }
    ok
}
