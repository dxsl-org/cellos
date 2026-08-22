//! Two-hart runtime regressions for fault retirement, quota-saturated clean
//! root Exit, and root quiescence.
//!
//! Hart 1 first holds a worker after scheduler selection but before the raw
//! switch. Hart 0 then saturates the root Cell and invokes the public clean
//! `Exit`; the syscall must publish only its fixed scalar record and surrender
//! allocation attribution before scheduler retirement. The later remote fault
//! and root teardown prove switch completion gates Context free, owner release,
//! and CellId reuse.

use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use super::syscall::{handle_syscall, Syscall, SyscallError};
use api::syscall::ViSyscall;
use types::CellId;

const CELL_RAW: u64 = 63;
const ROOT_EXIT_CODE: usize = 0x63;
const IDLE: u8 = 0;
const WAITING_FOR_SELECTION: u8 = 1;
const SELECTED_BEFORE_EXECUTING: u8 = 2;
const ALLOW_SELECTED_SWITCH: u8 = 3;
const WORKER_EXECUTING: u8 = 4;
const SWITCHED_AWAY: u8 = 5;
const ALLOW_COMPLETION: u8 = 6;
const COMPLETED: u8 = 7;

static PHASE: AtomicU8 = AtomicU8::new(IDLE);
static FORCED_SSIP_ARMED: AtomicBool = AtomicBool::new(false);
static FORCED_SSIP_DELIVERED: AtomicBool = AtomicBool::new(false);
static FORCED_SSIP_EARLY: AtomicBool = AtomicBool::new(false);
static CELL_ID_RELEASE_ORDER_OK: AtomicBool = AtomicBool::new(false);
static FAULT_ARMED: AtomicBool = AtomicBool::new(false);
static FAULT_DIRECT_TRIGGERED: AtomicBool = AtomicBool::new(false);
static FAULT_TASK_ENTRY: AtomicBool = AtomicBool::new(false);
static FAULT_DEFERRED_COMMITTED: AtomicBool = AtomicBool::new(false);
static FAULT_ENTERED: AtomicBool = AtomicBool::new(false);
static FAULT_SCHEDULER_FUNNEL_ATTEMPT: AtomicBool = AtomicBool::new(false);
static FAULT_RETIRED: AtomicBool = AtomicBool::new(false);
static FAULT_QUOTA_EXHAUSTED: AtomicBool = AtomicBool::new(false);
static FAULT_KERNEL_ATTRIBUTION: AtomicBool = AtomicBool::new(false);
static FAULT_SCHEDULER_GUARD_HELD: AtomicBool = AtomicBool::new(false);
static FAULT_SWITCH_ALLOWED: AtomicBool = AtomicBool::new(false);
static FAULT_WORKER_TID: AtomicUsize = AtomicUsize::new(0);
static RETIRING_ROOT_TID: AtomicUsize = AtomicUsize::new(0);
static RETIRING_SYSCALLS_DENIED: AtomicBool = AtomicBool::new(false);
static ROOT_EXIT_QUOTA_SATURATED: AtomicBool = AtomicBool::new(false);
static ROOT_EXIT_DEFERRED_COMMITTED: AtomicBool = AtomicBool::new(false);
static ROOT_EXIT_KERNEL_ATTRIBUTION: AtomicBool = AtomicBool::new(false);
static ROOT_EXIT_QUOTA_RELEASED: AtomicBool = AtomicBool::new(false);
const HEARTBEAT_CELL_RAW: u64 = 62;
static HEARTBEAT_TERMINAL_TID: AtomicUsize = AtomicUsize::new(0);
static HEARTBEAT_CURRENT_RETAINED: AtomicBool = AtomicBool::new(false);
static HEARTBEAT_BOOT_COMPLETED: AtomicBool = AtomicBool::new(false);

/// Assert that the actual quota-release boundary exposes a CellId only after
/// the scheduler has discarded the matching retiring owner slot.
///
/// `yield_cpu` invokes this immediately after `deregister`.  Reserving every
/// lower available quota slot forces the production `reserve_next` path to
/// select this retiring generation's CellId. With the old quota-first ordering,
/// that admission succeeds while the owner slot is still `Retiring`.
pub fn observe_cell_id_release(owner: api::cell_owner::CellOwner) {
    if owner.cell_id != CELL_RAW {
        return;
    }

    let owner_slot_released = {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .is_some_and(|scheduler| scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW)))
    };
    let mut lower_reservations = vec![];
    for raw in 1..CELL_RAW {
        if let Ok(reservation) = crate::memory::cell_quota::QuotaReservation::reserve(
            CellId(raw),
            crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
        ) {
            lower_reservations.push(reservation);
        }
    }
    let quota_released = crate::memory::cell_quota::QuotaReservation::reserve_next(
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
    )
    .map(|reservation| {
        let reused_retiring_cell = reservation.cell_id() == CellId(CELL_RAW);
        drop(reservation);
        reused_retiring_cell
    })
    .unwrap_or(false);
    drop(lower_reservations);

    if owner_slot_released && quota_released {
        CELL_ID_RELEASE_ORDER_OK.store(true, Ordering::Release);
        if owner.root_tid as usize == RETIRING_ROOT_TID.load(Ordering::Acquire) {
            ROOT_EXIT_QUOTA_RELEASED.store(true, Ordering::Release);
            log::info!(
                "[selftest] SMP-ROOT-EXIT-QUOTA: stage=clean-exit-terminal-quota-release"
            );
        }
        log::info!(
            "[selftest] SMP-RETIREMENT: stage=cell-id-admission-after-owner-release"
        );
    } else {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — CellId admission preceded owner release owner-empty={} quota-reusable={}",
            owner_slot_released,
            quota_released,
        );
    }
}

