//! Deterministic two-hart regression for IPC block-before-yield handoff.
//!
//! The worker publishes `Sending` on hart 1 and deliberately remains on that
//! stack. Hart 0 receives the request, which wakes and queues the worker on
//! hart 0. A scheduler attempt must defer the queued worker until hart 1 has
//! switched to its boot Context and the incoming completion hook has recorded
//! that the old Context was saved.

use alloc::vec;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use types::CellId;

const ROOT_CELL: u64 = 61;
const WORKER_CELL: u64 = 62;
const IDLE: u8 = 0;
const WAITING_FOR_BLOCK: u8 = 1;
const BLOCKED_BEFORE_YIELD: u8 = 2;
const REMOTE_WAKE_DEFERRED: u8 = 3;
const ALLOW_ORIGIN_SWITCH: u8 = 4;
const OUTGOING_SAVE_COMPLETE: u8 = 5;
const ORIGIN_OWNERSHIP_RELEASED: u8 = 6;
const MIGRATED_AFTER_SAVE: u8 = 7;

static PHASE: AtomicU8 = AtomicU8::new(IDLE);
static ROOT_TID: AtomicUsize = AtomicUsize::new(0);
const CTX_USER_PAGE: usize = 0x0600_0000;
static WORKER_TID: AtomicUsize = AtomicUsize::new(0);
static WORKER_ORIGIN: AtomicUsize = AtomicUsize::new(0);

fn wait_for(expected: u8) {
    while PHASE.load(Ordering::Acquire) != expected {
        core::hint::spin_loop();
    }
}

/// Keep the post-init cross-hart fixture independent from production stack
/// sizing; it executes only a bounded kernel handoff path.
fn spawn_handoff_task(
    scheduler: &mut super::scheduler::Scheduler,
    name: &str,
    cell_id: CellId,
) -> Result<usize, types::ViError> {
    let kernel_stack = super::stack::Stack::new_kernel(1)?;
    let user_stack = super::stack::Stack::new_user(1)?;
    Ok(scheduler.spawn_with_stacks(name, cell_id, vec![], kernel_stack, user_stack))
}

/// Called from the incoming Context after its assembly prologue has saved the
/// outgoing hart's Context and cleared the handoff guard.
pub fn observe_outgoing_save_completion(hart: usize) {
    if hart == super::smp::HART_RT
        && PHASE
            .compare_exchange(
                ALLOW_ORIGIN_SWITCH,
                OUTGOING_SAVE_COMPLETE,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    {
        log::info!(
            "[selftest] SMP-CONTEXT-HANDOFF: stage=origin-context-saved hart={} marker=CTX-HANDOFF-02",
            hart
        );
    }
}

/// Called after the incoming Context has published execution identity and
/// cleared the prior selection. A remote waiter may safely select the worker
/// only after this point: the earlier save hook proves stack safety but can
/// still race the outgoing hart's executing-identity publication.
pub fn observe_origin_ownership_release(hart: usize) {
    if hart == super::smp::HART_RT
        && PHASE
            .compare_exchange(
                OUTGOING_SAVE_COMPLETE,
                ORIGIN_OWNERSHIP_RELEASED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    {
        log::info!(
            "[selftest] SMP-CONTEXT-HANDOFF: stage=origin-ownership-released hart={} current={} selected={} executing={} outgoing-save={}",
            hart,
            super::hart_local::ready::current_task_id_for(hart),
            super::hart_local::ready::selected_task_id_for(hart),
            super::hart_local::ready::executing_task_id_for(hart),
            super::hart_local::ready::outgoing_context_save_task_id_for(hart),
        );
    }
}

fn report_post_origin_completion(worker_tid: usize) {
    let worker_state = super::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&worker_tid))
        .map(|worker| worker.state.clone());
    let hart = super::smp::HART_RT;
    log::info!(
        "[selftest] SMP-CONTEXT-HANDOFF: stage=h0-post-origin-completion worker={} state={:?} h0-queued={} h0-reservation={} h1-reservation={} h1-current={} h1-selected={} h1-executing={} h1-outgoing-save={}",
        worker_tid,
        worker_state,
        super::hart_local::ready::test_ready_contains_on_hart(0, worker_tid),
        super::hart_local::ready::test_dispatch_reservation_task_id_for(0),
        super::hart_local::ready::test_dispatch_reservation_task_id_for(hart),
        super::hart_local::ready::current_task_id_for(hart),
        super::hart_local::ready::selected_task_id_for(hart),
        super::hart_local::ready::executing_task_id_for(hart),
        super::hart_local::ready::outgoing_context_save_task_id_for(hart),
    );
}

