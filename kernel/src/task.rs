// This module's Result<_, ()> IPC/task-management functions predate a proper
// kernel error-type design; redesigning ~20 signatures (and every call site
// matching Err(())) is out of scope for a lint cleanup — tracked separately.
#![allow(clippy::result_unit_err)]

pub mod cap;
pub(crate) mod child_context;
pub mod completion;
#[cfg(not(cellos_posture = "release"))]
pub mod completion_selftest;
pub mod completion_wait;
pub(crate) mod copy_glue;
pub mod dir_inherit;
#[cfg(cellos_domain_grants)]
pub(crate) mod domain_grant;
/// Publish/revoke ordering for `domain_grant`; host-tested on its own.
#[cfg(any(test, cellos_domain_grants))]
#[path = "task/domain-grant-gate.rs"]
pub(crate) mod domain_grant_gate;
mod elf_prepare;
/// Test-hooks switches that disable one guarded grant/copy check for a
/// negative fixture's CONTROL run; production kernels have none.
#[cfg(all(feature = "test-hooks", feature = "native-domains", cellos_wide))]
pub(crate) mod fixture_controls;
/// Phase-01 containment witness for domain-backed zero-copy grants, and the
/// phase-03 capability witness on every architecture whose lifecycle is open.
#[cfg(all(cellos_domain_grants, feature = "test-hooks"))]
#[cfg(not(cellos_posture = "release"))]
pub(crate) mod grant_gate_selftest;
/// Phase-15 deferred-revoke witness and grant negatives; the body is
/// arch-neutral, the x86_64 lane is the one that runs it.
#[cfg(all(cellos_domain_grants, feature = "test-hooks", target_arch = "x86_64"))]
pub(crate) mod grant_negative_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod grant_reclaim_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod manifest_v2_selftest;
/// Selective MMIO revocation for runtime revocation (`.agents/260712-1901` P03).
#[cfg(feature = "test-hooks")]
pub mod mmio_revoke_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod net_rx_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod p_trust_selftest;
/// Phase-06 witness: an RT sender woken by a consume preempts its target hart.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
#[cfg(not(cellos_posture = "release"))]
pub(crate) mod rt_wake_selftest;
pub mod syscall;
#[cfg(feature = "test-hooks")]
pub mod task_id_selftest;
pub mod tcb;
#[cfg(not(cellos_posture = "release"))]
pub mod thread_cap_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod thread_quota_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod thread_user_entry_selftest;
/// Per-task user thread pointer (TLS base) — ADR-0018 §2.1 primitive.
pub(crate) mod tls;
pub use elf_prepare::{prepare_elf_task, PreparedElfTask};
pub mod launch;
pub use launch::{
    publish_prepared, CallerLaunchAuthority, LaunchRoutes, StagedMeasurement, TaskLaunchState,
};
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
#[cfg(not(cellos_posture = "release"))]
pub mod ipc_wire_selftest;
pub use tcb::Task;
pub mod async_ipc;
#[cfg(not(cellos_posture = "release"))]
pub mod cap_file_selftest;
#[path = "task/cell-image.rs"]
mod cell_image;
#[path = "task/gpu-scanout.rs"]
mod gpu_scanout;
#[cfg(not(cellos_posture = "release"))]
pub mod ipc_guardrail_selftest;
#[cfg(not(cellos_posture = "release"))]
pub mod ipc_pending_selftest;
// Uncalled scenario harness; never compiled into a release kernel.
#[cfg(not(cellos_posture = "release"))]
pub mod ipc_test;
pub mod stack;
pub(crate) mod telemetry;
#[cfg(all(feature = "native-domains", cellos_wide))]
pub(crate) mod user_copy;
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) mod user_copy_tests;
#[cfg(feature = "test-hooks")]
pub mod user_hello;
pub mod user_out;
#[cfg(feature = "test-hooks")]
pub mod vfs_lifecycle_selftest;
pub mod waker;

use crate::sched::scheduler::Scheduler;
use crate::sync::Spinlock;
use alloc::collections::VecDeque;
#[cfg(feature = "test-hooks")]
use core::sync::atomic::AtomicU8;
use core::sync::atomic::{AtomicBool, Ordering};
use log::info;
/// Conservative fallback for every unmeasured or risk-sensitive task path.
/// Measured Phase 07 paths use the post-reactor table below; unknown paths keep
/// this historical 256 KiB allocation.
pub const STACK_PAGES: usize = 64;
/// Post-reactor floor: twice the largest measured peak, rounded up to 64 KiB.
const MEASURED_STACK_PAGES: usize = 16;
pub(crate) const TRAP_FRAME_SIZE: usize = core::mem::size_of::<crate::hal::arch::ViTrapFrame>();
extern "C" {
    fn __trap_exit();
}

/// Prime a task's first context switch to enter `entry(arg)` in user mode.
///
/// Contract:
/// - `task.kernel_stack` and `task.user_stack` must already be installed.
/// - The helper writes the initial trap frame onto the kernel stack and points
///   the saved CPU context at the architecture's user-return path.
/// - Same-cell threads inherit the current process image, so this mirrors the
///   cell-spawn user-entry contract instead of the kernel-thread trampoline.
pub(crate) fn prime_user_mode_entry(task: &mut Task, entry: usize, arg: usize) {
    let user_stack_top = task
        .user_stack
        .as_ref()
        .expect("prime_user_mode_entry requires a user stack")
        .top;
    prime_user_mode_entry_at(task, entry, arg, user_stack_top);
}

/// As [`prime_user_mode_entry`], entering with `user_sp` instead of the bare
/// user stack top. A Cell's root task enters with `sp` at its initial stack
/// block (CP1 cell-initial-stack-auxv, AX-2), which lies inside its user stack.
pub(crate) fn prime_user_mode_entry_at(task: &mut Task, entry: usize, arg: usize, user_sp: usize) {
    let kernel_stack = task
        .kernel_stack
        .as_ref()
        .expect("prime_user_mode_entry requires a kernel stack");
    let user_stack = task
        .user_stack
        .as_ref()
        .expect("prime_user_mode_entry requires a user stack");
    debug_assert!(user_sp <= user_stack.top && user_sp > user_stack.usable_start());
    let tf_ptr = kernel_stack.top - TRAP_FRAME_SIZE;
    let user_stack_top = user_sp;
    let (_gp, _tp) = get_kernel_gp_tp();

    task.trap_frame = crate::hal::arch::ViTrapFrame::default();
    task.trap_frame.sepc = entry as _;
    task.trap_frame.regs[2] = user_stack_top as _;
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        task.trap_frame.sstatus = 0x6020_u64 as _;
        task.trap_frame.regs[10] = arg as _;
    }
    #[cfg(target_arch = "aarch64")]
    {
        task.trap_frame.regs[0] = arg as _;
    }
    #[cfg(target_arch = "x86_64")]
    {
        task.trap_frame.sstatus = 0x202_u64 as _;
        task.trap_frame.regs[6] = arg as _;
    }

    unsafe {
        let tf_dest = &mut *(tf_ptr as *mut crate::hal::arch::ViTrapFrame);
        *tf_dest = task.trap_frame;
    }

    task.context.sp = tf_ptr as _;
    #[cfg(target_arch = "riscv64")]
    {
        task.context.ra = __trap_exit as *const () as usize;
        task.context.sstatus = 0x42120;
        task.context.gp = _gp;
        task.context.tp = _tp;
    }
    #[cfg(target_arch = "riscv32")]
    {
        task.context.ra = __trap_exit as *const () as u32;
        task.context.sstatus = 0x120_u32;
        task.context.gp = _gp as u32;
        task.context.tp = _tp as u32;
    }
    #[cfg(target_arch = "aarch64")]
    {
        task.context.x30 = __trap_exit as *const () as u64;
        task.context.sp_el0 = user_stack_top as u64;
    }
    #[cfg(target_arch = "x86_64")]
    {
        task.context.rip = __trap_exit as *const () as u64;
        task.context.kernel_trap_sp = tf_ptr as u64;
    }
}

// use alloc::vec::Vec;
use tcb::TaskState;
use tcb::{Delivery, WaitKind};
use types::*;

#[cfg(not(cellos_posture = "release"))]
/// Queue an owned IPC message without invoking the infallible allocation path.
///
/// Returns `Err(())` when the mailbox is full or the sender's cell cannot fund
/// the owned copy. The target is unchanged on failure.
pub(crate) fn queue_pending_msg(
    target: &mut Task,
    sender_tid: usize,
    data: &[u8],
    max_depth: usize,
) -> core::result::Result<(), ()> {
    if target.pending_msgs.len() >= max_depth {
        return Err(());
    }
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        Some(crate::p01_resources::owner(target)),
        crate::memory::heap::p01_owner::LEGACY_PAYLOAD,
    );
    let owned =
        crate::ipc::pending_mailbox::PendingMsgData::try_copy(data, target.cell_id.0 as usize)?;
    #[cfg(feature = "p01-trace")]
    drop(_semantic);
    #[cfg(feature = "p01-trace")]
    let _mailbox = crate::memory::heap::p01_owner::creation(
        Some(crate::p01_resources::owner(target)),
        crate::memory::heap::p01_owner::MAILBOX,
    );
    let result = target.pending_msgs.try_push(tcb::PendingMsg {
        sender_tid,
        data: owned,
        wire: None,
        enqueued_tick: system_ticks() as u64,
    });
    #[cfg(feature = "p01-trace")]
    drop(_mailbox);
    #[cfg(feature = "p01-trace")]
    target
        .pending_msgs
        .observe_owner(crate::p01_resources::owner(target));
    result
}

