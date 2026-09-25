//! Boot self-tests for two trust-model fixes (2026-07-13):
//!   #7 — spawned threads inherit the parent cell's identity (CellId + CapSet +
//!        syscall_allowlist + PKU domain), closing the `CellId(0)` quota escape.
//!   #5 — `sys_cap_revoke` is honest about ambient authority: MMIO windows are
//!        torn down (unmapped + released, `.agents/260712-1901` P01-P04) and the
//!        victim is notified, while HYPERVISOR — which has no teardown path — is
//!        still refused with `NotSupported`.
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
/// Target of the privileged-cap revoke (P05): holds pcie_driver/platform/supervisor.
const DRIVER_TID: usize = 9004;
/// A synthetic BAR window, registered only under `test-hooks` so the global
/// BAR table keeps exactly what the ECAM scan put in it.
#[cfg(feature = "test-hooks")]
const DRIVER_BAR: usize = 0x5100_0000;
/// A synthetic requester ID; nothing else claims it during the boot window.
const DRIVER_BDF: u32 = 0x02_00_00;
/// Separate cell for the thread-cap section, so its fillers cannot be mistaken
/// for (or collide with) the identity-inheritance section's parent cell.
const DOS_CELL_ID: u64 = (crate::memory::cell_quota::MAX_CELLS - 4) as u64;
const DOS_FILLER_BASE: usize = 9100;

// Distinctive sentinels so an inherited value is unambiguously the parent's.
const TEST_CELL_ID: u64 = (crate::memory::cell_quota::MAX_CELLS - 2) as u64;
// A restricted allowlist that still PERMITS the Spawn syscall (so the global
// allowlist gate in handle_syscall lets the call through) yet differs from the
// Task::new default of u64::MAX — bit 63 is unassigned (real syscalls use bits
// ≤54), so clearing it is behaviourally inert but proves the field was inherited
// rather than left at the permit-all default.
const TEST_ALLOWLIST: u64 = !(1u64 << 63);
const TEST_PKU_KEY: u8 = 2;
const TEST_PKU_VALUE: u32 = 0xABCD_1234;

/// Build a bare task with the given tid/cell and no caps. Only the fields a
/// test reads/writes need to be meaningful; the task is never scheduled.
///
/// Shared with the sibling boot self-tests (`grant_reclaim_selftest`,
/// `mmio_revoke_selftest`) so each keeps one fixture implementation.
pub(crate) fn mk_task(tid: usize, cell: u64) -> alloc::boxed::Box<Task> {
    let mut task = alloc::boxed::Box::new(Task::new(
        tid,
        CellId(cell),
        "selftest",
        alloc::vec::Vec::new(),
    ));
    task.cell_generation = 1;
    task.root_tid = tid;
    task
}

/// Insert a task into the scheduler map (no ready-queue push — never scheduled).
pub(crate) fn insert(task: alloc::boxed::Box<Task>) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
            let owner = api::cell_owner::CellOwner::new(
                task.cell_id.0,
                task.cell_generation,
                task.root_tid as u64,
            );
            sched.publish_live_cell_owner(owner);
        }
        sched.tasks.insert(task.id, task);
    }
}