extern "C" fn blocked_worker_entry() -> ! {
    let worker_tid = WORKER_TID.load(Ordering::Acquire);
    let root_tid = ROOT_TID.load(Ordering::Acquire);
    let origin = super::hart_local::current_hart_id();
    WORKER_ORIGIN.store(origin, Ordering::Release);
    if origin != super::smp::HART_RT {
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-00 early-h0-steal origin={} worker={}",
            origin,
            worker_tid
        );
        loop {
            core::hint::spin_loop();
        }
    }

    if super::ipc_send(worker_tid, root_tid, CTX_USER_PAGE, 1) != Ok(1)
        || super::hart_local::ready::outgoing_context_save_task_id_for(origin) != worker_tid
    {
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-00 block-handoff origin={} worker={}",
            origin,
            worker_tid
        );
        loop {
            core::hint::spin_loop();
        }
    }

    log::info!(
        "[selftest] SMP-CONTEXT-HANDOFF: stage=blocked-before-yield hart={} marker=CTX-HANDOFF-00",
        origin
    );
    PHASE.store(BLOCKED_BEFORE_YIELD, Ordering::Release);
    // The scheduler releases this reservation as soon as hart 1 removes the
    // worker from its queue. Releasing from the worker would be too late: the
    // reservation must cover the initial cross-hart dispatch race, not the
    // worker's entire blocked lifetime.
    wait_for(ALLOW_ORIGIN_SWITCH);

    // This is the task→idle path: no peer is ready on hart 1, so its Context
    // must be saved before the hart-0 queue entry becomes selectable.
    super::yield_cpu();

    let resumed_hart = super::hart_local::current_hart_id();
    if resumed_hart != 0 {
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-03 migrated-to-wrong-hart expected=0 actual={}",
            resumed_hart
        );
        loop {
            core::hint::spin_loop();
        }
    }
    if super::hart_local::ready::test_dispatch_reservation_task_id_for(0) != 0 {
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-03 migration-pin-retained worker={}",
            worker_tid
        );
        loop {
            core::hint::spin_loop();
        }
    }
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    if !super::domain_switch_tests::resumed_worker_domain_matches(worker_tid) {
        log::error!(
            "[selftest] S22-RV64-MIGRATION: FAIL marker=DOMAIN-TUPLE worker={}",
            worker_tid
        );
        loop {
            core::hint::spin_loop();
        }
    }
    log::info!("[selftest] SMP-CONTEXT-HANDOFF: PASS marker=CTX-HANDOFF-03 hart=0");
    PHASE.store(MIGRATED_AFTER_SAVE, Ordering::Release);
    if let Some(scheduler) = super::SCHEDULER.lock().as_mut() {
        scheduler.exit_task(worker_tid, 0);
    }
    super::yield_cpu();
    loop {
        core::hint::spin_loop();
    }
}