/// Publish a kernel-owned wire message into the receiver's mailbox.
///
/// The queue record retains scalar sender identity/generation in the wire
/// header; the payload never aliases sender or receiver pages. Queue-full is
/// detected here so a blocking sender never publishes into a full mailbox.
/// Publish a kernel-owned wire message into the receiver's mailbox.
///
/// The queue record carries scalar sender identity/generation in the wire
/// header. `data` is an empty inline sentinel — all consumers must read
/// payload through `PendingMsg::payload()` which routes to `wire.as_slice()`.
pub(crate) fn queue_wire_msg(
    target: &mut Task,
    message: crate::ipc::ipc_wire::IpcWireMessage,
    max_depth: usize,
) -> core::result::Result<(), ()> {
    if target.pending_msgs.len() >= max_depth {
        return Err(());
    }
    let sender_tid = message.header.sender_tid;
    #[cfg(feature = "p01-trace")]
    let _mailbox = crate::memory::heap::p01_owner::creation(
        Some(crate::p01_resources::owner(target)),
        crate::memory::heap::p01_owner::MAILBOX,
    );
    let result = target.pending_msgs.try_push(tcb::PendingMsg {
        sender_tid,
        data: crate::ipc::pending_mailbox::PendingMsgData::empty(),
        wire: Some(message),
        enqueued_tick: system_ticks() as u64,
    });
    #[cfg(feature = "p01-trace")]
    drop(_mailbox);
    #[cfg(feature = "p01-trace")]
    {
        let owner = crate::p01_resources::owner(target);
        target.pending_msgs.observe_owner(owner);
        if result.is_ok() {
            target.pending_msgs.observe_wire_commit(owner);
        }
    }
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum IpcWakeCause {
    None,
    Recv,
    NetRxCompletionWait,
}

impl IpcWakeCause {
    fn made_runnable(self) -> bool {
        self != Self::None
    }
}

/// Classify and apply the receiver wake after its wire record is published.
///
/// The caller holds `SCHEDULER` and supplies the producer's existing `Recv`
/// eligibility result. Keeping that result outside this helper preserves each
/// producer's admission/mask semantics while giving every producer one
/// `WaitCompletion(NET_RX)` interrupt path.
fn wake_after_ipc_publish(target: &mut Task, recv_eligible: bool) -> IpcWakeCause {
    let cause = if recv_eligible {
        IpcWakeCause::Recv
    } else if matches!(
        target.state,
        TaskState::Blocked { wait: WaitKind::Completion { source, delivery: Delivery::Queue, .. }, .. }
            if source & abi::completion::source::NET_RX != 0
    ) {
        IpcWakeCause::NetRxCompletionWait
    } else {
        IpcWakeCause::None
    };
    if cause.made_runnable() {
        target.state = TaskState::Ready;
    }
    cause
}

fn sender_context(sched: &Scheduler, sender_tid: usize) -> (u64, u64) {
    let Some(task) = sched.tasks.get(&sender_tid) else {
        return (0, 0);
    };
    sched
        .resolve_live_cell_owner(task.cell_id, task.cell_generation)
        .map(|owner| (owner.cell_id, owner.generation))
        .unwrap_or((0, 0))
}

/// Monotonic delivery token source. Unique per published wire message so a
/// blocked sender is woken only by the consumption of its own message.
static NEXT_DELIVERY_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

pub(crate) fn next_delivery_id() -> u64 {
    NEXT_DELIVERY_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IpcSendError {
    TargetGone,
    Backpressure,
}

/// A `Retiring` or `Terminated` task is a tombstone: dispatch-visible so stale
/// remote contexts see a terminal state, but never a receiver. `exit_task`
/// sweeps `Sending` senders once at exit, so a send that lands on a tombstone
/// afterwards would park its sender forever.
pub(crate) fn ipc_target_is_tombstone(task: &Task) -> bool {
    matches!(task.state, TaskState::Retiring | TaskState::Terminated)
}

/// Whether `target_id` is absent or a tombstone, i.e. nothing can receive from it.
fn ipc_target_gone(sched: &Scheduler, target_id: usize) -> bool {
    sched
        .tasks
        .get(&target_id)
        .is_none_or(ipc_target_is_tombstone)
}

fn paused_target_rejects(sched: &Scheduler, caller_id: usize, target_id: usize) -> bool {
    // Lock order is SCHEDULER -> service registry. PauseService releases the
    // registry lock before checking scheduler drain state.
    if sched.tasks.contains_key(&caller_id) && sched.cell_rights(caller_id).supervisor {
        return false;
    }
    let Some(target) = sched.tasks.get(&target_id) else {
        return false;
    };
    if target.hotswap.ingress_closed {
        return true;
    }
    crate::cell::service_registry::is_paused_tid(target_id)
        && !matches!(target.state, TaskState::Frozen { .. })
}
/// Arm the current hart's outgoing Context before publishing an IPC-blocked
/// state.  A peer may observe that state and wake/queue this task before the
/// syscall reaches `yield_cpu`; the handoff guard bridges that interval until
/// the raw switch has saved this stack.
#[inline(always)]
pub(crate) fn arm_ipc_block_handoff(caller_id: usize) {
    let hart = crate::sched::hart_local::current_hart_id();
    if crate::sched::hart_local::ready::current_task_id_for(hart) == caller_id {
        crate::sched::hart_local::ready::begin_outgoing_context_save(hart, caller_id);
    }
}

/// Return whether every IPC accepted before a service pause has drained.
///
/// New non-supervisor sends are rejected once the registry entry is paused.
/// The provider remains runnable until its owned mailbox and all blocking
/// senders are empty, so the following Snapshot event is ordered after them.
pub fn inbound_ipc_drained(target_id: usize) -> bool {
    SCHEDULER.lock().as_ref().is_some_and(|sched| {
        let mailbox_empty = sched
            .tasks
            .get(&target_id)
            .is_some_and(|target| target.pending_msgs.is_empty());
        mailbox_empty
            && !sched.tasks.values().any(
                |task| matches!(task.state, TaskState::Blocked { wait: WaitKind::Send { target, .. }, .. } if target == target_id),
            )
    })
}

/// Let the RV64 test-only shootdown probe consume its expected S-mode store fault.
///
/// This symbol is always linked because the HAL owns trap dispatch; production
/// builds return `false`, leaving every kernel fault on the normal panic path.
#[no_mangle]
pub extern "Rust" fn vi_tlb_shootdown_test_fault(
    frame: &mut crate::hal::arch::ViTrapFrame,
) -> bool {
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    {
        crate::arch::memory::tlb_shootdown_selftest::handle_store_fault(frame)
    }
    #[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
    {
        let _ = frame;
        false
    }
}

#[cfg(target_arch = "riscv64")]
const _: crate::hal::TlbShootdownTestFault = vi_tlb_shootdown_test_fault;

#[cfg(cellos_wide)]
#[no_mangle]
pub extern "Rust" fn vi_user_copy_guard_fault(frame: &mut crate::hal::arch::ViTrapFrame) -> bool {
    #[cfg(feature = "native-domains")]
    {
        let hart = unsafe { crate::sched::hart_local::current_hart() };
        if hart
            .user_copy_guard_active
            .load(core::sync::atomic::Ordering::Acquire)
            != 0
        {
            let fault_addr = frame.stval;
            let lo = hart
                .user_copy_guard_start
                .load(core::sync::atomic::Ordering::Acquire);
            let hi = hart
                .user_copy_guard_end
                .load(core::sync::atomic::Ordering::Acquire);
            if fault_addr >= lo && fault_addr < hi {
                let resume_pc = hart
                    .user_copy_guard_resume_pc
                    .load(core::sync::atomic::Ordering::Acquire);
                if resume_pc != 0 {
                    frame.sepc = resume_pc;
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(feature = "native-domains"))]
    {
        let _ = frame;
        false
    }
}

#[cfg(cellos_wide)]
const _: crate::hal::UserCopyGuardFault = vi_user_copy_guard_fault;

// Global Scheduler Instance
pub(crate) static SCHEDULER: Spinlock<Option<Scheduler>> =
    Spinlock::ranked(None, crate::sync::rank::SCHEDULER);
/// Dead tasks whose IOMMU teardown has not yet been hardware-acknowledged.
/// One entry is retried per scheduler pass to bound teardown latency.
#[cfg(not(feature = "p01-trace"))]
static IOMMU_CLEANUP_RETRIES: Spinlock<VecDeque<usize>> =
    Spinlock::ranked(VecDeque::new(), crate::sync::rank::IOMMU_CLEANUP_RETRIES);
#[cfg(feature = "p01-trace")]
struct RetiredDmaCleanup {
    tid: usize,
    owner: Option<crate::p01_measurement::Owner>,
    obligation: Option<crate::p01_resources::Obligation>,
}
#[cfg(feature = "p01-trace")]
static IOMMU_CLEANUP_RETRIES: Spinlock<VecDeque<RetiredDmaCleanup>> =
    Spinlock::ranked(VecDeque::new(), crate::sync::rank::IOMMU_CLEANUP_RETRIES);
/// Prevents duplicate retirement paths from observing an in-progress domain as absent.
static IOMMU_CLEANUP_SERIAL: Spinlock<()> =
    Spinlock::ranked((), crate::sync::rank::IOMMU_CLEANUP_SERIAL);

// Global Tick Counter
static TICKS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "test-hooks")]
const STACK_BASELINE_INIT: u8 = 1 << 0;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_SHELL: u8 = 1 << 1;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_VFS: u8 = 1 << 2;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_VFS_TEST: u8 = 1 << 3;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_NET: u8 = 1 << 4;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_VIRTIO_NET: u8 = 1 << 5;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_THREAD: u8 = 1 << 6;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_ALL: u8 = STACK_BASELINE_INIT
    | STACK_BASELINE_SHELL
    | STACK_BASELINE_VFS
    | STACK_BASELINE_VFS_TEST
    | STACK_BASELINE_NET
    | STACK_BASELINE_VIRTIO_NET
    | STACK_BASELINE_THREAD;
#[cfg(feature = "test-hooks")]
const STACK_BASELINE_TICK_GATE: usize = 1_500;
#[cfg(feature = "test-hooks")]
static STACK_BASELINE_EMITTED: AtomicU8 = AtomicU8::new(0);

// Every hart returns to its own scheduler/idle stack when no task is runnable.
// A shared context would restore the boot hart's `tp` on a remote fallback,
// misroute the incoming-context retirement completion to hart 0, and corrupt
// the remote hart's local scheduler state.
#[cfg(target_arch = "riscv64")]
static mut BOOT_CONTEXTS: [crate::hal::arch::Context; crate::sched::smp::MAX_HARTS] =
    [crate::hal::arch::Context {
        ra: 0,
        sp: 0,
        s0: 0,
        s1: 0,
        s2: 0,
        s3: 0,
        s4: 0,
        s5: 0,
        s6: 0,
        s7: 0,
        s8: 0,
        s9: 0,
        s10: 0,
        s11: 0,
        sepc: 0,
        sstatus: 0x102,
        gp: 0,
        tp: 0,
        sscratch: 0,
    }; crate::sched::smp::MAX_HARTS];
/// The context a hart that has never run a task saves itself into.
///
/// One slot per hart, like RV64's `BOOT_CONTEXTS`: a *shared* slot means the
/// second hart's incoming switch loads whatever the first hart last saved there.
/// Measured on a two-hart boot before this was per-hart: the incoming `eret`
/// took `elr_el1 = 0` and the kernel died on an instruction abort at `PC=0`
/// (`ec=0x21 elr=0x0 spsr=0x3C5`).
#[cfg(target_arch = "aarch64")]
const IDLE_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
    x19: 0,
    x20: 0,
    x21: 0,
    x22: 0,
    x23: 0,
    x24: 0,
    x25: 0,
    x26: 0,
    x27: 0,
    x28: 0,
    x29: 0,
    x30: 0,
    sp: 0,
    elr_el1: 0,
    spsr_el1: 0x305,
    sp_el0: 0,
    daif: 0, // saved/restored by __switch_el1; 0 = no DAIF masking (IRQs enabled)
};

#[cfg(target_arch = "aarch64")]
static mut BOOT_CONTEXTS: [crate::hal::arch::Context; crate::sched::smp::MAX_HARTS] =
    [IDLE_CONTEXT; crate::sched::smp::MAX_HARTS];
#[cfg(target_arch = "riscv32")]
static mut BOOT_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
    ra: 0,
    sp: 0,
    s0: 0,
    s1: 0,
    s2: 0,
    s3: 0,
    s4: 0,
    s5: 0,
    s6: 0,
    s7: 0,
    s8: 0,
    s9: 0,
    s10: 0,
    s11: 0,
    sepc: 0,
    sstatus: 0x102,
    gp: 0,
    tp: 0,
    sscratch: 0,
};
#[cfg(target_arch = "x86_64")]
static mut BOOT_CONTEXTS: [crate::hal::arch::Context; crate::sched::smp::MAX_HARTS] =
    [crate::hal::arch::Context {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        rbx: 0,
        rbp: 0,
        sp: 0,
        rip: 0,
        kernel_trap_sp: 0,
        user_cr3: 0,
        fp_control: hal::context::FpuControl::new(),
    }; crate::sched::smp::MAX_HARTS];
#[cfg(target_arch = "arm")]
static mut BOOT_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
    r4: 0,
    r5: 0,
    r6: 0,
    r7: 0,
    r8: 0,
    r9: 0,
    r10: 0,
    r11: 0,
    sp: 0,
    lr: 0,
    cpsr: 0x13,
};
#[cfg(target_arch = "x86")]
static mut BOOT_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
    ebx: 0,
    esi: 0,
    edi: 0,
    ebp: 0,
    sp: 0,
    eip: 0,
};

// Trampoline for Thread Spawning
// Trampoline for Thread Spawning handled by HAL

extern "C" {
    // pub fn thread_trampoline(); // In HAL
}

pub fn get_kernel_gp_tp() -> (usize, usize) {
    crate::hal::arch::get_gp_tp()
}

/// Set by every producer of scheduler housekeeping (zombies, pending
/// releases and reaps) while it holds `SCHEDULER`; `yield_cpu` drains only
/// when it is set.
static HOUSEKEEPING: AtomicBool = AtomicBool::new(false);

/// Flag scheduler housekeeping for the next `yield_cpu` on any hart. Call
/// after queuing the work, still under `SCHEDULER`.
pub(crate) fn note_housekeeping() {
    HOUSEKEEPING.store(true, Ordering::Release);
}