/// The root's clean `Exit` reaches this hook only after publishing its scalar
/// record and changing allocation attribution to kernel Cell 0. The root's
/// quota was deliberately saturated immediately before the syscall.
pub fn observe_exit_deferred_record_commit(exit: super::hart_local::DeferredExit) {
    if exit.tid != RETIRING_ROOT_TID.load(Ordering::Acquire)
        || exit.cell_id != CELL_RAW as usize
        || exit.code != ROOT_EXIT_CODE
    {
        return;
    }

    let kernel_attribution = super::hart_local::current_cell_id() == 0;
    let allocator_accepts = crate::memory::cell_quota::charge(
        super::hart_local::current_cell_id(),
        1,
    );
    if allocator_accepts {
        crate::memory::cell_quota::refund(0, 1);
    }
    if ROOT_EXIT_QUOTA_SATURATED.load(Ordering::Acquire)
        && kernel_attribution
        && allocator_accepts
    {
        ROOT_EXIT_DEFERRED_COMMITTED.store(true, Ordering::Release);
        ROOT_EXIT_KERNEL_ATTRIBUTION.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-ROOT-EXIT-QUOTA: stage=hart0-clean-exit-fixed-record-kernel-attribution"
        );
    } else {
        log::error!(
            "[selftest] SMP-ROOT-EXIT-QUOTA: FAIL — saturated root Exit retained victim allocation attribution"
        );
    }
}

/// The heartbeat sweep invokes this while a terminal task still owns the
/// outgoing Context. A direct trap dispatch must remain a nonzero caller and
/// therefore deny an allowlisted-only syscall rather than inheriting kernel 0.
pub fn observe_heartbeat_terminal_current() {
    let tid = HEARTBEAT_TERMINAL_TID.load(Ordering::Acquire);
    if tid == 0 {
        return;
    }

    let hart = super::hart_local::current_hart_id();
    let current_tid = super::hart_local::ready::current_task_id_for(hart);
    let current_cell = super::hart_local::current_cell_id();
    let no_successor = super::hart_local::ready::total_ready_count() == 0;
    let direct_denied = matches!(
        handle_syscall(tid, Syscall::ReadLog { buf_ptr: 0, max: 0 }),
        Err(SyscallError::PermissionDenied)
    );
    let mut frame = crate::hal::arch::ViTrapFrame::default();
    frame.regs[17] = ViSyscall::ReadLog as usize;
    super::syscall::ViCell_syscall_dispatch(&mut frame);
    let trap_denied = frame.regs[10] == usize::MAX;

    if current_tid == tid
        && current_cell == HEARTBEAT_CELL_RAW as usize
        && no_successor
        && direct_denied
        && trap_denied
    {
        HEARTBEAT_CURRENT_RETAINED.store(true, Ordering::Release);
        log::info!(
            "[selftest] HEARTBEAT-TERMINAL-IDENTITY: stage=current-retained-no-successor-readlog-denied tid={} cell={}",
            tid,
            current_cell,
        );
    } else {
        log::error!(
            "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — current={} cell={} no-successor={} direct-denied={} trap-denied={}",
            current_tid,
            current_cell,
            no_successor,
            direct_denied,
            trap_denied,
        );
    }
}

/// Only the incoming boot context can clear the task/Cell tuple for the
/// heartbeat regression's terminal switch.
pub fn observe_heartbeat_boot_completion(hart: usize) {
    if HEARTBEAT_TERMINAL_TID.load(Ordering::Acquire) == 0
        || !HEARTBEAT_CURRENT_RETAINED.load(Ordering::Acquire)
    {
        return;
    }

    let current_tid = super::hart_local::ready::current_task_id_for(hart);
    let executing_tid = super::hart_local::ready::executing_task_id_for(hart);
    let current_cell = super::hart_local::current_cell_id();
    if current_tid == 0 && executing_tid == 0 && current_cell == 0 {
        HEARTBEAT_BOOT_COMPLETED.store(true, Ordering::Release);
        log::info!(
            "[selftest] HEARTBEAT-TERMINAL-IDENTITY: stage=boot-completion-cleared-identity"
        );
    } else {
        log::error!(
            "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — boot completion current={} executing={} cell={}",
            current_tid,
            executing_tid,
            current_cell,
        );
    }
}



fn wait_for(expected: u8) {
    while PHASE.load(Ordering::Acquire) != expected {
        core::hint::spin_loop();
    }
}

/// Record the worker's real synthetic trap trigger before it enters the
/// architecture-visible fault entry point.
pub fn observe_direct_fault_trigger() {
    let hart = super::hart_local::current_hart_id();
    let tid = super::hart_local::ready::current_task_id_for(hart);
    if hart == super::smp::HART_RT
        && tid != 0
        && tid == FAULT_WORKER_TID.load(Ordering::Acquire)
        && super::hart_local::current_cell_id() == CELL_RAW as usize
    {
        FAULT_DIRECT_TRIGGERED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-direct-fault-trigger-entered tid={}",
            tid
        );
    }
}

