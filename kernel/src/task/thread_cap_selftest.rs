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
use abi::app_envelope;
use abi::syscall::cap_mask as CM;
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
/// A synthetic requester ID: 00:1f.7, a well-formed BDF that no supported board
/// populates, so retirement quiescence finds no function and touches nothing.
/// Where the kernel has no config-space window (AArch64) the function cannot be
/// shown quiescent at all, and the revoke must refuse instead.
/// (The previous `0x02_00_00` overflowed the 16-bit encoding and aliased the
/// real host bridge at 00:00.0.)
const DRIVER_BDF: u32 = 0x00_FF;
/// A later owner of `DRIVER_BDF`, which must find the revoked driver's NIC
/// route vacant.
#[cfg(feature = "test-hooks")]
const REPLACEMENT_TID: usize = 9005;
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
            let owner = abi::cell_owner::CellOwner::new(
                task.cell_id.0,
                task.cell_generation,
                task.root_tid as u64,
            );
            sched.publish_live_cell_owner(owner);
        }
        #[cfg(feature = "p01-trace")]
        crate::memory::heap::p01_owner::task_backing(&task);
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            None,
            crate::memory::heap::p01_owner::SCHEDULER_SHARED,
        );
        sched.tasks.insert(task.id, task);
    }
}

/// Remove a tid from the scheduler map AND every hart's ready queue.
pub(crate) fn remove(tid: usize) {
    if let Some(sched) = super::SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.remove(&tid) {
            if (task.cell_id.0 as usize) < crate::memory::cell_quota::MAX_CELLS {
                let owner = abi::cell_owner::CellOwner::new(
                    task.cell_id.0,
                    task.cell_generation,
                    task.root_tid as u64,
                );
                sched.clear_live_cell_owner_for_test(owner);
            }
        }
    }
    crate::sched::hart_local::ready::remove_from_all(tid);
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
    // domain, and one transferable cap (supervisor) + SpawnCap (needed to spawn).
    {
        let mut parent = mk_task(PARENT_TID, TEST_CELL_ID);
        // Two of the three S2-T/S2-W rights (memory withheld), so equality below
        // proves each one is the Cell's rather than defaulted.
        // No `network`/`block_io`: those mark a system Cell, which `CapRevoke`
        // refuses, and the Cell-wide revoke below must be admissible.
        let parent_caps = CapSet {
            supervisor: true,
            spawn: true,
            observe_tasks: true,
            vfs_mutate: true,
            ..CapSet::EMPTY
        };
        parent_caps.apply_to(&mut parent);
        parent.syscall_allowlist = TEST_ALLOWLIST;
        parent.pku_key = TEST_PKU_KEY;
        parent.pku_value = TEST_PKU_VALUE;
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
                                                                   // The thread is gated on its Cell's record, not a copy.
                        let caps_ok = sched.cell_rights(thread_tid) == parent_caps;
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
                // H-1: a revoke aimed at the thread removes the right from the
                // whole Cell — the root included — and no thread keeps it.
                let mut ctrl = mk_task(CTRL_TID, 0x2222);
                CapSet {
                    spawn: true,
                    ..CapSet::EMPTY
                }
                .apply_to(&mut ctrl);
                insert(ctrl);
                let revoked = handle_syscall(
                    CTRL_TID,
                    Syscall::CapRevoke {
                        target_tid: thread_tid,
                        cap_mask: CM::SPAWN,
                    },
                );
                let cell_wide = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
                    !sched.cell_rights(PARENT_TID).spawn && !sched.cell_rights(thread_tid).spawn
                });
                if !(revoked.is_ok() && cell_wide) {
                    ok = false;
                    log::error!(
                        "[selftest] REVOKE-CELL-WIDE: FAIL r={:?} cell_wide={}",
                        revoked,
                        cell_wide
                    );
                }
                remove(CTRL_TID);
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
        CapSet {
            spawn: true,
            ..CapSet::EMPTY
        }
        .apply_to(&mut caller);
        insert(caller);

        let mut target = mk_task(TARGET_TID, 0x1111);
        CapSet {
            hypervisor: true,
            mmio_devices: crate::resource_registry::DEV_GPIO,
            ..CapSet::EMPTY
        }
        .apply_to(&mut target);
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
            && super::SCHEDULER
                .lock()
                .as_ref()
                .is_some_and(|sched| sched.cell_rights(TARGET_TID).hypervisor);
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
                crate::resource_registry::request_mmio(TARGET_TID, base, len, class)
                    .is_ok()
                    .then_some((base, len))
            });

        // (b) MMIO bits → Ok, window released, owned grant reclaimed, the MMIO
        //     field cleared (the hypervisor cap untouched), and the victim
        //     notified with the `KIND_CAP_REVOKED` envelope.
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
                            && payload[0] == app_envelope::MAGIC
                            && payload[1] == app_envelope::KIND_CAP_REVOKED
                            && u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]])
                                == CM::MMIO_MASK
                    });
                let rights = sched.cell_rights(TARGET_TID);
                (rights.mmio_devices, rights.hypervisor, notified)
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

        // The window above came from the *board's* allowlist, so revoking it
        // changed its mapping for the whole system. A self-test must not leave
        // the machine in a state the boot did not produce, so the window goes
        // back to its boot posture whatever the assertions said: USER where the
        // board boot-maps it for Cells (RISC-V, QEMU virt — `/bin/periph-demo`
        // lost the PL011 window at boot before this existed), kernel-only on
        // the Pi boards, where only a live `RequestMmio` grant sets USER.
        if let Some((base, len)) = window {
            #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
            match crate::memory::paging::restore_mmio_boot_posture(base, len) {
                Ok(pages) => log::info!(
                    "[selftest] REVOKE-MMIO: window {:#x} restored to boot posture ({} page(s))",
                    base,
                    pages
                ),
                Err(e) => log::error!(
                    "[selftest] REVOKE-MMIO: window {:#x} could not be restored: {:?}",
                    base,
                    e
                ),
            }
            #[cfg(target_arch = "x86_64")]
            crate::memory::paging::map_mmio_user_x86(base, len);
        }

        // (c) Positive control: SPAWN is a lazy (syscall-gated) bit — revoke of a
        // non-system target must still SUCCEED and clear the field.
        let mut ctrl = mk_task(CTRL_TID, 0x2222);
        CapSet {
            spawn: true,
            ..CapSet::EMPTY
        }
        .apply_to(&mut ctrl);
        insert(ctrl);
        let r_spawn = handle_syscall(
            PARENT_TID,
            Syscall::CapRevoke {
                target_tid: CTRL_TID,
                cap_mask: CM::SPAWN,
            },
        );
        let cleared = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
            sched.tasks.contains_key(&CTRL_TID) && !sched.cell_rights(CTRL_TID).spawn
        });
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
        CapSet {
            pcie_driver: true,
            supervisor: true,
            ..CapSet::EMPTY
        }
        .apply_to(&mut driver);
        // `apply_to` never grants the Platform singleton; the fixture sets the
        // record bit directly (no reservation is taken).
        driver.rights.platform = true;
        insert(driver);

        let bdf_claimed = crate::dev::ownership::claim_pci(DRIVER_BDF, DRIVER_TID);
        #[cfg(feature = "test-hooks")]
        let nic_route = bind_driver_nic_route(bdf_claimed);
        // The BAR table is boot-discovered data with no removal path, so the
        // synthetic window is registered only where the test hook exists.
        #[cfg(feature = "test-hooks")]
        let bar_claimed = crate::resource_registry::test_register_region(
            DRIVER_TID,
            DRIVER_BAR,
            0x1000,
            crate::resource_registry::DEV_PCIE as crate::resource_registry::MmioClass,
        );

        // Without config space the owned function cannot be shown quiescent, so the
        // revoke must fail closed: error, BDF still owned, every capability intact.
        // The claim is then dropped so the rest of the teardown is still proven.
        if bdf_claimed && !crate::dev::pcie_ecam::config_space_admitted() {
            let refused = handle_syscall(
                PARENT_TID,
                Syscall::CapRevoke {
                    target_tid: DRIVER_TID,
                    cap_mask: CM::PCIE_DRIVER | CM::PLATFORM | CM::SUPERVISOR,
                },
            );
            let caps_intact = super::SCHEDULER.lock().as_ref().is_some_and(|sched| {
                let rights = sched.cell_rights(DRIVER_TID);
                rights.pcie_driver && rights.platform && rights.supervisor
            });
            let bdf_retained = crate::dev::ownership::pci_owner(DRIVER_BDF) == Some(DRIVER_TID);
            // A refused revoke freed nothing, so the route keeps its provider.
            #[cfg(feature = "test-hooks")]
            let route_kept = nic_route.is_some()
                && crate::cell::service_registry::lookup(abi::syscall::service::NIC_DRIVER)
                    == Some(DRIVER_TID);
            #[cfg(not(feature = "test-hooks"))]
            let route_kept = true;
            let refused_closed = matches!(refused, Err(SyscallError::NotSupported));
            if !(refused_closed && caps_intact && bdf_retained && route_kept) {
                ok = false;
                log::error!(
                    "[selftest] REVOKE-PRIVILEGED: no-config-space revoke did not fail closed \
                     r={:?} caps_intact={} bdf_retained={} route_kept={}",
                    refused,
                    caps_intact,
                    bdf_retained,
                    route_kept
                );
            }
            crate::dev::ownership::release_for(DRIVER_TID, crate::dev::ownership::Scope::Retired);
        }

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
                            && payload[0] == app_envelope::MAGIC
                            && payload[1] == app_envelope::KIND_CAP_REVOKED
                            && u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]])
                                == CM::PCIE_DRIVER | CM::PLATFORM | CM::SUPERVISOR
                    });
                let rights = sched.cell_rights(DRIVER_TID);
                (
                    !rights.pcie_driver,
                    !rights.platform,
                    !rights.supervisor,
                    notified,
                )
            })
        });
        let bdf_released = bdf_claimed && crate::dev::ownership::pci_owner(DRIVER_BDF).is_none();
        #[cfg(feature = "test-hooks")]
        let bar_released =
            bar_claimed && crate::resource_registry::lookup_mmio_owner(DRIVER_BAR).is_none();
        #[cfg(not(feature = "test-hooks"))]
        let bar_released = true;
        #[cfg(feature = "test-hooks")]
        let route_vacated = nic_route.is_some_and(driver_nic_route_vacated);
        #[cfg(not(feature = "test-hooks"))]
        let route_vacated = true;

        let priv_ok = r_priv.is_ok()
            && priv_state.is_some_and(|(pcie, platform, supervisor, notified)| {
                pcie && platform && supervisor && notified
            })
            && bdf_released
            && bar_released
            && route_vacated;
        if !priv_ok {
            ok = false;
            log::error!(
                "[selftest] REVOKE-PRIVILEGED: FAIL r={:?} state={:?} bdf_released={} \
                 bar_released={} route_vacated={}",
                r_priv,
                priv_state,
                bdf_released,
                bar_released,
                route_vacated
            );
        }
        // Leaves no route provider or `service::NIC_DRIVER` row behind, even
        // when an assertion above failed.
        #[cfg(feature = "test-hooks")]
        crate::dev::driver_cell::deregister_all_for(DRIVER_TID);

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
        CapSet {
            spawn: true,
            ..CapSet::EMPTY
        }
        .apply_to(&mut parent);
        insert(parent);

        let filler: alloc::vec::Vec<usize> = (0..crate::sched::scheduler::MAX_THREADS_PER_CELL - 1)
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