pub fn system_ticks() -> usize {
    TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn tick() {
    TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn stack_pages_for(name: &str) -> usize {
    match name {
        // The VFS stack was sized from a transaction-path measurement taken
        // while RedoxFS still served /srv; the current backend is CellosFS
        // Native, so the conservative value stays until a new watermark is
        // captured against it.
        "vfs" => STACK_PAGES,
        "init" | "cellbox" | "vfs-test" | "net" | "virtio-net" | "thread" | "block"
        | "platform" | "config" | "bench" | "bench-probe" => MEASURED_STACK_PAGES,
        _ => STACK_PAGES,
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn stack_sizing_policy_self_test() -> bool {
    ["init", "cellbox", "vfs-test", "net", "virtio-net", "thread"]
        .into_iter()
        .all(|name| stack_pages_for(name) == MEASURED_STACK_PAGES)
        && stack_pages_for("vfs") == STACK_PAGES
        && stack_pages_for("unmeasured-path") == STACK_PAGES
}

#[cfg(feature = "test-hooks")]
fn stack_baseline_bit(name: &str) -> Option<u8> {
    match name {
        "init" => Some(STACK_BASELINE_INIT),
        "cellbox" => Some(STACK_BASELINE_SHELL),
        "vfs" => Some(STACK_BASELINE_VFS),
        "vfs-test" => Some(STACK_BASELINE_VFS_TEST),
        "net" => Some(STACK_BASELINE_NET),
        "virtio-net" => Some(STACK_BASELINE_VIRTIO_NET),
        "thread" => Some(STACK_BASELINE_THREAD),
        _ => None,
    }
}

#[cfg(feature = "test-hooks")]
fn emit_stack_baseline(name: &str, phase: &str, kind: &str, stack: &stack::Stack) {
    let used_bytes = stack.test_hook_watermark_bytes();
    let used_pages = used_bytes.div_ceil(crate::memory::paging::PAGE_SIZE);
    // Test-hooks run after normal boot lowers the global logger to WARN, so the
    // marker must remain visible to the serial integration harness.
    log::warn!(
        "[stack-baseline] name={} phase={} kind={} used_bytes={} used_pages={} alloc_bytes={} usable_bytes={} baseline=authoritative-input",
        name,
        phase,
        kind,
        used_bytes,
        used_pages,
        stack.allocated_bytes(),
        stack.usable_bytes(),
    );
}

#[cfg(feature = "test-hooks")]
fn maybe_emit_boot_stack_baselines() {
    if system_ticks() < STACK_BASELINE_TICK_GATE {
        return;
    }
    let emitted = STACK_BASELINE_EMITTED.load(Ordering::Relaxed);
    if emitted == STACK_BASELINE_ALL {
        return;
    }
    let mut newly_emitted = 0u8;
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        for task in sched.tasks.values() {
            let Some(bit) = stack_baseline_bit(&task.name) else {
                continue;
            };
            // vfs-test has a deterministic exit after its full integration suite;
            // defer that sample so the baseline includes the complete workload.
            if bit == STACK_BASELINE_VFS_TEST {
                continue;
            }
            if emitted & bit != 0 {
                continue;
            }
            if let Some(kstack) = task.kernel_stack.as_ref() {
                emit_stack_baseline(&task.name, "boot", "kernel", kstack);
            }
            if let Some(ustack) = task.user_stack.as_ref() {
                emit_stack_baseline(&task.name, "boot", "user", ustack);
            }
            newly_emitted |= bit;
        }
    }
    if newly_emitted != 0 {
        STACK_BASELINE_EMITTED.fetch_or(newly_emitted, Ordering::Relaxed);
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn maybe_emit_exit_stack_baseline(
    name: &str,
    kernel_stack: Option<&stack::Stack>,
    user_stack: Option<&stack::Stack>,
) {
    let Some(bit) = stack_baseline_bit(name) else {
        return;
    };
    if bit != STACK_BASELINE_VFS_TEST || STACK_BASELINE_EMITTED.load(Ordering::Relaxed) & bit != 0 {
        return;
    }
    if let Some(stack) = kernel_stack {
        emit_stack_baseline(name, "exit", "kernel", stack);
    }
    if let Some(stack) = user_stack {
        emit_stack_baseline(name, "exit", "user", stack);
    }
    STACK_BASELINE_EMITTED.fetch_or(bit, Ordering::Relaxed);
}

pub fn init() {
    // Install hart-local state for hart 0 BEFORE the scheduler or timer start,
    // so current_cell_id() works correctly once interrupts are enabled.
    crate::sched::hart_local::install(0);
    // From here every lock spin answers peers' TLB-flush requests, and the
    // test-posture rank check keeps its held set per hart.
    crate::sync::install_hart_hooks(
        crate::sched::hart_local::installed_hart_id,
        crate::sched::smp::answer_tlb_flush_in_spin,
    );
    // Hart 0 is online from here on, on every architecture, so every later
    // remote shootdown or flush-epoch set names it.
    crate::sched::smp::publish_boot_hart_online();

    info!("Process: Initializing Scheduler...");
    let mut sched_guard = SCHEDULER.lock();

    // SAFETY: Use ptr::write to overwrite the Spinlock guard's data WITHOUT dropping the old value.
    // This prevents "Freed node aliases existing hole" panic on soft reboot (where .data persists but Heap is reset).
    unsafe {
        core::ptr::write(&mut *sched_guard, Some(Scheduler::new()));
    }
    drop(sched_guard);
    #[cfg(all(feature = "x86-idt-cpl3-test", target_arch = "x86_64"))]
    crate::arch::task::x86_idt_cpl3::run();

    // Enable S-mode timer interrupt and arm the first preemption tick.
    // Done after scheduler init so vi_timer_tick() sees a valid SCHEDULER.
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        crate::arch::clock::start_tick();
        info!("Timer preemption enabled (10 ms timeslice)");
    }
}

/// Exposes the trap-proven U-mode fault funnel to the RISC-V HAL via
/// `extern "Rust"` linkage.
///
/// This symbol is deliberately not a generic "terminate current Cell" entry
/// point: its only callers are trap arms that proved their saved privilege
/// state was U-mode.  Kernel panics retain Cell accounting attribution while
/// locks may be held and must take the non-recoverable panic path instead.
#[no_mangle]
pub extern "Rust" fn vi_terminate_on_user_trap_fault(cause: usize, pc: usize, fault_addr: usize) {
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::observe_fault_task_entry();
    // x86_64 #CP (vector 21) from CPL3. User shadow stacks are never enabled
    // (IA32_U_CET.SH_STK_EN stays 0), so the only CPL3 source is CET-IBT: an
    // opted-in Cell's indirect CALL/JMP reached an instruction that is not
    // ENDBR64 (`hal::cet::switch_user_cet`, `loader::gnu_property`).
    #[cfg(target_arch = "x86_64")]
    if cause == 21 {
        log::error!(
            "[cet] #CP (vector 21) at CPL3 pc={:#x}: indirect branch target is not an ENDBR64 landing pad (CET-IBT) — terminating the Cell",
            pc
        );
    }
    terminate_current_cell_on_user_trap_fault(cause, pc, fault_addr);
}

#[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
const _: crate::hal::TerminateOnUserTrapFault = vi_terminate_on_user_trap_fault;

/// Test-hook equivalent of the post-validation RV64 trap boundary.
///
/// Production code may enter the recoverable funnel only through
/// `vi_terminate_on_user_trap_fault`.  The SMP retirement self-test models an
/// already-validated U-mode fault while it holds its deterministic remote
/// scheduler guard, without fabricating an ABI caller in the HAL.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub(crate) fn terminate_test_hook_trap_proven_user_fault(
    cause: usize,
    pc: usize,
    fault_addr: usize,
) {
    crate::arch::task::retirement_selftest::observe_fault_task_entry();
    terminate_current_cell_on_user_trap_fault(cause, pc, fault_addr);
}

/// Records AArch64 exception state and the backing instruction word before
/// delegating to the architecture-neutral Cell fault teardown.
///
/// `spsr` is the saved lower-EL PSTATE from `SPSR_EL2`. The physical read uses
/// the kernel's RAM alias so a stale or unsafe execution mapping cannot trigger
/// a recursive exception while the original fault is still being handled.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "Rust" fn vi_terminate_on_fault_aarch64(
    cause: usize,
    pc: usize,
    fault_addr: usize,
    spsr: usize,
    vector_kind: usize,
) {
    let ec = cause >> 26;
    let iss = cause & 0x01ff_ffff;
    match crate::memory::paging::virt_to_phys(pc) {
        Some(phys) => {
            let word_phys = phys & !3;
            let word = unsafe {
                core::ptr::read_volatile(crate::memory::frame::phys_to_virt(word_phys) as *const u32)
            };
            log::error!(
                "[fault-probe] a64 vector={} ec={:#x} iss={:#x} spsr={:#x} pc={:#x} far={:#x} pa={:#x} word_addr={:#x} word_pa={:#010x}",
                vector_kind,
                ec,
                iss,
                spsr,
                pc,
                fault_addr,
                phys,
                word_phys,
                word
            );
        }
        None => log::error!(
            "[fault-probe] a64 vector={} ec={:#x} iss={:#x} spsr={:#x} pc={:#x} far={:#x} pa=unmapped",
            vector_kind,
            ec,
            iss,
            spsr,
            pc,
            fault_addr
        ),
    }
    // EC 0x0D: Branch Target exception — an opted-in Cell's indirect branch
    // reached a guarded page (`loader::gnu_property`) on an instruction that is
    // not a BTI landing pad compatible with ISS.BTYPE (bits [1:0]).
    if ec == 0x0d {
        log::error!(
            "[bti] Branch Target exception at EL0 pc={:#x} btype={:#b}: indirect branch target is not a BTI landing pad — terminating the Cell",
            pc,
            iss & 0b11
        );
    }
    terminate_current_cell_on_user_trap_fault(cause, pc, fault_addr);
}

#[cfg(target_arch = "aarch64")]
const _: crate::hal::TerminateOnFaultAarch64 = vi_terminate_on_fault_aarch64;

/// Exposes `crate::sched::scheduler::current_cell_id` to the HAL trap handler.
#[no_mangle]
pub extern "Rust" fn vi_current_cell_id() -> usize {
    crate::sched::scheduler::current_cell_id()
}

#[cfg(any(
    target_arch = "riscv64",
    target_arch = "riscv32",
    target_arch = "aarch64"
))]
const _: crate::hal::CurrentCellId = vi_current_cell_id;

/// Service a cross-hart IPI: the duties a remote hart asked of this one, and
/// nothing else.
///
/// An SGI is not a preemption request, and routing it through [`vi_timer_tick`]
/// was wrong in two ways that were both measured on a two-hart AArch64 boot:
///
/// * a hart parked in `wfi` (the whole life of `smp_aarch64_secondary_main`) has
///   no boot context and takes no part in scheduling, so the tick path's
///   `yield_cpu` could select a switch for a hart that cannot survive one — hart
///   1 published 33 acknowledgements and then went deaf (`TLB-ACK … epoch=35`,
///   requested epochs 36-38 never answered, the grant pair stalling on an
///   unconfirmed invalidation with 233 reaper attempts);
/// * on a hart that *is* running a task, a preemption inside the IPI re-enters the
///   scheduler between a reader's load and its use — which is how
///   `current_task_id` came to read three different ways inside a single tick
///   path.
///
/// The invalidation and the retirement request are the two things an IPI carries.
/// The local flush is issued here and its epoch published immediately; the
/// retirement is published when this hart holds no task, because "no task" is the
/// same proof a switch gives (nothing of the retiring generation is executing
/// here) and it is the only proof a parked hart can offer.
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
#[no_mangle]
pub extern "Rust" fn vi_ipi_service() {
    let hart = crate::sched::hart_local::current_hart_id();
    #[cfg(target_arch = "aarch64")]
    crate::sched::smp::note_ipi_taken(hart);
    crate::sched::smp::service_tlb_flush(hart);
    #[cfg(feature = "native-domains")]
    if crate::sched::smp::retirement_switch_pending(hart)
        && crate::sched::hart_local::ready::current_task_id_for(hart) == 0
    {
        crate::sched::smp::complete_retirement_switch(hart);
    }
    #[cfg(target_arch = "x86_64")]
    if crate::sched::smp::x86_scheduler_ready()
        && (crate::sched::smp::take_preemption(hart)
            || crate::sched::smp::retirement_switch_pending(hart))
    {
        yield_cpu();
    }
}

/// Called from the S-mode timer ISR via `extern "Rust"` linkage.
///
/// Increments the global tick counter, rearmed the timer for the next
/// 10 ms slice, and yields the CPU so the scheduler can preempt the
/// current task if a higher-priority task has become runnable.
#[no_mangle]
pub extern "Rust" fn vi_timer_tick() {
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::observe_forced_ssip_trap();

    // A hart that was asked to invalidate its TLB does so here and publishes the
    // epoch the requester waits on. This is the trap path, not the switch path:
    // an idle hart parked in WFI takes the IPI's trap and returns to its loop
    // without ever switching, so a switch-boundary hook would never run and the
    // requester would wait for an acknowledgement that could not arrive.
    #[cfg(cellos_wide)]
    {
        let hart = crate::sched::hart_local::current_hart_id();
        crate::sched::smp::service_tlb_flush(hart);

        // Same observation point, same reason, for the capture preflight's park
        // request: a hart can only stop where it is already taking the trap that
        // brings the request in. Placed after the TLB acknowledgement so a remote
        // invalidation's deadline is never extended by a park, and before `tick()`,
        // the console poll and `yield_cpu()` so the hart is parked holding no lock
        // and with no scheduler state half-changed — see
        // `crate::sched::quiesce::park_here_if_requested` for the full safe-point argument.
        #[cfg(target_arch = "riscv64")]
        crate::sched::quiesce::park_here_if_requested(hart);

        // A retirement asks this hart to prove that the context it was executing
        // has been re-entered through a switch boundary. A hart that is running
        // *no* task cannot be executing any member of the retiring generation, so
        // the request is already satisfied where it is taken — the same argument
        // the invalidation acknowledgement above rests on. A hart that *is*
        // running a task proves it by switching: the incoming side of
        // `Context::switch` publishes that epoch.
        //
        // Without this an idle hart leaves the request outstanding for ever. It
        // was measured on a two-hart boot: 531 `no switch while a retirement is
        // pending` ticks, no `remote-switch-completed`, and then the retired
        // generation's CellId blocked every later cell (the grant pair's owner
        // could not even `GrantFree`, and the receiver's revoked mapping never
        // faulted).
        if crate::sched::smp::retirement_switch_pending(hart)
            && crate::sched::hart_local::ready::current_task_id_for(hart) == 0
        {
            crate::sched::smp::complete_retirement_switch(hart);
        }
    }

    // Complete frame releases whose invalidation (a private tag or the shared
    // root) a release path could not confirm. Hart 0 owns the global sweeps,
    // this is the trap path where the acknowledgements are published, no lock
    // is held here, and the drain never waits — so it cannot widen the
    // interrupt window the trap already closed nor block the slice. Every
    // pending entry is stepped and nothing is abandoned. Before the second hart
    // is online there is no remote to survey and an empty quarantine makes the
    // call a no-op, so it is also safe on the way up.
    #[cfg(cellos_wide)]
    if crate::sched::hart_local::current_hart_id() == 0 {
        crate::memory::frame_run::reap();
    }

    #[cfg(target_arch = "x86_64")]
    if crate::sched::hart_local::current_hart_id() == 0 {
        tick();
    }
    #[cfg(not(target_arch = "x86_64"))]
    tick();

    #[cfg(feature = "test-hooks")]
    maybe_emit_boot_stack_baselines();

    // Rearm timer anchored to current mtime so the slice is constant
    // regardless of how long this ISR takes.
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        let next = hal::common::timer::read_mtime() + hal::common::timer::TICKS_PER_10MS;
        hal::common::sbi::set_timer(next);
    }

    // VirtIO input event routing is handled entirely by the input service Cell.
    // The kernel's timer tick no longer needs to poll or dispatch input events.

    // Poll UART hardware and relay any new bytes to the input service.
    // Makes UART delivery reader-independent: events arrive even when no cell
    // is currently blocked in sys_read(0).  VirtIO events were already drained
    // above, so the VirtIO section of poll() is a no-op here.
    #[cfg(target_arch = "x86_64")]
    if crate::sched::hart_local::current_hart_id() == 0 {
        crate::dev::console_drv::CONSOLE.lock().poll();
        // ACPI power button: same hart, same no-lock point as the byte relay.
        crate::arch::acpi::sci::relay_pending();
    }
    #[cfg(not(target_arch = "x86_64"))]
    crate::dev::console_drv::CONSOLE.lock().poll();

    // Run the scheduler.  If a higher-priority (or simply next round-robin)
    // task is ready, this performs a context switch.  Safe to call from the
    // timer ISR because:
    //   (a) interrupts are disabled by hardware on trap entry (sstatus.SIE=0)
    //   (b) yield_cpu() releases SCHEDULER lock before calling Context::switch
    //   (c) trap.S restores the correct ViTrapFrame from the new task's stack
    #[cfg(target_arch = "x86_64")]
    if !crate::sched::smp::x86_scheduler_ready() {
        return;
    }
    yield_cpu();
}