/// Run after hart 1 is online and before workload cells spawn.
pub fn run_primary() {
    if !super::smp::is_rt_hart_online() {
        log::warn!("[selftest] SMP-CONTEXT-HANDOFF: RUNTIME-GATED (hart 1 offline)");
        return;
    }

    PHASE.store(IDLE, Ordering::Release);
    WORKER_ORIGIN.store(0, Ordering::Release);
    let test_frame = {
        let mut guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let alloc = guard.as_mut().expect("frame allocator");
        let frame = alloc.allocate_frame().expect("allocate frame for ctx test");
        let flags = crate::memory::paging::Flags::from_bits(
            crate::memory::paging::Flags::VALID
                | crate::memory::paging::Flags::USER
                | crate::memory::paging::Flags::READ
                | crate::memory::paging::Flags::WRITE,
        );
        crate::memory::paging::map_page(alloc, CTX_USER_PAGE, frame, flags).expect("map ctx user page");
        crate::hal::paging::flush_tlb_page(CTX_USER_PAGE);
        frame
    };
    let (root_tid, worker_tid) = {
        let mut guard = super::SCHEDULER.lock();
        let Some(scheduler) = guard.as_mut() else {
            log::error!("[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP scheduler-unavailable");
            return;
        };
        let Ok(root_tid) = spawn_handoff_task(scheduler, "smp-context-root", CellId(ROOT_CELL))
        else {
            log::error!("[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP root-stack");
            return;
        };
        let Ok(worker_tid) =
            spawn_handoff_task(scheduler, "smp-context-worker", CellId(WORKER_CELL))
        else {
            log::error!(
                "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP worker-stack"
            );
            return;
        };
        super::hart_local::ready::remove_from_all(root_tid);
        super::hart_local::ready::remove_from_all(worker_tid);
        let Some(root) = scheduler.tasks.get_mut(&root_tid) else {
            log::error!("[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP root-lost");
            return;
        };
        // The worker must block rather than deliver immediately. Hart 0 will
        // call `ipc_recv` below to perform the wake at the controlled point.
        root.state = super::tcb::TaskState::Running;
        let Some(worker) = scheduler.tasks.get_mut(&worker_tid) else {
            log::error!(
                "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP worker-lost"
            );
            return;
        };
        #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
        {
            let binding = (|| {
                let kernel_stack = worker
                    .kernel_stack
                    .as_ref()
                    .ok_or(crate::memory::address_space::AddressSpaceError::InvalidMapping)?;
                let mut builder = crate::memory::address_space::AddressSpaceBuilder::new();
                builder.map_registered_execution(kernel_stack);
                builder.map_user_page(
                    CTX_USER_PAGE,
                    crate::memory::address_space::MappingKind::Private,
                    crate::memory::paging::Flags::from_bits(
                        crate::memory::paging::Flags::VALID
                            | crate::memory::paging::Flags::USER
                            | crate::memory::paging::Flags::READ
                            | crate::memory::paging::Flags::WRITE,
                    ),
                )?;
                let space = builder.build()?;
                worker.bind_address_space_for_test(space.clone());
                Ok::<alloc::sync::Arc<crate::memory::address_space::AddressSpace>, crate::memory::address_space::AddressSpaceError>(space)
            })();
            if let Err(error) = binding {
                log::error!(
                    "[selftest] S22-RV64-MIGRATION: FAIL marker=DOMAIN-BIND error={:?}",
                    error
                );
                return;
            }
        }
        worker.context.ra = blocked_worker_entry as *const () as usize;
        super::hart_local::ready::push_on_hart(0, worker_tid, worker.priority);
        super::hart_local::ready::remove_from_all(worker_tid);
        if !super::hart_local::ready::reserve_test_dispatch_on_hart(super::smp::HART_RT, worker_tid)
        {
            log::error!(
                "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP dispatch-pin"
            );
            return;
        }
        super::hart_local::ready::push_on_hart(super::smp::HART_RT, worker_tid, worker.priority);
        (root_tid, worker_tid)
    };

    ROOT_TID.store(root_tid, Ordering::Release);
    WORKER_TID.store(worker_tid, Ordering::Release);
    PHASE.store(WAITING_FOR_BLOCK, Ordering::Release);
    let Some((mask, base)) = super::smp::logical_sbi_target(super::smp::HART_RT) else {
        let _ = super::hart_local::ready::release_test_dispatch_on_hart(
            super::smp::HART_RT,
            worker_tid,
        );
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP dispatch-target"
        );
        return;
    };
    if hal::common::sbi::sbi_send_ipi(mask, base).is_err() {
        let _ = super::hart_local::ready::release_test_dispatch_on_hart(
            super::smp::HART_RT,
            worker_tid,
        );
        log::error!("[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-SETUP dispatch-ipi");
        return;
    }

    wait_for(BLOCKED_BEFORE_YIELD);
    if WORKER_ORIGIN.load(Ordering::Acquire) != super::smp::HART_RT {
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-00 early-h0-steal-observed origin={}",
            WORKER_ORIGIN.load(Ordering::Acquire)
        );
        return;
    }
    if super::ipc_recv(root_tid, 0, CTX_USER_PAGE + 16, 1) != Ok(worker_tid) {
        log::error!("[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-01 wake");
        return;
    }

    // Retain the wake on hart 0 after it becomes eligible. Without this
    // test-only reservation, hart 1 can steal the already-queued wake after
    // its Context save completes but before hart 0 performs its second yield.
    // Local selection on hart 0 remains unmodified.
    if !super::hart_local::ready::reserve_test_dispatch_on_hart(0, worker_tid) {
        log::error!("[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-01 migration-pin");
        return;
    }

    // Hart 0 has the wake queued, but eligibility must reject it while hart 1
    // still owns the worker's live stack. No eligible work exists, so this
    // returns without a raw switch and leaves the queue entry intact.
    super::yield_cpu();
    if super::hart_local::ready::outgoing_context_save_task_id_for(super::smp::HART_RT)
        != worker_tid
    {
        let _ = super::hart_local::ready::release_test_dispatch_on_hart(0, worker_tid);
        log::error!(
            "[selftest] SMP-CONTEXT-HANDOFF: FAIL marker=CTX-HANDOFF-01 selection-ran-before-save"
        );
        return;
    }
    log::info!("[selftest] SMP-CONTEXT-HANDOFF: stage=remote-wake-deferred marker=CTX-HANDOFF-01");
    PHASE.store(REMOTE_WAKE_DEFERRED, Ordering::Release);
    PHASE.store(ALLOW_ORIGIN_SWITCH, Ordering::Release);
    wait_for(OUTGOING_SAVE_COMPLETE);
    // CTX02 proves the old stack was saved; wait for the same switch to clear
    // H1's selected/executing identity before asking H0 to pick the queued
    // worker. Otherwise H0 can correctly defer it once and never receive a
    // second local scheduling trigger.
    wait_for(ORIGIN_OWNERSHIP_RELEASED);
    report_post_origin_completion(worker_tid);

    // The same queued wake is now eligible and must resume exactly once.
    super::yield_cpu();
    wait_for(MIGRATED_AFTER_SAVE);
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    log::info!("S22-RV64-MIGRATION: PASS harts=2");
    // This is the stable UART oracle. `MIGRATED_AFTER_SAVE` is published only
    // after CTX03, and the preceding phase transitions prove CTX00-02.
    log::info!("[selftest] SMP-CONTEXT-HANDOFF: PASS aggregate=CTX00-03");
    if let Some(scheduler) = super::SCHEDULER.lock().as_mut() {
        scheduler.exit_task(root_tid, 0);
    }
    let _ = crate::memory::paging::unmap_page(CTX_USER_PAGE);
    crate::hal::paging::flush_tlb_page(CTX_USER_PAGE);
    if let Some(alloc) = crate::memory::frame::FRAME_ALLOCATOR.lock().as_mut() {
        alloc.deallocate_frame(test_frame);
    }
    super::yield_cpu();
}