/// Remove a tid from the scheduler map AND every hart's ready queue.
pub(crate) fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
                let owner = api::cell_owner::CellOwner::new(
                    task.cell_id.0,
                    task.cell_generation,
                    task.root_tid as u64,
                );
                sched.clear_live_cell_owner_for_test(owner);
            }
        }
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
        let spawned = handle_syscall(
            PARENT_TID,
            Syscall::Spawn {
                entry: 0x1000,
                arg: 0,
            },
        );

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
                            log::error!(
                                "[selftest] THREAD-INHERIT: FAIL \
                                cell={} caps={} allow={} pku={}",
                                cell_ok,
                                caps_ok,
                                allow_ok,
                                pku_ok
                            );
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
                log::error!(
                    "[selftest] THREAD-INHERIT: FAIL — spawn returned {:?}",
                    spawned
                );
            }
        }
        remove(PARENT_TID);
    }

    // ── #5: honest revoke (ambient bits torn down, hypervisor still refused) ────
    {
        // Caller holds SpawnCap (Gate 1). The target holds an MMIO device bit, an
        // owned grant and a hypervisor cap. An MMIO window is ambient authority —
        // a mapped window is reachable with no syscall on the access path — so
        // revoking it must tear the window down, not just clear the field
        // (`.agents/260712-1901` P01-P04 replaced P00's blanket refusal).
        // HYPERVISOR has no teardown path and must still be refused.
        let mut caller = mk_task(PARENT_TID, TEST_CELL_ID);
        caller.spawn_cap = Some(super::cap::SpawnCap::new());
        insert(caller);

        let mut target = mk_task(TARGET_TID, 0x1111);
        target.hypervisor_cap = Some(super::cap::HypervisorCap::new());
        target.mmio_devices = crate::resource_registry::DEV_GPIO;
        insert(target);

        // (a) HYPERVISOR bit → NotSupported, cap untouched.
        let r_hyp = handle_syscall(
            PARENT_TID,
            Syscall::CapRevoke {
                target_tid: TARGET_TID,
                cap_mask: CM::HYPERVISOR,
            },
        );
        let hyp_refused = matches!(r_hyp, Err(SyscallError::NotSupported))
            && if let Some(sched) = super::SCHEDULER.lock().as_ref() {
                sched
                    .tasks
                    .get(&TARGET_TID)
                    .map(|t| t.hypervisor_cap.is_some())
                    .unwrap_or(false)
            } else {
                false
            };
        if !hyp_refused {
            ok = false;
            log::error!("[selftest] REVOKE-HYPERVISOR: FAIL r={:?}", r_hyp);
        }

        // The target owns a grant, and — where the board has an allowlisted
        // window — an MMIO region of the class it is about to lose.
        let owned_grant = match handle_syscall(TARGET_TID, Syscall::GrantAlloc { size: 4096 }) {
            Ok(base) if base != 0 => Some(base),
            _ => None,
        };
        let window = crate::resource_registry::allowed_windows()
            .first()
            .copied()
            .and_then(|(base, len, class)| {
                crate::resource_registry::request_mmio(CellId(0x1111), base, len, class)
                    .is_ok()
                    .then_some((base, len))
            });

        // (b) MMIO bits → Ok, window released, owned grant reclaimed, the MMIO
        //     field cleared (the hypervisor cap untouched), and the victim
        //     notified with `[0xAC, 0xF2, mask_le4]`.
        let r_mmio = handle_syscall(
            PARENT_TID,
            Syscall::CapRevoke {
                target_tid: TARGET_TID,
                cap_mask: CM::MMIO_MASK,
            },
        );
        let target_state = super::SCHEDULER.lock().as_ref().and_then(|sched| {
            sched.tasks.get(&TARGET_TID).map(|t| {
                let notified = t
                    .pending_msgs
                    .as_slice()
                    .iter()
                    .filter_map(|msg| msg.wire.as_ref())
                    .any(|wire| {
                        let payload = wire.as_slice();
                        payload.len() == 6
                            && payload[0] == 0xAC
                            && payload[1] == 0xF2
                            && u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]])
                                == CM::MMIO_MASK
                    });
                (t.mmio_devices, t.hypervisor_cap.is_some(), notified)
            })
        });
        let window_released = window
            .map(|(base, _)| crate::resource_registry::lookup_mmio_owner(base).is_none())
            .unwrap_or(true);
        let grant_reclaimed = owned_grant
            .map(|base| {
                matches!(
                    handle_syscall(TARGET_TID, Syscall::GrantFree { grant_id: base }),
                    Err(SyscallError::PermissionDenied)
                )
            })
            .unwrap_or(true);

        let mmio_ok = r_mmio.is_ok()
            && target_state.is_some_and(|(devices, hyp, notified)| devices == 0 && hyp && notified)
            && window_released
            && grant_reclaimed;
        if !mmio_ok {
            ok = false;
            log::error!(
                "[selftest] REVOKE-MMIO: FAIL r={:?} state={:?} window_released={} \
                 grant_reclaimed={}",
                r_mmio,
                target_state,
                window_released,
                grant_reclaimed
            );
        }

        // (c) Positive control: SPAWN is a lazy (syscall-gated) bit — revoke of a
        // non-system target must still SUCCEED and clear the field.
        let mut ctrl = mk_task(CTRL_TID, 0x2222);
        ctrl.spawn_cap = Some(super::cap::SpawnCap::new());
        insert(ctrl);
        let r_spawn = handle_syscall(
            PARENT_TID,
            Syscall::CapRevoke {
                target_tid: CTRL_TID,
                cap_mask: CM::SPAWN,
            },
        );
        let cleared = if let Some(sched) = super::SCHEDULER.lock().as_ref() {
            sched
                .tasks
                .get(&CTRL_TID)
                .map(|t| t.spawn_cap.is_none())
                .unwrap_or(false)
        } else {
            false
        };
        if !(r_spawn.is_ok() && cleared) {
            ok = false;
            log::error!(
                "[selftest] REVOKE-ALLOW: FAIL r={:?} cleared={}",
                r_spawn,
                cleared
            );
        }

        // (d) Privileged caps are revocable end to end (`.agents/260712-1901`
        //     P05): `pcie_driver` drains the DMA authority and releases the BDF,
        //     `platform` releases the ECAM window, `supervisor` is a field clear.
        let mut driver = mk_task(DRIVER_TID, 0x3333);
        driver.pcie_driver_cap = Some(super::cap::PcieDriverCap::new());
        driver.platform_cap = Some(super::cap::PlatformCap::new());
        driver.supervisor_cap = Some(super::cap::SupervisorCap::new());
        insert(driver);

        let bdf_claimed = crate::resource_registry::claim_bdf_owner(DRIVER_BDF, DRIVER_TID);
        // The BAR table is boot-discovered data with no removal path, so the
        // synthetic window is registered only where the test hook exists.
        #[cfg(feature = "test-hooks")]
        let bar_claimed = crate::resource_registry::test_register_region(
            CellId(0x3333),
            DRIVER_BAR,
            0x1000,
            crate::resource_registry::DEV_PCIE as crate::resource_registry::MmioClass,
        );

        let r_priv = handle_syscall(
            PARENT_TID,
            Syscall::CapRevoke {
                target_tid: DRIVER_TID,
                cap_mask: CM::PCIE_DRIVER | CM::PLATFORM | CM::SUPERVISOR,
            },
        );
        let priv_state = super::SCHEDULER.lock().as_ref().and_then(|sched| {
            sched.tasks.get(&DRIVER_TID).map(|t| {
                let notified = t
                    .pending_msgs
                    .as_slice()
                    .iter()
                    .filter_map(|msg| msg.wire.as_ref())
                    .any(|wire| {
                        let payload = wire.as_slice();
                        payload.len() == 6
                            && payload[0] == 0xAC
                            && payload[1] == 0xF2
                            && u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]])
                                == CM::PCIE_DRIVER | CM::PLATFORM | CM::SUPERVISOR
                    });
                (
                    t.pcie_driver_cap.is_none(),
                    t.platform_cap.is_none(),
                    t.supervisor_cap.is_none(),
                    notified,
                )
            })
        });
        let bdf_released =
            bdf_claimed && crate::resource_registry::owner_of_bdf(DRIVER_BDF).is_none();
        #[cfg(feature = "test-hooks")]
        let bar_released =
            bar_claimed && crate::resource_registry::lookup_mmio_owner(DRIVER_BAR).is_none();
        #[cfg(not(feature = "test-hooks"))]
        let bar_released = true;

        let priv_ok = r_priv.is_ok()
            && priv_state.is_some_and(|(pcie, platform, supervisor, notified)| {
                pcie && platform && supervisor && notified
            })
            && bdf_released
            && bar_released;
        if !priv_ok {
            ok = false;
            log::error!(
                "[selftest] REVOKE-PRIVILEGED: FAIL r={:?} state={:?} bdf_released={} \
                 bar_released={}",
                r_priv,
                priv_state,
                bdf_released,
                bar_released
            );
        }

        remove(PARENT_TID);
        remove(TARGET_TID);
        remove(CTRL_TID);
        remove(DRIVER_TID);
    }

    // ── thread-spawn DoS bound ─────────────────────────────────────────────────
    // `Syscall::Spawn` is gated by the syscall allowlist but NOT by SpawnCap, and
    // each thread costs a contiguous run of STACK_PAGES+1 frames. Before the cap an
    // unprivileged cell could loop here until the allocator fragmented, at which
    // point the scheduler's `.expect("OOM Stack")` panicked the kernel — never-die
    // broken from userspace. The refusal must be an error the caller survives.
    //
    // The cell is filled with STACKLESS synthetic tasks: the cap counts live tasks
    // sharing a CellId, so proving the bound costs 31 Task structs instead of 31
    // real 65-frame stack allocations.
    {
        let mut parent = mk_task(PARENT_TID, DOS_CELL_ID);
        parent.spawn_cap = Some(super::cap::SpawnCap::new());
        insert(parent);

        let filler: alloc::vec::Vec<usize> = (0..super::scheduler::MAX_THREADS_PER_CELL - 1)
            .map(|i| DOS_FILLER_BASE + i)
            .collect();
        for &tid in &filler {
            insert(mk_task(tid, DOS_CELL_ID));
        }

        // Parent + fillers occupy the whole budget, so the next thread must be
        // refused — with TryAgain (a recoverable refusal), not a panic, and not a
        // silent success.
        let refused = handle_syscall(
            PARENT_TID,
            Syscall::Spawn {
                entry: 0x1000,
                arg: 0,
            },
        );
        if !matches!(refused, Err(SyscallError::TryAgain)) {
            ok = false;
            log::error!(
                "[selftest] THREAD-CAP-DOS: FAIL — spawn at cap returned {:?}, expected TryAgain",
                refused
            );
            // A spawn that unexpectedly succeeded left a task behind; drop it so the
            // scheduler is still returned exactly as it was found.
            if let Ok(stray) = refused {
                remove(stray);
            }
        }

        for tid in filler {
            remove(tid);
        }
        remove(PARENT_TID);
    }

    // Restore the spawn counter so the test leaves no trace on tid assignment.
    if let (Some(sched), Some(n)) = (super::SCHEDULER.lock().as_mut(), saved_next_tid) {
        sched.next_task_id = n;
    }

    if ok {
        log::info!("[selftest] THREAD-CAP: PASS (thread-inherit + honest-revoke + spawn bound)");
    } else {
        log::error!("[selftest] THREAD-CAP: FAIL");
    }
    ok
}