/// Retire a remote-root switch only from the incoming context, after the raw
/// context switch has changed stacks. Trap entry is intentionally not an ACK:
/// the interrupted retiring task can still execute its outgoing kernel path.
#[no_mangle]
pub extern "C" fn vi_context_switch_complete() {
    let hart = crate::sched::hart_local::current_hart_id();
    // A zero selected TID with an outgoing-save guard denotes task→boot,
    // rather than an unpinned task switch. Keep the terminal task's identity
    // published until this incoming boot context proves the old Context was
    // saved; otherwise a stale user trap could be dispatched as caller 0.
    let pinned = crate::sched::hart_local::ready::selected_task_id_for(hart);
    let switched_to_boot = pinned == 0
        && crate::sched::hart_local::ready::outgoing_context_save_task_id_for(hart) != 0;
    // Every hook-backed raw switch (RV64 `__switch`, x86_64 `switch_with_root`,
    // AArch64 `__switch_el1`/`__switch_el2`) reaches this callback only after
    // it saved every callee-save register and system register of the outgoing
    // Context. Release the ready-queue steal guard before publishing the
    // incoming ownership.
    crate::sched::hart_local::ready::complete_outgoing_context_save(hart);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::context_handoff_selftest::observe_outgoing_save_completion(hart);
    let selected = if pinned != 0 {
        pinned
    } else if switched_to_boot {
        0
    } else {
        crate::sched::hart_local::ready::current_task_id_for(hart)
    };
    #[cfg(feature = "g1-cpu-accounting")]
    {
        let scheduler = SCHEDULER.lock();
        let sched = scheduler.as_ref().unwrap_or_else(|| {
            crate::sched::cpu_accounting::fatal(crate::sched::cpu_accounting::Error::Busy)
        });
        crate::sched::cpu_accounting::require(crate::sched::cpu_accounting::complete(
            sched, hart, selected,
        ));
    }
    crate::sched::hart_local::ready::complete_selected_switch(hart, selected);
    if switched_to_boot {
        crate::sched::hart_local::ready::set_current_task_id(hart, 0);
        crate::sched::hart_local::set_current_cell_context(0, 0);
        #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
        crate::arch::task::retirement_selftest::observe_heartbeat_boot_completion(hart);
    }
    #[cfg(all(feature = "native-domains", cellos_wide))]
    {
        // Test posture: a switch that programmed no root left no domain
        // recorded. Checked before the safe-root acknowledge, which clears it.
        #[cfg(feature = "test-hooks")]
        crate::sched::domain_switch::check_rootless_completion(hart);
        let safe_root = crate::sched::hart_local::take_safe_root_pending();
        if switched_to_boot || safe_root {
            crate::sched::hart_local::acknowledge_safe_root();
        }
        // Safe-root completion owns outgoing attribution release: clear this
        // hart's execution pin on whatever root the completed switch left.
        // Unconditional take — a direct private→private switch leaves no
        // safe-root pending yet still owes the displaced root a release.
        if let Some(space) = crate::sched::hart_local::take_staged_execution_release() {
            let _ = space.set_current_hart(hart, false);
        }
        user_copy::clear_guard_for_context_switch();
    }
    // Phase 02 domain witnesses: read the live root on the resumed context's
    // own stack. Placed after the safe-root acknowledge on purpose — that call
    // is what makes a stale published domain id unreadable here, and it is
    // where the displaced root's release was attempted.
    #[cfg(all(
        feature = "native-domains",
        feature = "test-hooks",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ))]
    {
        crate::sched::domain_switch_tests::observe_incoming_live_root();
        crate::sched::domain_switch_tests::observe_domain_teardown(hart);
    }
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::context_handoff_selftest::observe_origin_ownership_release(hart);

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    log::info!(
        "[selftest] SMP-RETIREMENT: stage=rv64-switch-boundary hart={} selected={} executing={}",
        hart,
        selected,
        crate::sched::hart_local::ready::executing_task_id_for(hart),
    );

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::hold_before_switch_completion(hart);
    #[cfg(not(target_arch = "x86_64"))]
    crate::sched::smp::complete_retirement_switch(hart);
    #[cfg(target_arch = "x86_64")]
    crate::sched::smp::complete_captured_retirement_boundary(hart);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::observe_switch_completion(hart);
}

#[cfg(any(
    target_arch = "riscv64",
    target_arch = "riscv32",
    target_arch = "aarch64",
    target_arch = "x86_64"
))]
const _: crate::hal::TimerTick = vi_timer_tick;

/// Terminate the currently-executing Cell due to a trap-proven U-mode fault.
///
/// The trap half only snapshots fixed scalar state into the calling hart's
/// deferred-fault record, then changes allocation attribution to Cell 0.  It
/// must not clone `Task::name`, format a diagnostic, or mutate scheduler
/// collections while the exhausted victim Cell remains attributed.  The
/// scheduler half consumes the record under its normal lock and funnels the
/// matching Cell generation through `exit_task`.
///
/// # Arguments
///
/// Named for the role each value plays, not for one architecture's register:
/// `cause` is the trap syndrome, `pc` the faulting instruction address, and
/// `fault_addr` the faulting data/instruction address (zero if unavailable).
///
/// `provenance` is an unforgeable capability minted only after an architecture
/// trap handler has proved the interrupted context was U-mode and has a
/// non-zero Cell attribution. Kernel panic code has no equivalent capability
/// and cannot enter this scheduling path.
///
/// Recoverable Cell faults never force-release kernel locks.  On SMP, a lock
/// held by another hart protects live mutable state; clearing it from this hart
/// would allow concurrent access through the other hart's still-live guard.
pub fn terminate_current_cell_on_fault(
    provenance: crate::sched::hart_local::TrapProvenUserFault,
    cause: usize,
    pc: usize,
    fault_addr: usize,
) {
    let hart = crate::sched::hart_local::current_hart_id();
    let fault = crate::sched::hart_local::DeferredFault::from_user_trap(
        provenance,
        crate::sched::hart_local::ready::current_task_id_for(hart),
        crate::sched::hart_local::current_cell_id(),
        crate::sched::hart_local::current_cell_generation(),
        cause,
        pc,
        fault_addr,
    );

    // This fixed record is the only fault-path publication before the allocator
    // is uncharged from the victim.  Do not move logging, task lookup, or
    // scheduler collection work above this attribution handoff.
    crate::sched::hart_local::defer_fault(fault);
    crate::sched::hart_local::set_current_cell_id(0);

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::observe_fault_deferred_record_commit(fault);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::observe_fault_kernel_attribution(fault.cell_id);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::task::retirement_selftest::observe_fault_funnel_entry(fault.tid);

    // Switch to the next ready task.  Does not return to the faulting Cell.
    yield_cpu();
}

/// Mint the capability at an architecture trap boundary that has already
/// proved its interrupted context was U-mode.
#[inline(always)]
pub(crate) fn terminate_current_cell_on_user_trap_fault(
    cause: usize,
    pc: usize,
    fault_addr: usize,
) {
    terminate_current_cell_on_fault(
        crate::sched::hart_local::TrapProvenUserFault::new(),
        cause,
        pc,
        fault_addr,
    );
}

/// The boot/idle context slot a switch from `hart_id` saves into.
///
/// Every 64-bit architecture owns one idle context per logical CPU.
#[cfg(cellos_wide)]
#[inline]
unsafe fn boot_context_for(hart_id: usize) -> *mut crate::hal::arch::Context {
    // SAFETY: `hart_id < MAX_HARTS` — it comes from `current_hart_id()`.
    unsafe { &raw mut BOOT_CONTEXTS[hart_id] }
}

#[cfg(not(cellos_wide))]
#[inline]
unsafe fn boot_context_for(_hart_id: usize) -> *mut crate::hal::arch::Context {
    &raw mut BOOT_CONTEXT
}

/// Report, once per hart, that the scheduler handed this hart a task.
///
/// A secondary that only takes interrupts and never runs a task is
/// indistinguishable from one that schedules; this is the line that tells them
/// apart, and it is bounded so a long boot cannot flood the console. Reported
/// where `arch::sched::REPORTS_DISPATCH` is set.
fn note_task_dispatch(hart_id: usize) {
    use core::sync::atomic::{AtomicU8, Ordering};
    static REPORTED: [AtomicU8; crate::sched::smp::MAX_HARTS] =
        [const { AtomicU8::new(0) }; crate::sched::smp::MAX_HARTS];
    if let Some(slot) = REPORTED.get(hart_id) {
        if slot.swap(1, Ordering::AcqRel) == 0 {
            log::info!("[sched] hart {} dispatched a task", hart_id);
        }
    }
}

/// The selection a non-native-domain kernel switches to: every switch is SAS to
/// SAS, so it carries no root.
#[cfg(not(all(feature = "native-domains", cellos_wide)))]
struct SasSwitch {
    outgoing: *mut crate::hal::arch::Context,
    incoming: *const crate::hal::arch::Context,
}

#[cfg(not(all(feature = "native-domains", cellos_wide)))]
impl SasSwitch {
    /// Same contract as `RootDecision::switch`, with no root.
    unsafe fn switch(
        self,
        out: *mut crate::hal::arch::Context,
        inc: *const crate::hal::arch::Context,
        irq: crate::arch::sched::IrqSaved,
    ) -> crate::arch::sched::IrqSaved {
        // SAFETY: the caller's contract.
        unsafe {
            crate::arch::sched::switch(crate::arch::sched::SwitchPlan {
                out,
                inc,
                root: None,
                irq,
            })
        }
    }
}

/// Core scheduling logic: picks next task and performs switch OUTSIDE of the lock.
pub fn yield_cpu() {
    #[cfg(target_arch = "x86_64")]
    if !crate::sched::smp::x86_scheduler_ready() {
        return;
    }
    // Where the architecture masks at entry (`MASK_AT_ENTRY`), capture and mask
    // before any scheduler lock or publication. RV64 cooperative yields can
    // enter with SIE set, and masking only at `__switch` leaves current/selected
    // scheduler state interruptible; on x86_64 a LAPIC tick inside the lock
    // would re-enter `yield_cpu` and deadlock on the same spinlock.
    let entry_irq = crate::arch::sched::MASK_AT_ENTRY.then(crate::arch::sched::IrqSaved::save);

    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::observe_schedule_attempt();

    drain_before_selection();

    // AArch64 (`MASK_AT_ENTRY == false`) reaches this point from the boot idle
    // loop with IRQs unmasked, and the SCHEDULER guard restores that state when
    // it drops. Mask from selection through the raw switch: a timer IRQ in that
    // window would re-enter the scheduler after this hart published its
    // selected and outgoing-save identities but before the switch, and save the
    // IRQ activation into the selected task's Context. The incoming Context
    // restores its own interrupt state; this activation restores the captured
    // state when it resumes, at the end of `yield_cpu`.
    let irq = entry_irq.unwrap_or_else(crate::arch::sched::IrqSaved::save);
    let hart_id = crate::sched::hart_local::current_hart_id();
    #[cfg(all(feature = "native-domains", cellos_wide))]
    let selection = if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.pick_next_domain(hart_id)
    } else {
        None
    };
    #[cfg(not(all(feature = "native-domains", cellos_wide)))]
    let selection = if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched
            .pick_next(hart_id)
            .map(|(outgoing, incoming)| SasSwitch { outgoing, incoming })
    } else {
        None
    };
    // The scheduler lock is now released but the terminal Context is still
    // executing. Exercise the real trap admission boundary in this exact
    // pre-switch interval; the regression proves a heartbeat victim cannot
    // become caller 0 before boot owns the incoming Context.
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if let Some(selection) = &selection {
        if selection.incoming.is_null() {
            crate::arch::task::retirement_selftest::observe_heartbeat_terminal_current();
        }
    }
    let irq = match selection {
        None => {
            // Selection is normally published only with a concrete switch
            // tuple. Clear defensively on the no-switch/abort path so a stale
            // pin cannot retain a retired Context indefinitely. No incoming
            // Context restores the interrupt state here: the token below keeps
            // the caller's exact state instead of unconditionally enabling.
            crate::sched::hart_local::ready::abort_selected_switch(hart_id);
            irq
        }
        Some(selection) => unsafe {
            let out = if selection.outgoing.is_null() {
                boot_context_for(hart_id)
            } else {
                selection.outgoing
            };
            let inc = if selection.incoming.is_null() {
                boot_context_for(hart_id) as *const crate::hal::arch::Context
            } else {
                selection.incoming
            };
            if !selection.incoming.is_null() {
                #[cfg(feature = "bench-counters")]
                crate::bench_counters::context_switch();
                if crate::arch::sched::REPORTS_DISPATCH {
                    note_task_dispatch(hart_id);
                }
                crate::arch::sched::set_kernel_stack_for(&*selection.incoming);
                tls::install_published_base();
            }
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            crate::arch::task::retirement_selftest::hold_after_selection_before_switch(hart_id);
            selection.switch(out, inc, irq)
        },
    };

    // The incoming task is now the running one. Install its user thread pointer
    // (aarch64/x86_64; riscv64 carries it in the trap frame) from the hart-local
    // value the scheduler published, so this stays lock-free.
    tls::apply_on_resume();
    // This activation may resume on another CPU. Its captured interrupt state
    // belongs to the suspended context, not the selecting CPU; never enable
    // interrupts inside an activation that originally entered masked. (RV64's
    // token is spent: `__switch` already restored this context's `sstatus`.)
    drop(irq);
}