/// Record entry through the same exported fault symbol used by the RISC-V trap
/// handler. This only publishes state; the record is committed below after
/// allocation attribution has become kernel-owned.
pub fn observe_fault_task_entry() {
    let hart = super::hart_local::current_hart_id();
    let tid = super::hart_local::ready::current_task_id_for(hart);
    if hart == super::smp::HART_RT
        && tid != 0
        && tid == FAULT_WORKER_TID.load(Ordering::Acquire)
        && FAULT_DIRECT_TRIGGERED.load(Ordering::Acquire)
    {
        FAULT_TASK_ENTRY.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-task-fault-entry tid={}",
            tid
        );
    }

}
/// Mark the real fixed-record publication after `defer_fault` has released its
/// pending flag and the faulting Cell is no longer charged for this path.
pub fn observe_fault_deferred_record_commit(fault: super::hart_local::DeferredFault) {
    if fault.tid != 0
        && fault.tid == FAULT_WORKER_TID.load(Ordering::Acquire)
        && fault.cell_id == CELL_RAW as usize
        && FAULT_TASK_ENTRY.load(Ordering::Acquire)
        && super::hart_local::current_cell_id() == 0
    {
        FAULT_DEFERRED_COMMITTED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-deferred-fault-record-committed tid={}",
            fault.tid
        );
    }
}

/// Mark entry to the recoverable-fault funnel before it attempts to acquire
/// SCHEDULER. The primary hart deliberately owns that lock at this point.
pub fn observe_fault_funnel_entry(tid: usize) {
    if tid != 0
        && tid == FAULT_WORKER_TID.load(Ordering::Acquire)
        && FAULT_DEFERRED_COMMITTED.load(Ordering::Acquire)
    {
        FAULT_ENTERED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-fault-entered-scheduler-owned-funnel tid={}",
            tid
        );
    }
}

/// Mark the scheduler-side handoff immediately before its real lock attempt.
/// H0 may release its deliberately retained guard only after this is visible:
/// publication at trap entry alone does not prove the worker reached the
/// scheduler funnel rather than being preempted between the two phases.
pub fn observe_fault_scheduler_funnel_attempt(tid: usize) {
    if tid != 0
        && tid == FAULT_WORKER_TID.load(Ordering::Acquire)
        && FAULT_ENTERED.load(Ordering::Acquire)
    {
        FAULT_SCHEDULER_FUNNEL_ATTEMPT.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-scheduler-funnel-pre-lock-attempt-published tid={}",
            tid
        );
    }
}

/// Mark the point at which scheduler-owned task retirement completed.  This
/// must not become visible until the remote SCHEDULER owner releases its guard.
pub fn observe_fault_scheduler_retirement(tid: usize) {
    if tid != 0 && tid == FAULT_WORKER_TID.load(Ordering::Acquire) {
        FAULT_RETIRED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-worker-retired-by-scheduler tid={}",
            tid
        );
    }
}

/// Prove that the trap changed allocation attribution before scheduling work.
/// Pause here until hart 0 has acquired `SCHEDULER`: that makes the following
/// deferred-retirement attempt contend with a real remote owner, rather than
/// merely observing a guard that happened to be held before the handoff.
pub fn observe_fault_kernel_attribution(victim_cell: usize) {
    let tid = super::hart_local::ready::current_task_id_for(super::hart_local::current_hart_id());
    if victim_cell != CELL_RAW as usize || tid != FAULT_WORKER_TID.load(Ordering::Acquire) {
        return;
    }
    let kernel_attribution = super::hart_local::current_cell_id() == 0;
    let allocator_accepts = crate::memory::cell_quota::charge(
        super::hart_local::current_cell_id(),
        1,
    );
    if allocator_accepts {
        crate::memory::cell_quota::refund(0, 1);
    }
    if kernel_attribution && allocator_accepts {
        FAULT_KERNEL_ATTRIBUTION.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=quota-exhausted-fault-handoff-kernel-attribution"
        );
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-attribution-paused-awaiting-hart0-guard"
        );
        while !FAULT_SCHEDULER_GUARD_HELD.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-hart0-guard-observed-entering-scheduler-funnel"
        );
    } else {
        log::error!(
            "[selftest] SMP-FAULT-RETIREMENT: FAIL — victim quota remained attributed during deferred fault handoff"
        );
    }
}

/// Keep the faulted worker alive after its scheduler transition so hart 0 can
/// inspect the exact zombie/owner boundary before the remote context switches.
pub fn hold_after_fault_scheduler_retirement(tid: usize) {
    if tid != 0 && tid == FAULT_WORKER_TID.load(Ordering::Acquire) {
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=worker-zombie-published-before-remote-switch tid={}",
            tid
        );
        while !FAULT_SWITCH_ALLOWED.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
    }
}
/// Hold the RT hart after scheduler selection but before the raw switch.
///
/// The scheduler lock has already been released, allowing hart 0 to retire the
/// root and run reaping while the incoming worker Context is ownership-pinned
/// only by `selected_task_id`.
pub fn hold_after_selection_before_switch(hart: usize) {
    if hart != super::smp::HART_RT {
        return;
    }
    if PHASE
        .compare_exchange(
            WAITING_FOR_SELECTION,
            SELECTED_BEFORE_EXECUTING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        log::info!("[selftest] SMP-RETIREMENT: stage=selected-pre-executing-hold");

        let sstatus: usize;
        // SAFETY: reading sstatus and setting the calling hart's SSIP bit are
        // permitted in S-mode. `yield_cpu` must already have cleared SIE before
        // this post-pick hook can run.
        unsafe {
            core::arch::asm!(
                "csrr {status}, sstatus",
                status = out(reg) sstatus,
                options(nomem, nostack),
            );
        }
        if sstatus & 0x2 != 0 {
            FORCED_SSIP_EARLY.store(true, Ordering::Release);
            log::error!(
                "[selftest] SMP-RETIREMENT: FAIL — post-pick hook remained interruptible"
            );
        } else {
            FORCED_SSIP_ARMED.store(true, Ordering::Release);
            unsafe {
                core::arch::asm!(
                    "csrs sip, {ssip}",
                    ssip = in(reg) 0x2usize,
                    options(nomem, nostack),
                );
            }
            if FORCED_SSIP_DELIVERED.load(Ordering::Acquire) {
                FORCED_SSIP_EARLY.store(true, Ordering::Release);
                log::error!(
                    "[selftest] SMP-RETIREMENT: FAIL — forced SSIP nested before raw switch"
                );
            } else {
                log::info!(
                    "[selftest] SMP-RETIREMENT: stage=forced-post-pick-ssip-deferred"
                );
            }
        }
        wait_for(ALLOW_SELECTED_SWITCH);
    }
}