#[cfg(feature = "test-hooks")]
fn driver_nic_key() -> Option<abi::block_route::DeviceKey> {
    abi::block_route::DeviceKey::new(abi::block_route::BLOCK_DEVICE_PCI, u64::from(DRIVER_BDF))
}

/// Bind the driver's NIC route as `RegisterNicDriver` (417) does, before any
/// real NIC registers, so it is the primary and publishes `service::NIC_DRIVER`.
/// `Some(route index)` only then: otherwise the revoke's effect on the service
/// row could not be observed.
///
/// The route table has no removal path, so the slot stays keyed (vacant) for
/// the boot; hence test-hooks only, like the synthetic BAR window.
#[cfg(feature = "test-hooks")]
fn bind_driver_nic_route(bdf_claimed: bool) -> Option<usize> {
    use crate::cell::service_registry;
    use abi::syscall::service::NIC_DRIVER;
    // A row this early would be replaced by the publication below.
    if !bdf_claimed || service_registry::lookup(NIC_DRIVER).is_some() {
        log::error!("[selftest] REVOKE-PRIVILEGED: NIC route fixture unavailable");
        return None;
    }
    let provider = (DRIVER_TID, 0x3333, 1);
    let index = crate::dev::driver_cell::publish_nic_route(driver_nic_key()?, provider, || {
        service_registry::register(NIC_DRIVER, provider.0, provider.1, provider.2)
    })?;
    (service_registry::lookup(NIC_DRIVER) == Some(DRIVER_TID)).then_some(index)
}

/// After the revoke freed `DRIVER_BDF` from the still-live driver: its
/// `service::NIC_DRIVER` row is gone (the primary had no live successor) and a
/// new owner of the function binds the same route rather than being refused as
/// held. The replacement is torn down again before returning.
#[cfg(feature = "test-hooks")]
fn driver_nic_route_vacated(index: usize) -> bool {
    use crate::dev::ownership;
    let row_gone = crate::cell::service_registry::lookup(abi::syscall::service::NIC_DRIVER)
        != Some(DRIVER_TID);
    let rebound = ownership::claim_pci(DRIVER_BDF, REPLACEMENT_TID)
        && driver_nic_key().is_some_and(|key| {
            crate::dev::driver_cell::publish_nic_route(key, (REPLACEMENT_TID, 0x3334, 1), || true)
                == Some(index)
        });
    crate::dev::driver_cell::deregister_all_for(REPLACEMENT_TID);
    ownership::release_for(REPLACEMENT_TID, ownership::Scope::Retired);
    if !(row_gone && rebound) {
        log::error!(
            "[selftest] REVOKE-PRIVILEGED: NIC route {} not vacated (row_gone={} rebound={})",
            index,
            row_gone,
            rebound
        );
    }
    row_gone && rebound
}