/// The scheduler pass before selection: retirement funnel, housekeeping
/// drains and wake delivery. Runs with the interrupt state `yield_cpu` set up.
#[inline(always)]
fn drain_before_selection() {
    #[cfg(target_arch = "x86_64")]
    crate::sched::smp::capture_retirement_boundary(crate::sched::hart_local::current_hart_id());
    #[cfg(feature = "g1-cpu-accounting")]
    {
        let scheduler = SCHEDULER.lock();
        if let Some(sched) = scheduler.as_ref() {
            crate::sched::cpu_accounting::require(crate::sched::cpu_accounting::service(
                sched,
                crate::sched::hart_local::current_hart_id(),
            ));
        }
    }

    // Reap zombies already switched away from. Take them under the lock (cheap
    // pointer moves), then drop OUTSIDE it so Stack::drop's frame-free + unmap
    // (FRAME_ALLOCATOR / KERNEL_ROOT) never run while SCHEDULER is held. This is
    // what frees a dead cell's stacks — without it every cell death leaked them
    // (e.g. the shell-supervisor restart loop would grow until OOM).

    // Trap faults and clean exits publish only fixed records. Retirement starts
    // here, in the scheduler's ordinary masked/locked phase, after attribution
    // is already kernel-owned. This is deliberately before zombie reaping and
    // task selection so every terminal path participates in the same quiescence
    // lifecycle.
    let deferred_retired = match crate::sched::hart_local::take_deferred_retirement() {
        Some(crate::sched::hart_local::DeferredRetirement::Fault(fault)) => {
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            crate::arch::task::retirement_selftest::observe_fault_scheduler_funnel_attempt(
                fault.tid,
            );
            let mut guard = SCHEDULER.lock();
            let scheduler = guard
                .as_mut()
                .expect("[fault] deferred handoff before scheduler initialization");
            scheduler.retire_deferred_fault(fault);
            Some(fault.tid)
        }
        Some(crate::sched::hart_local::DeferredRetirement::Exit(exit)) => {
            let mut guard = SCHEDULER.lock();
            let scheduler = guard
                .as_mut()
                .expect("[task] deferred Exit before scheduler initialization");
            scheduler.retire_deferred_exit(exit);
            None
        }
        None => None,
    };
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if let Some(tid) = deferred_retired {
        crate::arch::task::retirement_selftest::observe_fault_scheduler_retirement(tid);
        crate::arch::task::retirement_selftest::hold_after_fault_scheduler_retirement(tid);
    }
    #[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
    let _ = deferred_retired;
    // Phase 13: a trusted-init death recorded under SCHEDULER (just above, or by a
    // watchdog/heartbeat kill in an earlier pass) resets or halts here. Every
    // SCHEDULER guard of this pass is already dropped, and `yield_cpu` cannot be
    // entered with SCHEDULER held (it takes that lock itself).
    crate::cell::init_death::act_if_pending();
    retry_retired_dma_cleanup();
    // Exports whose window drain was unacknowledged stay Revoking with their
    // owner range locked; retry each pass (ADR-0033 CP1 §5.3).
    crate::task::syscall::dma_export::retry_pending();

    // Scheduler housekeeping: reap zombies, release completion slots, grants,
    // quiesced root generations and VFS leases. Each drain takes SCHEDULER, so
    // they run only when a producer flagged work (Phase 10 step 6); work that
    // is not ready yet (a zombie still current on a remote hart) re-arms the
    // flag below, so it is retried on the next pass exactly as before.
    if HOUSEKEEPING.swap(false, Ordering::AcqRel) {
        let reaped = {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.take_reapable_zombies()
            } else {
                alloc::vec::Vec::new()
            }
        };
        let mut waiter_wakes = alloc::vec::Vec::new();
        for reaped_task in reaped {
            #[cfg(feature = "p01-trace")]
            let p01_owner = crate::p01_resources::owner(&reaped_task.task);
            drop(reaped_task.task);
            #[cfg(feature = "p01-trace")]
            crate::p01_resources::dropped(p01_owner);
            waiter_wakes.extend(reaped_task.waiter_wakes);
        }
        if let Some(sched) = SCHEDULER.lock().as_mut() {
            sched.release_waiters_after_resource_drop(waiter_wakes);
        }

        // A task can die while its TIMER completion is parked. The queue is shared
        // by every thread in the cell, so task teardown alone does not free that
        // slot. Release it here, outside SCHEDULER, and clear the dead waiter before
        // another thread reuses the queue.
        let completion_releases = {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.take_pending_completion_release()
            } else {
                alloc::vec::Vec::new()
            }
        };
        for (tid, queue, slot) in completion_releases {
            let _ = queue.clear_waiter(tid);
            let _ = queue.release(slot);
        }

        // The common retirement funnel owns every task-local release. Worker exits
        // contribute one tid; a root contributes its entire quiesced generation.
        let grant_tids = {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.take_pending_grant_reap()
            } else {
                alloc::vec::Vec::new()
            }
        };
        for tid in grant_tids {
            reap_retired_task_resources(tid);
        }

        let root_retirements = {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.take_quiescent_root_retirements()
            } else {
                alloc::vec::Vec::new()
            }
        };
        for mut retirement in root_retirements {
            // Leaks counted anywhere in this window are charged to this verdict:
            // a concurrent leak elsewhere can only over-report, which fails closed.
            let leaked_before = crate::task::syscall::leaked_pages();
            let mut tally = crate::cell::retirement_verdict::RetirementTally {
                member_threads: retirement.member_tids.len(),
                remote_hart_mask: retirement.remote_hart_mask,
                ..Default::default()
            };
            for tid in retirement.member_tids.iter().copied() {
                let reap = reap_retired_task_resources(tid);
                tally.dma_deferred += usize::from(reap.dma_deferred);
                tally.pins_quarantined += reap.grants.pins_quarantined;
                tally.grantee_refs_cleared += reap.grants.grantee_refs_cleared;
                tally.writable_borrows += reap.grants.writable_borrows;
                tally.grants_reclaimed += reap.grants.grants_reclaimed;
                crate::task::syscall::release_vfs_holder_leases(tid);
                #[cfg(feature = "p01-trace")]
                if let Some(owner) = crate::p01_resources::owner_for_tid(tid) {
                    crate::task::syscall::p01_retirement_check(owner);
                    crate::memory::pin::p01_retirement_check(owner);
                }
            }
            tally.leaked_pages = crate::task::syscall::leaked_pages().wrapping_sub(leaked_before);
            // Counted after every member released its own holder leases, so only
            // obligations owed to live holders outside the incarnation remain.
            tally.vfs_leases_pending = retirement
                .member_tids
                .iter()
                .map(|&tid| crate::memory::pin::vfs_leases_pending_for_owner(tid))
                .sum();
            tally.quarantined_pages =
                crate::memory::pin::quarantined_pages_attributed(&retirement.member_tids);
            // `take_quiescent_root_retirements` removed these exact zombies while
            // holding SCHEDULER after all remote switch proofs completed. Drop their
            // stacks before exposing the CellId or quota to a new generation.
            let waiter_wakes = core::mem::take(&mut retirement.waiter_wakes);
            #[cfg(not(feature = "p01-trace"))]
            drop(retirement.zombies);
            #[cfg(feature = "p01-trace")]
            for zombie in retirement.zombies {
                let owner = crate::p01_resources::owner(&zombie);
                drop(zombie);
                crate::p01_resources::dropped(owner);
            }
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.release_waiters_after_resource_drop(waiter_wakes);
            }
            tally.shm_frames_released = crate::task::syscall::release_shm_for(
                retirement.owner.cell_id,
                retirement.owner.generation,
            );
            crate::cell::state_stash::release_retired(crate::cell::state_stash::Incarnation {
                cell: retirement.owner.cell_id,
                generation: retirement.owner.generation,
            });
            crate::cell::retirement_verdict::close(retirement.owner, tally);
            #[cfg(feature = "p01-trace")]
            if let Some(owner) =
                crate::p01_resources::owner_for_tid(retirement.owner.root_tid as usize)
            {
                crate::p01_resources::checked(owner, 7);
            }
            // The quota row is the admission gate for `reserve_next`. Keep it
            // registered until every generation-scoped resource is gone and the
            // retiring owner slot has been released; otherwise another hart can
            // reserve this CellId while the scheduler still identifies it as
            // `Retiring`.
            let owner_slot_released = SCHEDULER
                .lock()
                .as_mut()
                .is_some_and(|sched| sched.finish_root_retirement(retirement.owner));
            #[cfg(feature = "p01-trace")]
            if let Some(owner) =
                crate::p01_resources::owner_for_tid(retirement.owner.root_tid as usize)
            {
                crate::p01_resources::slot_result(owner, owner_slot_released);
            }
            if owner_slot_released {
                #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
                log::info!(
                    "[selftest] SMP-RETIREMENT: stage=owner-slot-released-before-quota cell={} generation={} root={}",
                    retirement.owner.cell_id,
                    retirement.owner.generation,
                    retirement.owner.root_tid,
                );
                crate::memory::cell_quota::deregister(types::CellId(retirement.owner.cell_id));
                #[cfg(feature = "p01-trace")]
                if let Some(owner) =
                    crate::p01_resources::owner_for_tid(retirement.owner.root_tid as usize)
                {
                    crate::p01_resources::quota_result(owner, true);
                }
                #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
                {
                    log::info!(
                        "[selftest] SMP-RETIREMENT: stage=quota-deregistered-after-owner-slot cell={} generation={} root={}",
                        retirement.owner.cell_id,
                        retirement.owner.generation,
                        retirement.owner.root_tid,
                    );
                    crate::arch::task::retirement_selftest::observe_cell_id_release(
                        retirement.owner,
                    );
                }
            } else {
                #[cfg(feature = "p01-trace")]
                if let Some(owner) =
                    crate::p01_resources::owner_for_tid(retirement.owner.root_tid as usize)
                {
                    crate::p01_resources::quota_result(owner, false);
                }
                log::error!(
                    "[task] root retirement retained CellId quota: owner slot did not match retiring owner cell={} generation={} root={}",
                    retirement.owner.cell_id,
                    retirement.owner.generation,
                    retirement.owner.root_tid,
                );
            }
        }

        let vfs_context_releases = {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.take_pending_vfs_context_release()
            } else {
                alloc::vec::Vec::new()
            }
        };
        for release in vfs_context_releases {
            crate::task::syscall::release_vfs_context_lease(release);
        }

        let vfs_holder_tids = {
            if let Some(sched) = SCHEDULER.lock().as_mut() {
                sched.take_pending_vfs_holder_release()
            } else {
                alloc::vec::Vec::new()
            }
        };
        for tid in vfs_holder_tids {
            crate::task::syscall::release_vfs_holder_leases(tid);
        }

        // Domain rows whose owner died while their revoke was unacknowledged:
        // retried every pass until their tags confirm, not only on another
        // task's exit (census F5). A row still waiting re-arms the pass.
        if crate::task::syscall::sweep_deferred_domain_grants() {
            note_housekeeping();
        }

        if SCHEDULER
            .lock()
            .as_ref()
            .is_some_and(|sched| sched.housekeeping_pending())
        {
            note_housekeeping();
        }
    }

    // Turn completion appends into scheduler wakes. An append may run in
    // interrupt context and must not take SCHEDULER, so it only raises a flag;
    // the wake happens here, the same deferral the two reaps above use. The
    // gate is one relaxed load on the overwhelmingly common empty tick.
    if completion::wakes_pending() {
        if let Some(sched) = SCHEDULER.lock().as_mut() {
            completion::deliver_pending_wakes(sched);
        }
    }

    // Exit switches stacks without returning through the syscall wrapper.
    // Drain only here, after all retirement/cleanup owner locks are released.
    #[cfg(feature = "p01-trace")]
    crate::p01_measurement::safe_point();
    #[cfg(feature = "g1-cpu-accounting")]
    crate::sched::cpu_accounting::safe_point();
}

/// Wake a parked task with a syscall result, if it is still parked.
///
/// Used by kernel objects that complete a peer's blocking call (a pipe write
/// handing bytes to a parked reader, a writer seeing EOF). Takes `SCHEDULER`, so
/// callers must release any object lock first: the lock order is
/// `SCHEDULER → object`, never the reverse.
pub(crate) fn wake_task(tid: usize, result: usize) {
    let mut scheduler = SCHEDULER.lock();
    let Some(sched) = scheduler.as_mut() else {
        return;
    };
    if let Some(task) = sched.tasks.get_mut(&tid) {
        if matches!(task.state, TaskState::Ready) {
            return;
        }
        task.state = TaskState::Ready;
        task.trap_frame.regs[10] = result as _;
        sched.push_ready(tid);
    }
}