/// Observe trap entry before `vi_timer_tick` performs any scheduler work.
///
/// A forced SSIP armed in the post-pick hook may become deliverable only after
/// the complete incoming context restores its original SIE state. Any entry
/// while the selection hold is active proves the old nested-scheduling window.
pub fn observe_forced_ssip_trap() {
    if super::hart_local::current_hart_id() != super::smp::HART_RT {
        return;
    }

    let sip: usize;
    // The SSIP handler clears sip.SSIP before entering `vi_timer_tick`. If the
    // bit is still pending, this was a timer/external scheduling tick and must
    // not consume the forced-interrupt observation.
    unsafe {
        core::arch::asm!(
            "csrr {pending}, sip",
            pending = out(reg) sip,
            options(nomem, nostack),
        );
    }
    if sip & 0x2 != 0 || !FORCED_SSIP_ARMED.swap(false, Ordering::AcqRel) {
        return;
    }

    if PHASE.load(Ordering::Acquire) == SELECTED_BEFORE_EXECUTING {
        FORCED_SSIP_EARLY.store(true, Ordering::Release);
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — forced SSIP entered post-pick window"
        );
    } else {
        FORCED_SSIP_DELIVERED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-RETIREMENT: stage=forced-post-pick-ssip-delivered-after-switch"
        );
    }
}



/// Called from the incoming side of RV64 `Context::switch`, after the new stack
/// is active but before `complete_retirement_switch` publishes its epoch.
pub fn hold_before_switch_completion(hart: usize) {
    if hart != super::smp::HART_RT {
        return;
    }
    if PHASE
        .compare_exchange(
            WORKER_EXECUTING,
            SWITCHED_AWAY,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        // This executes on the incoming boot stack, after the remote worker's
        // stack is no longer live and before the retirement epoch is visible.
        log::info!("[selftest] SMP-RETIREMENT: stage=post-stack-pre-epoch-hold");
        wait_for(ALLOW_COMPLETION);
    }
}

/// Record that the held incoming context has now published the completion epoch.
pub fn observe_switch_completion(hart: usize) {
    if hart == super::smp::HART_RT
        && PHASE.load(Ordering::Acquire) == ALLOW_COMPLETION
    {
        // `complete_retirement_switch` has already made the raw-switch epoch
        // visible. Publish the selftest observation only after that boundary.
        log::info!("[selftest] SMP-RETIREMENT: stage=completion-epoch-published");
        PHASE.store(COMPLETED, Ordering::Release);
    }
}

/// Verify that the first remote raw switch published its execution identity to
/// the destination hart.  A worker entry alone is insufficient evidence: a
/// stale task `tp` can run the callback against hart 0 and leave hart 1's
/// selected pin set forever, which blocks the retirement IPI completion.
fn verify_remote_execution_publication(worker_tid: usize) -> bool {
    let hart = super::smp::HART_RT;
    let selected = super::hart_local::ready::selected_task_id_for(hart);
    let executing = super::hart_local::ready::executing_task_id_for(hart);
    let outgoing = super::hart_local::ready::outgoing_context_save_task_id_for(hart);
    if selected == 0 && executing == worker_tid && outgoing == 0 {
        log::info!(
            "[selftest] SMP-RETIREMENT: stage=worker-execution-published hart={} selected=0 executing={} outgoing-save=0",
            hart,
            worker_tid,
        );
        true
    } else {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — worker execution publication hart={} selected={} executing={} outgoing-save={}",
            hart,
            selected,
            executing,
            outgoing,
        );
        false
    }
}

fn retiring_syscalls_denied(tid: usize) -> bool {
    matches!(
        handle_syscall(tid, Syscall::ReadLog { buf_ptr: 0, max: 0 }),
        Err(SyscallError::PermissionDenied)
    ) && matches!(
        handle_syscall(tid, Syscall::GetProcs2 { buf_ptr: 0, buf_len: 0 }),
        Err(SyscallError::PermissionDenied)
    ) && matches!(
        handle_syscall(tid, Syscall::MemInfo { out_ptr: 0, out_len: 0 }),
        Err(SyscallError::PermissionDenied)
    )
}

fn remote_dispatch_denies_retiring_syscalls() -> bool {
    [
        ViSyscall::ReadLog as usize,
        ViSyscall::GetProcs2 as usize,
        ViSyscall::MemInfo as usize,
    ]
    .iter()
    .copied()
    .all(|syscall_id| {
        let mut frame = crate::hal::arch::ViTrapFrame::default();
        frame.regs[17] = syscall_id;
        super::syscall::ViCell_syscall_dispatch(&mut frame);
        frame.regs[10] == usize::MAX
    })
}