/// Tear down `tid`'s DMA authority: its IOMMU domain, its requester entries, and
/// the frames a driver was still holding.
///
/// One implementation for both callers — cell death and a runtime
/// `pcie_driver` revoke — so a Cell that loses DMA authority mid-life ends in
/// exactly the hardware state it would reach by dying. Serialized against the
/// other teardowns: a timed-out invalidation must be retried, not interleaved.
///
/// Order: quiesce every owned PCI function (no new DMA, no interrupts), then
/// revoke and fence its IOMMU translations, then release the BDFs and the
/// acknowledged frames. Quiescing is idempotent, so a retried teardown repeats
/// it harmlessly.
///
/// Returns `false` when hardware did not acknowledge the teardown, including a
/// function that could not be shown quiescent; the caller must then keep the
/// authority (and the frames) fail-closed.
pub(crate) fn teardown_dma_authority(tid: usize) -> bool {
    let _cleanup = IOMMU_CLEANUP_SERIAL.lock();
    let owned = crate::resource_registry::bdfs_owned_by(tid);
    #[cfg(feature = "bench-counters")]
    let owned_any = !owned.is_empty();
    for bdf in owned {
        match crate::dev::pcie_ecam::quiesce_function(bdf) {
            Ok(quiesced) if quiesced.changed => log::info!(
                "[pcie] quiesced {:02x}:{:02x}.{} for tid {}: cmd=0x{:04x} bme=0 intx_disable=1 msi={} msix={}",
                (bdf >> 8) & 0xFF,
                (bdf >> 3) & 0x1F,
                bdf & 0x7,
                tid,
                quiesced.command,
                if quiesced.msi { "off" } else { "none" },
                if quiesced.msix { "masked" } else { "none" },
            ),
            Ok(_) => {}
            Err(error) => {
                log::warn!(
                    "[pcie] {:02x}:{:02x}.{} of tid {} not quiescent ({:?}); BDF and frames retained",
                    (bdf >> 8) & 0xFF,
                    (bdf >> 3) & 0x1F,
                    bdf & 0x7,
                    tid,
                    error
                );
                return false;
            }
        }
    }
    // Every owned function is quiescent (MSI off, MSI-X masked), so none can
    // fire into a vector handed to the next owner.
    #[cfg(target_arch = "x86_64")]
    crate::arch::dev::pcie_msi::release_for(tid);
    if !crate::dev::iommu::revoke_dma_for_cell(tid as u64) {
        return false;
    }
    // The holder's export windows lived in that domain: their leases now drop
    // (or lose their mapping) before any owner frame can leave quarantine.
    crate::task::syscall::dma_export::holder_domain_revoked(tid);
    crate::resource_registry::release_bdfs_for(tid);
    crate::task::syscall::release_acked_frames(tid);
    #[cfg(feature = "bench-counters")]
    if owned_any {
        crate::bench_counters::milestone("dma-retired", tid);
    }
    true
}

fn queue_retired_dma_cleanup(tid: usize) {
    // Retirement emits each TID once per funnel. A failed retry moves its one
    // entry from front to back, so requeue stays constant-time.
    #[cfg(not(feature = "p01-trace"))]
    IOMMU_CLEANUP_RETRIES.lock().push_back(tid);
    #[cfg(feature = "p01-trace")]
    {
        let owner = crate::p01_resources::owner_for_tid(tid);
        let obligation = owner.and_then(|owner| crate::p01_resources::obligation(owner, 5, 0));
        IOMMU_CLEANUP_RETRIES.lock().push_back(RetiredDmaCleanup {
            tid,
            owner,
            obligation,
        });
    }
}

fn retry_retired_dma_cleanup() {
    let retry = IOMMU_CLEANUP_RETRIES.lock().pop_front();
    #[cfg(not(feature = "p01-trace"))]
    if let Some(tid) = retry {
        if !teardown_dma_authority(tid) {
            queue_retired_dma_cleanup(tid);
        } else {
            crate::resource_registry::release_mmio_for(tid);
        }
    }
    #[cfg(feature = "p01-trace")]
    if let Some(cleanup) = retry {
        if !teardown_dma_authority(cleanup.tid) {
            if let Some(owner) = cleanup.owner {
                crate::p01_resources::retry(owner, 5);
            }
            // Preserve the immutable incarnation and token through every retry.
            IOMMU_CLEANUP_RETRIES.lock().push_back(cleanup);
        } else {
            crate::resource_registry::release_mmio_for(cleanup.tid);
            crate::p01_resources::finish(cleanup.obligation, 1);
            if let Some(owner) = cleanup.owner {
                crate::p01_resources::checked(owner, 5);
            }
        }
    }
}

/// What reaping one retired task observed, for the retirement verdict.
struct MemberReap {
    /// DMA teardown was not acknowledged; it is queued for retry.
    dma_deferred: bool,
    grants: crate::task::syscall::GrantReap,
}

fn reap_retired_task_resources(tid: usize) -> MemberReap {
    // IOFENCE/IOTLB completion is the acknowledgement required before another
    // Driver Cell may claim the BDF or pinned frames may leave quarantine.
    // On timeout both ownership and still-pinned frames remain fail-closed and
    // one deferred teardown is retried on each scheduler pass.
    //
    // First, as an export owner (ADR-0033 CP1 §6): revoke every DMA export
    // `tid` made, so the grant reap below frees only frames no holder window
    // can still reach, and quarantines the rest under their export's key.
    crate::task::syscall::dma_export::reap_owner(tid);
    let dma_deferred = !teardown_dma_authority(tid);
    if dma_deferred {
        queue_retired_dma_cleanup(tid);
    } else {
        // A window stays owned, like its BDF, until the device's DMA teardown
        // is acknowledged; the deferred retry releases it on success.
        crate::resource_registry::release_mmio_for(tid);
    }
    crate::dev::irq_wait::release_waiters_for(tid);
    let grants = crate::task::syscall::reap_grants_for_task(tid);
    crate::hypervisor::registry::reap_vms_for_task(tid);
    MemberReap {
        dma_deferred,
        grants,
    }
}

/// Allocate a stack pair and register a task. `Err` on OOM — never a panic: this
/// runs on the spawn path, and a panic there is a dead machine, not a failed call.
pub fn spawn(
    name: &str,
    cell_id: CellId,
    allowed_drivers: alloc::vec::Vec<usize>,
) -> Result<usize, ViError> {
    match SCHEDULER.lock().as_mut() {
        Some(sched) => sched.spawn(name, cell_id, allowed_drivers),
        None => Err(ViError::Unknown),
    }
}

/// Register a task around a stack pair the caller already owns. See
/// [`crate::sched::scheduler::Scheduler::spawn_with_stacks`] for why the caller allocates.
pub fn spawn_with_stacks(
    name: &str,
    cell_id: CellId,
    allowed_drivers: alloc::vec::Vec<usize>,
    kstack: stack::Stack,
    ustack: stack::Stack,
) -> Result<usize, ViError> {
    match SCHEDULER.lock().as_mut() {
        Some(sched) => sched.spawn_with_stacks(name, cell_id, allowed_drivers, kstack, ustack),
        None => Err(ViError::Unknown),
    }
}

pub fn spawn_with_arg(
    caller_id: usize,
    name: &str,
    entry: VAddr,
    arg: usize,
) -> Result<usize, ViError> {
    match SCHEDULER.lock().as_mut() {
        Some(sched) => sched.spawn_thread(caller_id, name, entry, arg),
        None => Err(ViError::Unknown),
    }
}

/// Detect whether an ELF binary is PIE (ET_DYN, e_type == 3).
///
/// Reads e_type directly from the ELF header bytes (offset 16, 2 bytes LE).
/// A return value of `true` means the ELF was compiled with
/// `-C relocation-model=pic` and must be loaded at a dynamically allocated VA.
fn elf_is_pie(data: &[u8]) -> bool {
    // ELF64 header: bytes [16..18] = e_type (u16 LE).  ET_DYN == 3.
    data.len() >= 18 && u16::from_le_bytes([data[16], data[17]]) == 3
}

pub fn spawn_from_file(path: &str) -> core::result::Result<usize, ViError> {
    // 1. Request file from VFS (Cell 3)
    let path_bytes = path.as_bytes();
    if path_bytes.len() > 250 {
        return Err(ViError::InvalidInput);
    }

    let mut req = [0u8; 256];
    req[0] = 1; // OpCode: GetFile
    req[1] = path_bytes.len() as u8;
    req[2..2 + path_bytes.len()].copy_from_slice(path_bytes);

    // Caller ID? We are in kernel context.
    // We impersonate the current task? Or use Kernel ID (0)?
    // Protocol expects Sender ID.
    // If we use `ipc_send` directly, we can specify caller.
    // VFS replies to Sender.
    // If we say Sender is CurrentTask, VFS replies to CurrentTask.
    // CurrentTask needs to be in Recv state?
    // BUT we are in a Syscall Handler! CurrentTask is Running.
    // We cannot block in Syscall Handler waiting for IPC easily unless we yield/sleep.
    // BUT syscalls must be atomic-ish or handle blocking.
    // If we block, we set state to Waiting/Recv?

    // Simpler approach: Use "Synchronous" IPC via busy-wait or special kernel privilege?
    // Or just spawn from memory in `init` and avoid this complexity in kernel.
    // But `shell` needs it.

    // Let's rely on standard IPC mechanisms.
    // We need to send, then wait for reply.
    // This is hard inside a syscall handler without async/await or state machine.

    // Hack: Busy loop/Yield loop waiting for VFS reply?
    // Since VFS is on another core or time-sliced, we must yield.

    // For now, let's implement a blocking IPC exchange using polling?
    // We can't easily pollute the task state machine.

    log::error!("spawn_from_file: Kernel-side VFS request not fully implemented due to blocking complexity.");
    Err(ViError::NotSupported)
}

pub fn current_task_id() -> usize {
    crate::sched::hart_local::ready::current_task_id_for(crate::sched::hart_local::current_hart_id())
}

pub fn has_ready_tasks() -> bool {
    crate::sched::hart_local::ready::total_ready_count() > 0
}

pub fn ipc_send(
    caller_id: usize,
    target_id: usize,
    msg_ptr: VAddr,
    msg_len: usize,
) -> core::result::Result<usize, IpcSendError> {
    if msg_len > crate::ipc::ipc_wire::MAX_IPC_WIRE_PAYLOAD {
        return Err(IpcSendError::Backpressure);
    }

    let caller_view = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(IpcSendError::TargetGone)?;
        if ipc_target_gone(sched, target_id) {
            log::debug!("IPC: Target Task {} not found (cell exited)", target_id);
            return Err(IpcSendError::TargetGone);
        }
        if paused_target_rejects(sched, caller_id, target_id) {
            return Err(IpcSendError::Backpressure);
        }
        copy_glue::TaskCopyView::of(
            sched
                .tasks
                .get(&caller_id)
                .ok_or(IpcSendError::TargetGone)?,
        )
    };
    let msg_bytes = caller_view
        .read_bytes(msg_ptr, msg_len)
        .map_err(|_| IpcSendError::Backpressure)?;
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::ipc_bytes(msg_bytes.len());
    ipc_send_kernel(caller_id, target_id, &msg_bytes)
}

/// Send kernel-owned IPC bytes with the same receiver-mask and delivery-token
/// semantics as [`ipc_send`], which copies the caller's bytes and calls this.
///
/// It must never use the fire-and-forget path: a masked nested receive must
/// retain its target restriction.
pub fn ipc_send_kernel(
    caller_id: usize,
    target_id: usize,
    msg: &[u8],
) -> core::result::Result<usize, IpcSendError> {
    if msg.len() > crate::ipc::ipc_wire::MAX_IPC_WIRE_PAYLOAD {
        return Err(IpcSendError::Backpressure);
    }

    #[cfg(feature = "p01-trace")]
    let heap_owner;
    let header = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(IpcSendError::TargetGone)?;
        if ipc_target_gone(sched, target_id) || !sched.tasks.contains_key(&caller_id) {
            return Err(IpcSendError::TargetGone);
        }
        if paused_target_rejects(sched, caller_id, target_id) {
            return Err(IpcSendError::Backpressure);
        }
        let (sender_cell_id, sender_generation) = sender_context(sched, caller_id);
        #[cfg(feature = "p01-trace")]
        {
            heap_owner = (sender_cell_id != 0 && sender_generation != 0)
                .then(|| crate::p01_resources::owner(sched.tasks.get(&caller_id).unwrap()));
        }
        crate::ipc::ipc_wire::IpcWireHeader {
            sender_tid: caller_id,
            sender_cell_id,
            sender_generation,
            delivery_id: next_delivery_id(),
            async_op: 0,
        }
    };
    let wire_msg = crate::ipc::ipc_wire::IpcWireMessage::try_new(header, msg)
        .map_err(|_| IpcSendError::Backpressure)?;
    #[cfg(feature = "p01-trace")]
    if let Some(owner) = heap_owner {
        wire_msg.observe_owner(owner, crate::memory::heap::p01_owner::SENDER_WIRE);
    } else {
        wire_msg.observe_synthetic();
    }

    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(IpcSendError::TargetGone)?;
    if ipc_target_gone(sched, target_id) {
        return Err(IpcSendError::TargetGone);
    }
    if paused_target_rejects(sched, caller_id, target_id) {
        return Err(IpcSendError::Backpressure);
    }

    let target = sched
        .tasks
        .get(&target_id)
        .ok_or(IpcSendError::TargetGone)?;
    let recv_eligible = matches!(target.state, TaskState::Blocked { wait: WaitKind::Recv { mask, .. }, .. } if mask == 0 || mask == caller_id);
    let target_frozen = matches!(target.state, TaskState::Frozen { .. });
    // A target already blocked sending *to this caller* is the other half of a
    // send cycle: it cannot consume this message until its own send completes,
    // and its own send completes only once this caller returns to receive.
    // Parking here would strand both cells (the board's Net Cell and its NIC
    // driver did exactly that, and the liveness heartbeat then killed the Net
    // Cell every ~1 s). Queueing is safe instead: the message is owned by the
    // receiver's mailbox, so the target consumes it as soon as its own send
    // lands, and the caller is free to receive the message that unblocks it.
    let target_sending_to_caller = matches!(target.state, TaskState::Blocked { wait: WaitKind::Send { target: peer, .. }, .. } if peer == caller_id);
    let wake = if let Some(target) = sched.tasks.get_mut(&target_id) {
        queue_wire_msg(target, wire_msg, tcb::HOTSWAP_MSG_QUEUE_DEPTH)
            .map_err(|_| IpcSendError::Backpressure)?;
        wake_after_ipc_publish(target, recv_eligible)
    } else {
        IpcWakeCause::None
    };
    if target_frozen || target_sending_to_caller {
        return Ok(0);
    }
    if wake.made_runnable() {
        let prio = sched.push_ready(target_id);
        sched.pend_preempt_if_needed(prio);
    }
    if wake == IpcWakeCause::Recv {
        return Ok(0);
    }
    arm_ipc_block_handoff(caller_id);
    if let Some(caller) = sched.tasks.get_mut(&caller_id) {
        caller.reply_value = None;
        caller.state = TaskState::Blocked {
            wait: WaitKind::Send {
                target: target_id,
                delivery_id: header.delivery_id,
            },
            deadline: None,
        };
    }
    Ok(1)
}

/// Post a message to `target_id` without blocking the caller.
///
/// Queues an owned message and wakes a compatible `Recv` or a
/// `WaitCompletion(NET_RX)` target immediately. Other busy targets retain the
/// queued message until their next receive call. The mailbox is bounded by
/// `HOTSWAP_MSG_QUEUE_DEPTH`.
///
/// Never puts the caller in `Sending` state — the caller always continues.
/// Returns `Ok(())` if delivered or queued, `Err(())` if target is gone or queue full.
///
/// Used by `GpuFlush`/`GpuCursor` syscall handlers to fire-and-forget
/// IPC to the GPU Driver Cell without parking the compositor.
pub fn ipc_post_nonblock(
    sender_id: usize,
    target_id: usize,
    msg: &[u8],
) -> core::result::Result<(), ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        // A retiring or terminated target never receives: fail now, as a
        // blocking send does, instead of queueing a message nobody takes.
        if ipc_target_gone(sched, target_id) {
            return Err(());
        }
        if paused_target_rejects(sched, sender_id, target_id) {
            return Err(());
        }
        if msg.len() > crate::ipc::ipc_wire::MAX_IPC_WIRE_PAYLOAD {
            return Err(());
        }

        // Known pre-existing contract gap (2026-07-31 Recv buffer-pinning audit):
        // unlike ipc_send/ipc_try_send, this path intentionally preserves its
        // current behavior of matching any Recv without consulting the mask.
        let recv_eligible = sched.tasks.get(&target_id).is_some_and(|t| {
            matches!(
                t.state,
                TaskState::Blocked {
                    wait: WaitKind::Recv { .. },
                    ..
                }
            )
        });

        let (sender_cell_id, sender_generation) = sender_context(sched, sender_id);
        let header = crate::ipc::ipc_wire::IpcWireHeader {
            sender_tid: sender_id,
            sender_cell_id,
            sender_generation,
            delivery_id: next_delivery_id(),
            async_op: 0,
        };
        let wire = crate::ipc::ipc_wire::IpcWireMessage::try_new(header, msg)?;
        #[cfg(feature = "p01-trace")]
        if let Some(sender) = sched
            .tasks
            .get(&sender_id)
            .filter(|_| sender_cell_id != 0 && sender_generation != 0)
        {
            wire.observe_owner(
                crate::p01_resources::owner(sender),
                crate::memory::heap::p01_owner::SENDER_WIRE,
            );
        } else {
            wire.observe_synthetic();
        }
        let sender_is_task = sched.tasks.contains_key(&sender_id);
        let wake = if let Some(target) = sched.tasks.get_mut(&target_id) {
            // One Cell may hold only part of a target's mailbox with posts, so a
            // client posting faster than its service drains (damage every
            // frame) cannot crowd out other clients' requests and events.
            // Kernel senders (UART relay, power button; no task) keep the whole
            // queue.
            if sender_is_task
                && target
                    .pending_msgs
                    .iter()
                    .filter(|queued| queued.sender_tid == sender_id)
                    .count()
                    >= tcb::POST_QUOTA_PER_SENDER
            {
                return Err(());
            }
            queue_wire_msg(target, wire, tcb::HOTSWAP_MSG_QUEUE_DEPTH)?;
            // Caller-context assignment is deferred to the dequeue/commit path
            // so the request generation advances exactly once.
            wake_after_ipc_publish(target, recv_eligible)
        } else {
            IpcWakeCause::None
        };
        if wake.made_runnable() {
            let prio = sched.push_ready(target_id);
            sched.pend_preempt_if_needed(prio);
        }
        return Ok(());
    }
    Err(())
}

pub fn ipc_recv(
    caller_id: usize,
    mask: usize,
    buf_ptr: VAddr,
    buf_len: usize,
) -> core::result::Result<usize, ()> {
    // Peek: snapshot the wire payload fallibly WITHOUT removing the record.
    // Copy-out happens outside the scheduler lock; only a successful copy
    // commits removal + sender wake. A failed copy retains the message and
    // the sender token unchanged.
    let (sender_id, header, snapshot, receiver_view) = {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(())?;
        let slot = sched
            .tasks
            .get(&caller_id)
            .ok_or(())?
            .pending_msgs
            .iter()
            .position(|message| {
                message.wire.is_some() && (mask == 0 || message.sender_tid == mask)
            });
        let Some(index) = slot else {
            arm_ipc_block_handoff(caller_id);
            if let Some(caller) = sched.tasks.get_mut(&caller_id) {
                caller.state = TaskState::Blocked {
                    wait: WaitKind::Recv {
                        mask,
                        buf_ptr,
                        buf_len,
                    },
                    deadline: None,
                };
            }
            return Ok(0);
        };
        let receiver_task = sched.tasks.get_mut(&caller_id).ok_or(())?;
        let record = &receiver_task.pending_msgs.as_slice()[index];
        let sender_id = record.sender_tid;
        let wire = record.wire.as_ref().ok_or(())?;
        let header = wire.header;
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            Some(crate::p01_resources::owner(receiver_task)),
            crate::memory::heap::p01_owner::RECEIVE_SCRATCH,
        );
        let snapshot = wire.try_clone()?;
        let receiver_view = copy_glue::TaskCopyView::of(receiver_task);
        async_ipc::dispatch(sched, caller_id, header);
        (sender_id, header, snapshot, receiver_view)
    };

    // Copy-out: failure leaves queue and sender token untouched.
    let copy_len = core::cmp::min(snapshot.len(), buf_len);
    if copy_len > 0 {
        receiver_view.write_bytes(buf_ptr, &snapshot.as_slice()[..copy_len])?;
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::ipc_bytes(copy_len);
    }

    // Commit: remove by delivery token, wake sender, assign caller context.
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(())?;
    if let Some(receiver_task) = sched.tasks.get_mut(&caller_id) {
        let pos = receiver_task.pending_msgs.iter().position(|message| {
            message.sender_tid == sender_id
                && message
                    .wire
                    .as_ref()
                    .is_some_and(|w| w.header.delivery_id == header.delivery_id)
        });
        if let Some(index) = pos {
            receiver_task.pending_msgs.remove(index);
        }
        let held = receiver_task.set_received_request_context(
            sender_id,
            header.sender_cell_id,
            header.sender_generation,
            header.async_op,
        );
        if let Some(token) = held {
            sched.async_index.note_holder(caller_id, token);
        }
    }
    wake_sender_token(sched, sender_id, caller_id, header);
    Ok(sender_id)
}

pub fn ipc_try_recv(
    caller_id: usize,
    mask: usize,
    buf_ptr: VAddr,
    buf_len: usize,
) -> core::result::Result<usize, ()> {
    // Peek without removal: identical commit contract to ipc_recv, but
    // non-blocking — no matching record yields Ok(0).
    let (sender_id, header, snapshot, receiver_view) = {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().ok_or(())?;
        let slot = sched
            .tasks
            .get(&caller_id)
            .ok_or(())?
            .pending_msgs
            .iter()
            .position(|message| {
                message.wire.is_some() && (mask == 0 || message.sender_tid == mask)
            });
        let Some(index) = slot else {
            return Ok(0);
        };
        let receiver_task = sched.tasks.get(&caller_id).ok_or(())?;
        let record = &receiver_task.pending_msgs.as_slice()[index];
        let sender_id = record.sender_tid;
        let wire = record.wire.as_ref().ok_or(())?;
        let header = wire.header;
        #[cfg(feature = "p01-trace")]
        let _semantic = crate::memory::heap::p01_owner::creation(
            Some(crate::p01_resources::owner(receiver_task)),
            crate::memory::heap::p01_owner::RECEIVE_SCRATCH,
        );
        let snapshot = wire.try_clone()?;
        let receiver_view = copy_glue::TaskCopyView::of(receiver_task);
        async_ipc::dispatch(sched, caller_id, header);
        (sender_id, header, snapshot, receiver_view)
    };

    let copy_len = core::cmp::min(snapshot.len(), buf_len);
    if copy_len > 0 {
        receiver_view.write_bytes(buf_ptr, &snapshot.as_slice()[..copy_len])?;
        #[cfg(feature = "bench-counters")]
        crate::bench_counters::ipc_bytes(copy_len);
    }

    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(())?;
    if let Some(receiver_task) = sched.tasks.get_mut(&caller_id) {
        let pos = receiver_task.pending_msgs.iter().position(|message| {
            message.sender_tid == sender_id
                && message
                    .wire
                    .as_ref()
                    .is_some_and(|w| w.header.delivery_id == header.delivery_id)
        });
        if let Some(index) = pos {
            receiver_task.pending_msgs.remove(index);
        }
        let held = receiver_task.set_received_request_context(
            sender_id,
            header.sender_cell_id,
            header.sender_generation,
            header.async_op,
        );
        if let Some(token) = held {
            sched.async_index.note_holder(caller_id, token);
        }
    }
    wake_sender_token(sched, sender_id, caller_id, header);
    Ok(sender_id)
}

/// Ready a sender blocked on a delivered message only when the live task
/// still matches the wire header identity AND the delivery token of the
/// consumed message. A reused TID or a newer blocked send from the same
/// sender must never be woken by an older message's consumption.
pub(crate) fn wake_sender_token(
    sched: &mut Scheduler,
    sender_id: usize,
    target_id: usize,
    header: crate::ipc::ipc_wire::IpcWireHeader,
) {
    let Some(sender_task) = sched.tasks.get(&sender_id) else {
        return;
    };
    let identity_matches = sender_task.cell_id.0 == header.sender_cell_id
        && sender_task.cell_generation == header.sender_generation;
    let token_matches = matches!(
        sender_task.state,
        TaskState::Blocked { wait: WaitKind::Send { target, delivery_id }, .. }
            if target == target_id && delivery_id == header.delivery_id
    );
    if identity_matches && token_matches {
        if let Some(sender_task) = sched.tasks.get_mut(&sender_id) {
            sender_task.state = TaskState::Ready;
        }
        // The sender may be a RealTime cell: consuming its message has to request
        // preemption for the sender's actual target hart, exactly as the other wake
        // paths do. Dropping `push_ready`'s priority here left an RT sender waiting
        // for the next timer tick.
        let priority = sched.push_ready(sender_id);
        sched.pend_preempt_if_needed(priority);
    }
}
fn is_trusted_input_sender(caller_id: usize) -> bool {
    let input_tid =
        crate::dev::driver_cell::INPUT_CELL_TID.load(core::sync::atomic::Ordering::Acquire);
    let compositor_tid =
        crate::cell::service_registry::lookup(abi::syscall::service::COMPOSITOR).unwrap_or(0);
    (caller_id == input_tid && input_tid != 0)
        || (caller_id == compositor_tid && compositor_tid != 0)
}

/// Report a refused `TrySend` admission (`ipc-trace` images).
///
/// A refused admission is how a bounded caller (`service_call_bounded` retrying
/// `sys_try_send` until its deadline) learns the request never reached the
/// service. The target's state is what separates a healthy service that idles in
/// a completion wait from one wedged behind a masked receive, so it is printed
/// with the reason. Rate-limited per `(caller, target)` pair — one hot pair must
/// not consume another pair's budget, and the admission loop retries thousands
/// of times per deadline while the console is a 115200-baud UART.
#[cfg(feature = "ipc-trace")]
fn trace_try_send_refusal(caller_id: usize, target_id: usize, reason: &str, state: &str) {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Distinct `(caller, target)` pairs tracked before falling back to one
    /// shared counter.
    const PAIRS: usize = 6;
    /// High 32 bits: refusals seen. Low 32 bits: `caller<<16 | target`; 0 is
    /// unclaimed, so a tid of 0 must never be a trace key (it never is: task ids
    /// start at 1).
    const EMPTY: AtomicU64 = AtomicU64::new(0);
    static SLOTS: [AtomicU64; PAIRS] = [EMPTY; PAIRS];

    let key = (((caller_id as u64) & 0xffff) << 16) | ((target_id as u64) & 0xffff);
    let mut count = 0u64;
    let mut tracked = false;
    for slot in SLOTS.iter() {
        let current = slot.load(Ordering::Relaxed);
        if current & 0xffff_ffff == key {
            count = (current >> 32) + 1;
            slot.store((count << 32) | key, Ordering::Relaxed);
            tracked = true;
            break;
        }
    }
    if !tracked {
        for slot in SLOTS.iter() {
            if slot
                .compare_exchange(0, (1u64 << 32) | key, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                count = 1;
                tracked = true;
                break;
            }
        }
    }
    if !tracked {
        static OVERFLOW: AtomicU64 = AtomicU64::new(0);
        count = OVERFLOW.fetch_add(1, Ordering::Relaxed) + 1;
    }
    if count <= 4 || count % 4096 == 0 {
        log::info!(
            "[ipc-trace] try_send refused n={} caller={} target={} reason={} state={}",
            count,
            caller_id,
            target_id,
            reason,
            state
        );
    }
}

/// No-op without `ipc-trace`.
#[cfg(not(feature = "ipc-trace"))]
fn trace_try_send_refusal(_caller_id: usize, _target_id: usize, _reason: &str, _state: &str) {}