extern "C" fn remote_worker_entry() -> ! {
    // The first incoming Context::switch has already published the executing
    // worker TID. Hart 0 arms the synthetic fault, waits until this worker has
    // changed allocation attribution to kernel, then takes SCHEDULER before the
    // worker may enter the recoverable scheduler funnel. The funnel must wait
    // for that real guard rather than clearing the lock from hart 1.
    log::info!("[selftest] SMP-RETIREMENT: stage=worker-context-entered");
    let worker_tid = super::hart_local::ready::current_task_id_for(super::smp::HART_RT);
    let root_tid = RETIRING_ROOT_TID.load(Ordering::Acquire);
    if retiring_syscalls_denied(root_tid) && remote_dispatch_denies_retiring_syscalls() {
        RETIRING_SYSCALLS_DENIED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-RETIREMENT: stage=retiring-root-worker-syscalls-denied-before-switch-completion root={} worker={}",
            root_tid,
            worker_tid,
        );
    } else {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — retiring root or remote worker bypassed syscall admission"
        );
    }
    // H0 has already saturated this generation before its clean root Exit.
    // Stack refunds performed by scheduler retirement may open a small gap;
    // fill only that gap so the existing H1 fault regression remains exactly
    // quota-saturated without double-charging past the limit.
    let used = crate::memory::cell_quota::in_use(CellId(CELL_RAW));
    let quota_charged = crate::memory::cell_quota::charge(
        CELL_RAW as usize,
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES.saturating_sub(used),
    );
    let mut allocation_probe = alloc::vec::Vec::<u8>::new();
    let allocation_rejected = allocation_probe.try_reserve_exact(1).is_err();
    let quota_exhausted = quota_charged && allocation_rejected;
    if quota_exhausted {
        FAULT_QUOTA_EXHAUSTED.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=quota-exhausted-user-fault-confirmed"
        );
    } else {
        log::error!(
            "[selftest] SMP-FAULT-RETIREMENT: FAIL — could not deterministically exhaust worker Cell quota"
        );
    }
    PHASE.store(WORKER_EXECUTING, Ordering::Release);
    // Keep SIE masked until this synthetic trap has published its fixed record.
    // The post-pick SSIP is pending here; enabling it would enter `vi_timer_tick`
    // and block on hart 0's guard before this worker reaches the fault funnel.
    // A real trap enters with SIE clear, so matching that ordering is essential.
    let _worker_sstatus = crate::hal::arch::save_and_disable_interrupts();
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-direct-trap-sie-masked"
    );
    while !FAULT_ARMED.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    observe_direct_fault_trigger();
    crate::task::terminate_test_hook_trap_proven_user_fault(0xdead, 0, 0);
    unreachable!("fault retirement must switch away from the worker context");
}

extern "C" fn heartbeat_no_successor_entry() -> ! {
    // Advance the scheduler timebase after this task has entered, making the
    // synthetic deadline deterministic rather than dependent on a timer IRQ.
    super::tick();
    super::yield_cpu();
    unreachable!("expired heartbeat task must switch to boot, never resume");
}

fn run_heartbeat_terminal_identity_regression() {
    HEARTBEAT_TERMINAL_TID.store(0, Ordering::Release);
    HEARTBEAT_CURRENT_RETAINED.store(false, Ordering::Release);
    HEARTBEAT_BOOT_COMPLETED.store(false, Ordering::Release);

    let quota = match crate::memory::cell_quota::QuotaReservation::reserve(
        CellId(HEARTBEAT_CELL_RAW),
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
    ) {
        Ok(reservation) => reservation,
        Err(_) => {
            log::error!(
                "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — quota reservation"
            );
            return;
        }
    };
    let tid = {
        let mut guard = super::SCHEDULER.lock();
        let Some(scheduler) = guard.as_mut() else {
            log::error!(
                "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — scheduler unavailable"
            );
            return;
        };
        let Ok(tid) = scheduler.spawn(
            "heartbeat-terminal-identity",
            CellId(HEARTBEAT_CELL_RAW),
            vec![],
        ) else {
            log::error!(
                "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — task stack allocation"
            );
            return;
        };
        let Some(task) = scheduler.tasks.get_mut(&tid) else {
            log::error!(
                "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — task disappeared"
            );
            return;
        };
        task.heartbeat_deadline = Some(0);
        task.syscall_allowlist = 0;
        task.context.ra = heartbeat_no_successor_entry as *const () as usize;
        let owner = api::cell_owner::CellOwner::new(
            HEARTBEAT_CELL_RAW,
            task.cell_generation,
            tid as u64,
        );
        if !scheduler.publish_live_cell_owner(owner) {
            log::error!(
                "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — owner publication"
            );
            return;
        }
        tid
    };
    quota.commit();
    HEARTBEAT_TERMINAL_TID.store(tid, Ordering::Release);
    log::info!(
        "[selftest] HEARTBEAT-TERMINAL-IDENTITY: stage=expired-task-queued tid={} cell={}",
        tid,
        HEARTBEAT_CELL_RAW,
    );

    // Boot→task then expired task→boot. The second raw switch must retain the
    // terminal nonzero caller until its incoming boot completion.
    super::yield_cpu();

    if HEARTBEAT_CURRENT_RETAINED.load(Ordering::Acquire)
        && HEARTBEAT_BOOT_COMPLETED.load(Ordering::Acquire)
    {
        log::info!(
            "[selftest] HEARTBEAT-TERMINAL-IDENTITY: PASS (heartbeat retirement retained nonzero caller through boot switch; ReadLog denied)"
        );
    } else {
        log::error!(
            "[selftest] HEARTBEAT-TERMINAL-IDENTITY: FAIL — missing terminal-current or boot-completion proof"
        );
    }

    // Reap the terminal root and release its test-only quota registration.
    super::yield_cpu();
    HEARTBEAT_TERMINAL_TID.store(0, Ordering::Release);
}