pub fn ipc_try_send(
    caller_id: usize,
    target_id: usize,
    msg_ptr: VAddr,
    msg_len: usize,
) -> core::result::Result<(), ()> {
    enum TrySendAction {
        Publish(copy_glue::TaskCopyView),
        /// Refused before delivery: the reason, and the target's state.
        Reject(&'static str, &'static str),
    }

    let action = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(())?;
        if !sched.tasks.contains_key(&target_id) {
            TrySendAction::Reject("target-missing", "gone")
        } else if paused_target_rejects(sched, caller_id, target_id) {
            TrySendAction::Reject("paused", "paused")
        } else if msg_len > crate::ipc::ipc_wire::MAX_IPC_WIRE_PAYLOAD {
            TrySendAction::Reject("payload-too-big", "n/a")
        } else {
            let target_ready = sched
                .tasks
                .get(&target_id)
                .map(|t| matches!(t.state, TaskState::Blocked { wait: WaitKind::Recv { mask, .. }, .. } if mask == 0 || mask == caller_id))
                .unwrap_or(false);
            let is_same_cell_wake_candidate = sched
                .tasks
                .get(&caller_id)
                .zip(sched.tasks.get(&target_id))
                .map(|(c, t)| c.cell_id == t.cell_id && msg_len <= 8)
                .unwrap_or(false);
            if target_ready || is_trusted_input_sender(caller_id) || is_same_cell_wake_candidate {
                let caller_view = sched
                    .tasks
                    .get(&caller_id)
                    .map(copy_glue::TaskCopyView::of)
                    .ok_or(())?;
                TrySendAction::Publish(caller_view)
            } else {
                let state = sched
                    .tasks
                    .get(&target_id)
                    .map(|t| tcb::task_state_name(&t.state))
                    .unwrap_or("gone");
                TrySendAction::Reject("not-recv", state)
            }
        }
    };

    let caller_view = match action {
        TrySendAction::Publish(caller_view) => caller_view,
        TrySendAction::Reject(reason, state) => {
            trace_try_send_refusal(caller_id, target_id, reason, state);
            return Err(());
        }
    };
    let msg_bytes = caller_view.read_bytes(msg_ptr, msg_len).map_err(|_| ())?;
    #[cfg(feature = "bench-counters")]
    crate::bench_counters::ipc_bytes(msg_bytes.len());
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(())?;
    // Revalidate under the second lock: phase-1 target state is stale once
    // released. A sender outside the trusted input route that raced a target
    // leaving Recv must be rejected; trusted input-route senders queue regardless.
    if !sched.tasks.contains_key(&target_id) || paused_target_rejects(sched, caller_id, target_id) {
        let reason = if sched.tasks.contains_key(&target_id) {
            "paused-revalidate"
        } else {
            "target-missing-revalidate"
        };
        drop(guard);
        trace_try_send_refusal(caller_id, target_id, reason, "n/a");
        return Err(());
    }
    let recv_eligible = sched
        .tasks
        .get(&target_id)
        .map(|t| matches!(t.state, TaskState::Blocked { wait: WaitKind::Recv { mask, .. }, .. } if mask == 0 || mask == caller_id))
        .unwrap_or(false);
    let is_reactor_wake = match (sched.tasks.get(&caller_id), sched.tasks.get(&target_id)) {
        (Some(c), Some(t)) => {
            c.cell_id == t.cell_id
                && msg_bytes.len() <= 8
                && msg_bytes.first() == Some(&abi::kernel_ipc::REACTOR_WAKE_OPCODE)
        }
        _ => false,
    };
    if !recv_eligible && !is_trusted_input_sender(caller_id) && !is_reactor_wake {
        let state = sched
            .tasks
            .get(&target_id)
            .map(|t| tcb::task_state_name(&t.state))
            .unwrap_or("gone");
        drop(guard);
        trace_try_send_refusal(caller_id, target_id, "not-recv-revalidate", state);
        return Err(());
    }
    if is_reactor_wake {
        if let Some(target) = sched.tasks.get(&target_id) {
            if target
                .pending_msgs
                .iter()
                .any(|m| m.payload().first() == Some(&abi::kernel_ipc::REACTOR_WAKE_OPCODE))
            {
                return Ok(());
            }
        }
    }
    let (sender_cell_id, sender_generation) = sender_context(sched, caller_id);
    let header = crate::ipc::ipc_wire::IpcWireHeader {
        sender_tid: caller_id,
        sender_cell_id,
        sender_generation,
        delivery_id: next_delivery_id(),
        async_op: 0,
    };
    let wire = crate::ipc::ipc_wire::IpcWireMessage::try_new(header, &msg_bytes)?;
    #[cfg(feature = "p01-trace")]
    if let Some(sender) = sched
        .tasks
        .get(&caller_id)
        .filter(|_| sender_cell_id != 0 && sender_generation != 0)
    {
        wire.observe_owner(
            crate::p01_resources::owner(sender),
            crate::memory::heap::p01_owner::SENDER_WIRE,
        );
    } else {
        wire.observe_synthetic();
    }
    let wake = if let Some(target) = sched.tasks.get_mut(&target_id) {
        queue_wire_msg(target, wire, tcb::INPUT_EVENT_QUEUE_DEPTH)?;
        // Caller-context assignment is deferred to the dequeue/commit path so
        // the request generation advances exactly once per message.
        wake_after_ipc_publish(target, recv_eligible)
    } else {
        IpcWakeCause::None
    };
    if wake.made_runnable() {
        let prio = sched.push_ready(target_id);
        sched.pend_preempt_if_needed(prio);
    }
    Ok(())
}

/// Get scheduler statistics
pub fn scheduler_stats() -> (usize, usize) {
    let task_count = if let Some(sched) = SCHEDULER.lock().as_ref() {
        sched.tasks.len()
    } else {
        0
    };
    (
        task_count,
        crate::sched::hart_local::ready::total_ready_count(),
    )
}

/// 8 KB circular ring buffer for `ReadLog = 237` syscall.
///
/// `print_user_log` writes here in addition to UART so the fb-console cell can
/// mirror kernel user-log output to the HDMI screen without touching serial I/O.
/// Capacity is a power-of-two so head/tail wrapping uses bitwise AND.
const LOG_RING_CAP: usize = 8192;

struct LogRing {
    buf: [u8; LOG_RING_CAP],
    head: usize, // next byte to write (producer)
    tail: usize, // next byte to read  (consumer)
}

impl LogRing {
    const fn new() -> Self {
        Self {
            buf: [0u8; LOG_RING_CAP],
            head: 0,
            tail: 0,
        }
    }

    /// Append bytes, overwriting oldest data when full.
    fn push(&mut self, data: &[u8]) {
        for &b in data {
            self.buf[self.head & (LOG_RING_CAP - 1)] = b;
            self.head = self.head.wrapping_add(1);
            // If we lapped the tail, advance it to discard the oldest byte.
            if self.head.wrapping_sub(self.tail) > LOG_RING_CAP {
                self.tail = self.tail.wrapping_add(1);
            }
        }
    }

    /// Drain up to `max` bytes into `out`. Returns number of bytes copied.
    fn drain(&mut self, out: &mut [u8]) -> usize {
        let max = out.len();
        let avail = self.head.wrapping_sub(self.tail).min(LOG_RING_CAP);
        let n = avail.min(max);
        for (i, slot) in out.iter_mut().enumerate().take(n) {
            *slot = self.buf[(self.tail.wrapping_add(i)) & (LOG_RING_CAP - 1)];
        }
        self.tail = self.tail.wrapping_add(n);
        n
    }
}

static LOG_RING: crate::sync::Spinlock<LogRing> =
    crate::sync::Spinlock::ranked(LogRing::new(), crate::sync::rank::LOG_RING);

/// Drain up to `buf.len()` bytes from the user-log ring into `buf`.
/// Called by the `ReadLog = 237` syscall handler.
pub fn read_log_ring(buf: &mut [u8]) -> usize {
    LOG_RING.lock().drain(buf)
}

/// Write one Cell record to the console, owned by `writer`.
///
/// `writer` is the calling task's id, handed to
/// [`crate::dev::uart::write_user_record`](crate::dev::uart::write_user_record)
/// as the console line's owner. The guest's shell echo is the case that matters:
/// it arrives one `sys_log` per keypress from the hypervisor, so a diagnostic
/// from another cell used to be spliced into the middle of the command being
/// typed.
pub fn print_user_log(msg: &str, writer: usize) {
    // USER stdout must ALWAYS reach the console, independent of the kernel log
    // level — it is cell application output, not kernel debug chatter. Writing
    // straight to the UART (not via info!) lets us quiet boot-time kernel Info
    // spam without also silencing the shell prompt / cell output.
    //
    // Emit the raw bytes verbatim (no trim, no synthesised newline) so the
    // distinction between print() and println() at the ostd layer is preserved:
    // print() concatenates inline; println() ends the line. The "USER: " prefix
    // is injected only at each line start, keeping log scrapers/tests matching
    // while making interactive echo behave like a real terminal.
    // Mirror raw message bytes to the ring buffer so the fb-console cell can read
    // them via ReadLog without reconstructing the UART prefix logic.
    LOG_RING.lock().push(msg.as_bytes());

    crate::dev::uart::write_user_record(writer, msg);
}

/// Spawns a synthetic task for testing User Mode without filesystem
pub fn spawn_synthetic(
    name: &str,
    cell_id: CellId,
    entry: VAddr,
) -> core::result::Result<usize, ViError> {
    // use hal::paging::PAGE_SIZE;

    // 1. Allocate the stacks, then register the task around them.
    //
    // This used to call `spawn()` (which allocated a pair) and then allocate a
    // SECOND pair inside the scheduler-lock section below, overwriting the first.
    // Beyond the wasted pair, the failure ordering was wrong: the task was already
    // inserted and runnable when the second allocation could still fail, so an OOM
    // there returned `Err` and left a half-built task in the scheduler forever.
    let stack_pages = stack_pages_for(name);
    let kstack = stack::Stack::new_kernel(stack_pages)?;
    let ustack = stack::Stack::new_user(stack_pages)?;
    let kstack_top = kstack.top;
    let user_stack_top = ustack.top;
    let tid = spawn_with_stacks(name, cell_id, alloc::vec::Vec::new(), kstack, ustack)?;

    // 2. Map Code Page at 'entry'
    {
        let frame = crate::memory::frame::allocate_frame().ok_or(ViError::OutOfMemory)?;

        // Write code to frame (Physical access)
        // Code: ecall (0x00000073) + loop (j .)
        // Write code to frame (Physical access)
        unsafe {
            let base = frame as *mut u8;

            // 1. lui a0, 0x1      => a0 = 0x1000 (Page Base)
            *(base as *mut u32) = 0x00001537;

            // 2. addi a0, a0, 32  => a0 = 0x1020 (String Address)
            *(base.add(4) as *mut u32) = 0x02050513;

            // 3. li a1, 21        => a1 = 21 (Length)
            *(base.add(8) as *mut u32) = 0x01500593;

            // 4. li a7, 11        => a7 = 11 (Syscall::Log)
            *(base.add(12) as *mut u32) = 0x00b00893;

            // 5. ecall
            *(base.add(16) as *mut u32) = 0x00000073;

            // 6. j .              => Loop forever
            *(base.add(20) as *mut u32) = 0x0000006F;

            // Data: "Hello from Userspace!" at offset 32
            let msg = b"Hello from Userspace!";
            core::ptr::copy_nonoverlapping(msg.as_ptr(), base.add(32), msg.len());
        }

        // Permissions: VALID | READ | EXECUTE | USER
        // Note: Generic PageFlags bits might not match RISC-V perfectly if not verified,
        // but we verified they DO match in hal implementation.
        // Or we use hal::PageFlags directly.
        use crate::memory::paging::Flags;
        // 1=V, 2=R, 8=X, 16=U ? No.
        // Check lib.rs: V=1, R=2, W=4, X=8, U=16
        // We want V, R, X, U. 1|2|8|16 = 27 (0x1B).

        let flags = Flags::from_bits(
            Flags::VALID
                | Flags::READ
                | Flags::WRITE
                | Flags::EXECUTE
                | Flags::USER
                | Flags::ACCESSED
                | Flags::DIRTY,
        );

        let mut batch = crate::memory::frame_run::Batch::new();
        let mapped = crate::memory::frame::with(|allocator| {
            crate::memory::paging::map_page(allocator, &mut batch, entry, frame, flags)
        });
        let flushed = batch.complete();
        mapped
            .ok_or(ViError::OutOfMemory)?
            .map_err(|_| ViError::OutOfMemory)?;
        if !flushed.is_confirmed() {
            return Err(ViError::OutOfMemory);
        }
    }

    // 3. Update Task Context (Copied from spawn_from_file)
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&tid) {
            task.trap_frame.sepc = entry as _;
            task.trap_frame.sstatus = 0x20; // User Mode (SPIE=1, SPP=0)

            // Stacks are already owned by the task (step 1) — nothing to allocate
            // here, so this section can no longer fail partway and abandon a task.
            let tf_ptr = kstack_top - TRAP_FRAME_SIZE;
            task.trap_frame.regs[2] = user_stack_top as _; // User SP

            unsafe {
                let tf_dest = &mut *(tf_ptr as *mut crate::hal::arch::ViTrapFrame);
                *tf_dest = task.trap_frame;
            }

            task.context.sp = tf_ptr as _;
            #[cfg(target_arch = "riscv64")]
            {
                task.context.ra = __trap_exit as *const () as usize;
                task.context.sstatus = 0x40120;
            } // SUM=1
            #[cfg(target_arch = "riscv32")]
            {
                task.context.ra = __trap_exit as *const () as u32;
                task.context.sstatus = 0x120_u32;
            } // SPP=1, SPIE=1
            #[cfg(target_arch = "aarch64")]
            {
                task.context.x30 = __trap_exit as *const () as u64;
                task.context.sp_el0 = user_stack_top as u64;
            }
            #[cfg(target_arch = "x86_64")]
            {
                task.context.rip = __trap_exit as *const () as u64;
            }

            info!(
                "Spawned Synthetic task '{}' (ID {}) at entry 0x{:X}",
                name, tid, entry
            );
        }
    }

    Ok(tid)
}