/// Run after hart 1 is online and before workload cells spawn.
pub fn run_primary() {
    if !super::smp::is_rt_hart_online() {
        log::warn!("[selftest] SMP-RETIREMENT: RUNTIME-GATED (hart 1 offline)");
        return;
    }

    PHASE.store(IDLE, Ordering::Release);
    FORCED_SSIP_ARMED.store(false, Ordering::Release);
    FORCED_SSIP_DELIVERED.store(false, Ordering::Release);
    FORCED_SSIP_EARLY.store(false, Ordering::Release);
    CELL_ID_RELEASE_ORDER_OK.store(false, Ordering::Release);
    FAULT_ARMED.store(false, Ordering::Release);
    FAULT_DIRECT_TRIGGERED.store(false, Ordering::Release);
    FAULT_TASK_ENTRY.store(false, Ordering::Release);
    FAULT_DEFERRED_COMMITTED.store(false, Ordering::Release);
    FAULT_ENTERED.store(false, Ordering::Release);
    FAULT_SCHEDULER_FUNNEL_ATTEMPT.store(false, Ordering::Release);
    FAULT_RETIRED.store(false, Ordering::Release);
    FAULT_QUOTA_EXHAUSTED.store(false, Ordering::Release);
    FAULT_KERNEL_ATTRIBUTION.store(false, Ordering::Release);
    FAULT_SCHEDULER_GUARD_HELD.store(false, Ordering::Release);
    FAULT_SWITCH_ALLOWED.store(false, Ordering::Release);
    FAULT_WORKER_TID.store(0, Ordering::Release);
    RETIRING_ROOT_TID.store(0, Ordering::Release);
    RETIRING_SYSCALLS_DENIED.store(false, Ordering::Release);
    ROOT_EXIT_QUOTA_SATURATED.store(false, Ordering::Release);
    ROOT_EXIT_DEFERRED_COMMITTED.store(false, Ordering::Release);
    ROOT_EXIT_KERNEL_ATTRIBUTION.store(false, Ordering::Release);
    ROOT_EXIT_QUOTA_RELEASED.store(false, Ordering::Release);
    let quota = match crate::memory::cell_quota::QuotaReservation::reserve(
        CellId(CELL_RAW),
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
    ) {
        Ok(reservation) => reservation,
        Err(_) => {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — CellId quota reservation");
            return;
        }
    };
    let (root_tid, worker_tid, owner) = {
        let mut guard = super::SCHEDULER.lock();
        let Some(scheduler) = guard.as_mut() else {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — scheduler unavailable");
            return;
        };
        let Ok(root_tid) = scheduler.spawn("smp-retirement-root", CellId(CELL_RAW), vec![]) else {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — root stack allocation");
            return;
        };
        super::hart_local::ready::remove_from_all(root_tid);
        let Some(root) = scheduler.tasks.get_mut(&root_tid) else {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — root task disappeared");
            return;
        };
        root.state = super::tcb::TaskState::Recv {
            mask: 0,
            buf_ptr: 0,
            buf_len: 0,
            deadline: None,
        };
        let owner = api::cell_owner::CellOwner::new(CELL_RAW, root.cell_generation, root_tid as u64);
        if !scheduler.publish_live_cell_owner(owner) {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — root owner publication");
            return;
        }

        let pages = super::stack_pages_for("smp-retirement-worker");
        let Ok(kstack) = super::stack::Stack::new_kernel(pages) else {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — worker kernel stack allocation");
            return;
        };
        let Ok(ustack) = super::stack::Stack::new_user(pages) else {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — worker user stack allocation");
            return;
        };
        // Arm the pre-switch hold while SCHEDULER is locked, before the worker
        // can be selected by hart 1.
        PHASE.store(WAITING_FOR_SELECTION, Ordering::Release);

        let worker_tid = scheduler.spawn_with_stacks_configured(
            "smp-retirement-worker",
            CellId(CELL_RAW),
            vec![],
            kstack,
            ustack,
            |worker| {
                worker.root_tid = root_tid;
                worker.cell_generation = owner.generation;
                worker.priority = api::TaskPriority::RealTime as u8;
                worker.context.ra = remote_worker_entry as *const () as usize;
            },
        );
        (root_tid, worker_tid, owner)
    };
    FAULT_WORKER_TID.store(worker_tid, Ordering::Release);
    RETIRING_ROOT_TID.store(root_tid, Ordering::Release);
    // The root-retirement funnel owns this reservation from here onward.
    quota.commit();

    log::info!("[selftest] SMP-RETIREMENT: stage=worker-queued-hart1");

    // The RT worker is queued on hart 1. Wake it immediately rather than rely
    // on a timer tick so the test observes an actual remote Context::switch.
    match super::smp::logical_sbi_target(super::smp::HART_RT) {
        Some((mask, base)) if hal::common::sbi::sbi_send_ipi(mask, base).is_ok() => {
            log::info!("[selftest] SMP-RETIREMENT: stage=worker-dispatch-ipi");
        }
        Some(_) => {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — worker-dispatch-ipi");
        }
        None => {
            log::error!("[selftest] SMP-RETIREMENT: FAIL — worker-dispatch-target");
        }
    }
    wait_for(SELECTED_BEFORE_EXECUTING);
    log::info!("[selftest] SMP-RETIREMENT: stage=selected-pre-executing-observed");

    // Saturate the root's Cell quota while H1 holds a real selected Context,
    // then exercise the public clean Exit path from H0. Its only
    // victim-attributed work is the fixed scalar record; `yield_cpu` consumes
    // it after attribution becomes kernel-owned.
    super::hart_local::set_current_cell_context(CELL_RAW as usize, owner.generation);
    let quota_charged = crate::memory::cell_quota::charge(
        CELL_RAW as usize,
        crate::memory::cell_quota::DEFAULT_QUOTA_BYTES,
    );
    let mut allocation_probe = alloc::vec::Vec::<u8>::new();
    let allocation_rejected = allocation_probe.try_reserve_exact(1).is_err();
    if !(quota_charged && allocation_rejected) {
        super::hart_local::set_current_cell_id(0);
        log::error!(
            "[selftest] SMP-ROOT-EXIT-QUOTA: FAIL — could not deterministically saturate root quota"
        );
        return;
    }
    ROOT_EXIT_QUOTA_SATURATED.store(true, Ordering::Release);
    log::info!(
        "[selftest] SMP-ROOT-EXIT-QUOTA: stage=hart0-root-quota-saturated-before-clean-exit"
    );
    if !matches!(
        handle_syscall(root_tid, Syscall::Exit { code: ROOT_EXIT_CODE }),
        Ok(0)
    ) {
        super::hart_local::set_current_cell_id(0);
        log::error!("[selftest] SMP-ROOT-EXIT-QUOTA: FAIL — clean root Exit rejected");
        return;
    }
    if !ROOT_EXIT_DEFERRED_COMMITTED.load(Ordering::Acquire)
        || !ROOT_EXIT_KERNEL_ATTRIBUTION.load(Ordering::Acquire)
    {
        log::error!(
            "[selftest] SMP-ROOT-EXIT-QUOTA: FAIL — root Exit did not defer with kernel allocation attribution"
        );
        return;
    }
    let selected_reap_blocked = {
        let guard = super::SCHEDULER.lock();
        guard.as_ref().is_some_and(|scheduler| {
            !scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW))
                && scheduler.tasks.get(&worker_tid).is_some_and(|task| {
                    task.state == super::tcb::TaskState::Retiring
                })
                && scheduler.tasks.get(&root_tid).is_some_and(|task| {
                    task.state == super::tcb::TaskState::Retiring
                })
        })
    };
    if !selected_reap_blocked {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — selected Context did not block retirement/reap"
        );
        return;
    }
    log::info!(
        "[selftest] SMP-RETIREMENT: stage=selected-context-blocked-retirement-and-reap"
    );

    PHASE.store(ALLOW_SELECTED_SWITCH, Ordering::Release);
    log::info!("[selftest] SMP-RETIREMENT: stage=selected-switch-permitted");
    wait_for(WORKER_EXECUTING);
    log::info!("[selftest] SMP-RETIREMENT: stage=worker-executing-observed");
    if !verify_remote_execution_publication(worker_tid) {
        return;
    }
    if !RETIRING_SYSCALLS_DENIED.load(Ordering::Acquire) {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — stale remote syscall denied marker missing"
        );
        return;
    }

    // Start the trap without `SCHEDULER` held. The worker pauses immediately
    // after it has deferred the fixed record and changed quota attribution to
    // Cell 0. Keep SIE masked across this complete H0 control handoff: a timer
    // yield between acquiring the guard and publishing it can re-enter
    // `SCHEDULER` on H0 while H1 is deliberately paused at attribution.
    let primary_sstatus = crate::hal::arch::save_and_disable_interrupts();
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-owner-proof-sie-masked"
    );
    FAULT_ARMED.store(true, Ordering::Release);
    while !FAULT_KERNEL_ATTRIBUTION.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-attribution-observed-attempting-scheduler-guard"
    );
    let owner_retained = {
        let _guard = super::SCHEDULER.lock();
        FAULT_SCHEDULER_GUARD_HELD.store(true, Ordering::Release);
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-scheduler-guard-acquired-published"
        );
        while !FAULT_SCHEDULER_FUNNEL_ATTEMPT.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        if !FAULT_DIRECT_TRIGGERED.load(Ordering::Acquire)
            || !FAULT_TASK_ENTRY.load(Ordering::Acquire)
            || !FAULT_DEFERRED_COMMITTED.load(Ordering::Acquire)
            || !FAULT_QUOTA_EXHAUSTED.load(Ordering::Acquire)
            || !FAULT_KERNEL_ATTRIBUTION.load(Ordering::Acquire)
        {
            log::error!(
                "[selftest] SMP-FAULT-RETIREMENT: FAIL — quota-exhausted direct fault did not complete trap entry, record commit, and kernel attribution"
            );
            false
        } else {
            let retired_while_owned = (0..100_000).any(|_| {
                let retired = FAULT_RETIRED.load(Ordering::Acquire);
                core::hint::spin_loop();
                retired
            });
            if retired_while_owned {
                log::error!(
                    "[selftest] SMP-FAULT-RETIREMENT: FAIL — hart1 retired while hart0 owned SCHEDULER"
                );
                false
            } else {
                log::info!(
                    "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-scheduler-owner-retained-hart1-retirement-blocked"
                );
                true
            }
        }
    };
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-scheduler-guard-released-worker-unblocked"
    );
    // SAFETY: this exact H0 invocation captured `primary_sstatus` above and
    // has dropped its scheduler guard before re-enabling interrupts.
    unsafe {
        crate::hal::arch::restore_sstatus(primary_sstatus);
    }
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart0-owner-proof-sie-restored"
    );
    if !owner_retained {
        return;
    }
    while !FAULT_RETIRED.load(Ordering::Acquire) {
        core::hint::spin_loop();
    }
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=worker-retirement-resumed-after-scheduler-owner-release"
    );

    // The faulted worker remains a dispatch-visible retiring record until the
    // remote context transition has completed. Inspect that exact boundary.
    let selected_window_blocked = {
        let mut guard = super::SCHEDULER.lock();
        guard.as_mut().is_some_and(|scheduler| {
            !scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW))
                && scheduler.tasks.get(&worker_tid).is_some_and(|task| {
                    task.state == super::tcb::TaskState::Retiring
                })
                && scheduler.tasks.get(&root_tid).is_some_and(|task| {
                    task.state == super::tcb::TaskState::Retiring
                })
        })
    };
    if !selected_window_blocked {
        log::error!(
            "[selftest] SMP-FAULT-RETIREMENT: FAIL — faulted worker was not retained through remote switch"
        );
        return;
    }
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=worker-retiring-record-and-root-owner-retained-until-quiescence"
    );
    FAULT_SWITCH_ALLOWED.store(true, Ordering::Release);

    // Root retirement was deliberately requested in the selected-before-
    // executing window above. The fault is idempotent against that matching
    // zombie and must not create a second lifecycle transition.

    // Reaping can proceed only after hart 1 has published the raw-switch
    // completion epoch; until then the retiring owner slot must remain occupied.
    super::yield_cpu();
    wait_for(SWITCHED_AWAY);
    super::yield_cpu();

    let replacement = api::cell_owner::CellOwner::new(
        CELL_RAW,
        owner.generation.saturating_add(1),
        owner.root_tid.saturating_add(1),
    );
    let blocked = {
        let mut guard = super::SCHEDULER.lock();
        guard.as_mut().is_some_and(|scheduler| {
            !scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW))
                && !scheduler.publish_live_cell_owner(replacement)
        })
    };
    if !blocked {
        log::error!("[selftest] SMP-RETIREMENT: FAIL — owner slot reused before completion");
        return;
    }

    PHASE.store(ALLOW_COMPLETION, Ordering::Release);
    log::info!("[selftest] SMP-RETIREMENT: stage=completion-permitted");
    wait_for(COMPLETED);
    super::yield_cpu();

    let reused = {
        let mut guard = super::SCHEDULER.lock();
        guard.as_mut().is_some_and(|scheduler| {
            let member_cleanup_complete = !scheduler
                .tasks
                .contains_key(&root_tid)
                && !scheduler.tasks.contains_key(&worker_tid)
                && !scheduler
                    .zombies
                    .iter()
                    .any(|task| task.id == root_tid || task.id == worker_tid);
            if scheduler.cell_owner_slot_is_empty(CellId(CELL_RAW))
                && member_cleanup_complete
                && scheduler.publish_live_cell_owner(replacement)
            {
                scheduler.clear_live_cell_owner_for_test(replacement);
                true
            } else {
                false
            }
        })
    };
    if !CELL_ID_RELEASE_ORDER_OK.load(Ordering::Acquire) {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — CellId quota released before owner retirement"
        );
        return;
    }
    if !ROOT_EXIT_QUOTA_RELEASED.load(Ordering::Acquire) {
        log::error!(
            "[selftest] SMP-ROOT-EXIT-QUOTA: FAIL — clean root Exit missed terminal quota release"
        );
        return;
    }
    log::info!(
        "[selftest] SMP-ROOT-EXIT-QUOTA: stage=clean-exit-terminal-release-observed"
    );
    if !FORCED_SSIP_DELIVERED.load(Ordering::Acquire)
        || FORCED_SSIP_EARLY.load(Ordering::Acquire)
    {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — forced post-pick SSIP ordering"
        );
        return;
    }


    let rt = super::smp::HART_RT;
    let idle_current = super::hart_local::ready::current_task_id_for(rt);
    let idle_executing = super::hart_local::ready::executing_task_id_for(rt);
    let idle_selected = super::hart_local::ready::selected_task_id_for(rt);
    let idle_cell = super::hart_local::HART_LOCALS[rt]
        .current_cell_id
        .load(Ordering::Acquire);
    if idle_current != 0 || idle_executing != 0 || idle_selected != 0 || idle_cell != 0 {
        log::error!(
            "[selftest] SMP-RETIREMENT: FAIL — idle attribution current={} executing={} selected={} cell={}",
            idle_current,
            idle_executing,
            idle_selected,
            idle_cell,
        );
        return;
    }
    log::info!(
        "[selftest] SMP-RETIREMENT: stage=idle-attribution-cleared current=0 executing=0 selected=0 cell=0"
    );
    log::info!(
        "[selftest] SMP-FAULT-RETIREMENT: stage=hart1-fault-retirement-quiesced"
    );

    if reused {
        log::info!(
            "[selftest] SMP-FAULT-RETIREMENT: PASS (quota-exhausted handoff used kernel attribution; remote lock retained; worker and root fault retirement quiesced before CellId reuse)"
        );
        log::info!(
            "[selftest] SMP-RETIREMENT: PASS (selected Context + zombie switch completion gate owner release + CellId reuse)"
        );
        log::info!(
            "[selftest] SMP-ROOT-EXIT-QUOTA: PASS (saturated clean root Exit used fixed handoff, kernel attribution, and exact terminal quota release)"
        );
        run_heartbeat_terminal_identity_regression();
    } else {
        log::error!(
            "[selftest] SMP-FAULT-RETIREMENT: FAIL — member cleanup or owner slot release incomplete"
        );
    }
}
