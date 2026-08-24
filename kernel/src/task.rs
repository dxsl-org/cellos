// This module's Result<_, ()> IPC/task-management functions predate a proper
// kernel error-type design; redesigning ~20 signatures (and every call site
// matching Err(())) is out of scope for a lint cleanup — tracked separately.
#![allow(clippy::result_unit_err)]

pub mod cap;
pub mod completion;
pub mod completion_selftest;
pub mod completion_wait;
pub(crate) mod copy_glue;
pub mod dir_inherit;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
pub(crate) mod domain_switch;
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) mod domain_switch_tests;
mod elf_prepare;
pub mod hart_local;
pub mod manifest_v2_selftest;
pub mod net_rx_selftest;
pub mod p_trust_selftest;
pub mod smp;
pub mod syscall;
pub mod tcb;
pub mod thread_cap_selftest;
pub mod thread_quota_selftest;
pub mod thread_user_entry_selftest;
pub use elf_prepare::{prepare_elf_task, PreparedElfTask};
pub mod launch;
pub use launch::{
    publish_prepared, CallerLaunchAuthority, LaunchRoutes, StagedMeasurement, TaskLaunchState,
};
pub mod ipc_wire;
#[cfg(test)]
mod ipc_wire_tests;
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub mod ipc_wire_selftest;
pub use tcb::Task;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub mod context_handoff_selftest;
pub mod drivers;
pub mod ipc_guardrail_selftest;
pub mod ipc_pending_selftest;
pub mod ipc_test;
pub mod pending_mailbox;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub mod retirement_selftest;
pub mod scheduler;
pub mod stack;
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub mod stack_overflow_probe;
#[cfg(feature = "test-hooks")]
pub mod user_hello;
pub mod user_out;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
pub(crate) mod user_copy;
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    target_arch = "riscv64"
))]
pub(crate) mod user_copy_tests;
#[cfg(feature = "test-hooks")]
pub mod vfs_lifecycle_selftest;
pub mod waker;

#[cfg(test)]
mod tests;

use crate::sync::Spinlock;
use alloc::string::String;
#[cfg(feature = "test-hooks")]
use core::sync::atomic::{AtomicU8, Ordering};
use log::info;
use scheduler::Scheduler;
/// Conservative fallback for every unmeasured or risk-sensitive task path.
/// Measured Phase 07 paths use the post-reactor table below; unknown paths keep
/// this historical 256 KiB allocation.
pub const STACK_PAGES: usize = 64;
/// Post-reactor floor: twice the largest measured peak, rounded up to 64 KiB.
const MEASURED_STACK_PAGES: usize = 16;
const TRAP_FRAME_SIZE: usize = core::mem::size_of::<crate::hal::arch::ViTrapFrame>();
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
    let kernel_stack = task
        .kernel_stack
        .as_ref()
        .expect("prime_user_mode_entry requires a kernel stack");
    let user_stack = task
        .user_stack
        .as_ref()
        .expect("prime_user_mode_entry requires a user stack");
    let tf_ptr = kernel_stack.top - TRAP_FRAME_SIZE;
    let user_stack_top = user_stack.top;
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
use types::*;

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
    let owned = pending_mailbox::PendingMsgData::try_copy(data, target.cell_id.0 as usize)?;
    target.pending_msgs.try_push(tcb::PendingMsg {
        sender_tid,
        data: owned,
        wire: None,
        enqueued_tick: system_ticks() as u64,
    })
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
    message: ipc_wire::IpcWireMessage,
    max_depth: usize,
) -> core::result::Result<(), ()> {
    if target.pending_msgs.len() >= max_depth {
        return Err(());
    }
    let sender_tid = message.header.sender_tid;
    target.pending_msgs.try_push(tcb::PendingMsg {
        sender_tid,
        data: pending_mailbox::PendingMsgData::empty(),
        wire: Some(message),
        enqueued_tick: system_ticks() as u64,
    })
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

fn paused_target_rejects(sched: &Scheduler, caller_id: usize, target_id: usize) -> bool {
    // Lock order is SCHEDULER -> service registry. PauseService releases the
    // registry lock before checking scheduler drain state.
    if sched
        .tasks
        .get(&caller_id)
        .is_some_and(|task| task.supervisor_cap.is_some())
    {
        return false;
    }
    let Some(target) = sched.tasks.get(&target_id) else {
        return false;
    };
    if target.hotswap_ingress_closed {
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
fn arm_ipc_block_handoff(caller_id: usize) {
    let hart = hart_local::current_hart_id();
    if hart_local::ready::current_task_id_for(hart) == caller_id {
        hart_local::ready::begin_outgoing_context_save(hart, caller_id);
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
                |task| matches!(task.state, TaskState::Sending { target, .. } if target == target_id),
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
        crate::memory::tlb_shootdown_selftest::handle_store_fault(frame)
    }
    #[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
    {
        let _ = frame;
        false
    }
}

#[cfg(target_arch = "riscv64")]
const _: crate::hal::TlbShootdownTestFault = vi_tlb_shootdown_test_fault;

#[cfg(target_arch = "riscv64")]
#[no_mangle]
pub extern "Rust" fn vi_user_copy_guard_fault(
    frame: &mut crate::hal::arch::ViTrapFrame,
) -> bool {
    #[cfg(feature = "native-domains")]
    {
        let hart = unsafe { hart_local::current_hart() };
        if hart.user_copy_guard_active.load(core::sync::atomic::Ordering::Acquire) != 0 {
            let fault_addr = frame.stval;
            let lo = hart.user_copy_guard_start.load(core::sync::atomic::Ordering::Acquire);
            let hi = hart.user_copy_guard_end.load(core::sync::atomic::Ordering::Acquire);
            if fault_addr >= lo && fault_addr < hi {
                let resume_pc =
                    hart.user_copy_guard_resume_pc.load(core::sync::atomic::Ordering::Acquire);
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

#[cfg(target_arch = "riscv64")]
const _: crate::hal::UserCopyGuardFault = vi_user_copy_guard_fault;

// Global Scheduler Instance
pub(crate) static SCHEDULER: Spinlock<Option<Scheduler>> = Spinlock::new(None);

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
static mut BOOT_CONTEXTS: [crate::hal::arch::Context; smp::MAX_HARTS] =
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
    }; smp::MAX_HARTS];
#[cfg(target_arch = "aarch64")]
static mut BOOT_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
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
static mut BOOT_CONTEXT: crate::hal::arch::Context = crate::hal::arch::Context {
    r15: 0,
    r14: 0,
    r13: 0,
    r12: 0,
    rbx: 0,
    rbp: 0,
    sp: 0,
    rip: 0,
    kernel_trap_sp: 0,
};
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

pub fn system_ticks() -> usize {
    TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn tick() {
    TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn stack_pages_for(name: &str) -> usize {
    match name {
        // RedoxFS transactions exceed the pre-RedoxFS 64 KiB measurement and
        // must retain the conservative stack until a new watermark is captured.
        "vfs" => STACK_PAGES,
        "init" | "shell" | "vfs-test" | "net" | "virtio-net" | "thread" => MEASURED_STACK_PAGES,
        _ => STACK_PAGES,
    }
}

#[cfg(feature = "test-hooks")]
pub(crate) fn stack_sizing_policy_self_test() -> bool {
    ["init", "shell", "vfs-test", "net", "virtio-net", "thread"]
        .into_iter()
        .all(|name| stack_pages_for(name) == MEASURED_STACK_PAGES)
        && stack_pages_for("vfs") == STACK_PAGES
        && stack_pages_for("unmeasured-path") == STACK_PAGES
}

#[cfg(feature = "test-hooks")]
fn stack_baseline_bit(name: &str) -> Option<u8> {
    match name {
        "init" => Some(STACK_BASELINE_INIT),
        "shell" => Some(STACK_BASELINE_SHELL),
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
    hart_local::install(0);

    info!("Process: Initializing Scheduler...");
    let mut sched_guard = SCHEDULER.lock();

    // SAFETY: Use ptr::write to overwrite the Spinlock guard's data WITHOUT dropping the old value.
    // This prevents "Freed node aliases existing hole" panic on soft reboot (where .data persists but Heap is reset).
    unsafe {
        core::ptr::write(&mut *sched_guard, Some(Scheduler::new()));
    }
    drop(sched_guard);

    // Enable S-mode timer interrupt and arm the first preemption tick.
    // Done after scheduler init so vi_timer_tick() sees a valid SCHEDULER.
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    {
        // SAFETY: sets STIE (bit 5 = mask 0x20) in sie from S-mode. Must use the
        // register form of csrs — csrsi's immediate is only 5 bits (0..=31), so a
        // 0x20 mask cannot be encoded as an immediate.
        unsafe {
            core::arch::asm!("csrs sie, {stie}", stie = in(reg) 0x20usize);
        }
        let next = hal::common::timer::read_mtime() + hal::common::timer::TICKS_PER_10MS;
        hal::common::sbi::set_timer(next);
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
    retirement_selftest::observe_fault_task_entry();
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
    retirement_selftest::observe_fault_task_entry();
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
    terminate_current_cell_on_user_trap_fault(cause, pc, fault_addr);
}

#[cfg(target_arch = "aarch64")]
const _: crate::hal::TerminateOnFaultAarch64 = vi_terminate_on_fault_aarch64;

/// Exposes `scheduler::current_cell_id` to the HAL trap handler.
#[no_mangle]
pub extern "Rust" fn vi_current_cell_id() -> usize {
    scheduler::current_cell_id()
}

#[cfg(any(
    target_arch = "riscv64",
    target_arch = "riscv32",
    target_arch = "aarch64"
))]
const _: crate::hal::CurrentCellId = vi_current_cell_id;

/// Called from the S-mode timer ISR via `extern "Rust"` linkage.
///
/// Increments the global tick counter, rearmed the timer for the next
/// 10 ms slice, and yields the CPU so the scheduler can preempt the
/// current task if a higher-priority task has become runnable.
#[no_mangle]
pub extern "Rust" fn vi_timer_tick() {
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    retirement_selftest::observe_forced_ssip_trap();

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
    crate::task::drivers::console_drv::CONSOLE.lock().poll();

    // Run the scheduler.  If a higher-priority (or simply next round-robin)
    // task is ready, this performs a context switch.  Safe to call from the
    // timer ISR because:
    //   (a) interrupts are disabled by hardware on trap entry (sstatus.SIE=0)
    //   (b) yield_cpu() releases SCHEDULER lock before calling Context::switch
    //   (c) trap.S restores the correct ViTrapFrame from the new task's stack
    yield_cpu();
}

/// Retire a remote-root switch only from the incoming context, after the raw
/// context switch has changed stacks. Trap entry is intentionally not an ACK:
/// the interrupted retiring task can still execute its outgoing kernel path.
#[no_mangle]
pub extern "C" fn vi_context_switch_complete() {
    let hart = hart_local::current_hart_id();
    // A zero selected TID with an outgoing-save guard denotes task→boot,
    // rather than an unpinned task switch. Keep the terminal task's identity
    // published until this incoming boot context proves the old Context was
    // saved; otherwise a stale user trap could be dispatched as caller 0.
    let pinned = hart_local::ready::selected_task_id_for(hart);
    let switched_to_boot =
        pinned == 0 && hart_local::ready::outgoing_context_save_task_id_for(hart) != 0;
    // RV64 reaches this callback only after `__switch` has saved every
    // callee-save register and CSR of the outgoing Context. Release the
    // ready-queue steal guard before publishing the incoming ownership.
    hart_local::ready::complete_outgoing_context_save(hart);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    context_handoff_selftest::observe_outgoing_save_completion(hart);
    let selected = if pinned != 0 {
        pinned
    } else if switched_to_boot {
        0
    } else {
        hart_local::ready::current_task_id_for(hart)
    };
    hart_local::ready::complete_selected_switch(hart, selected);
    if switched_to_boot {
        hart_local::ready::set_current_task_id(hart, 0);
        hart_local::set_current_cell_context(0, 0);
        #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
        retirement_selftest::observe_heartbeat_boot_completion(hart);
    }
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    {
        let safe_root = hart_local::take_safe_root_pending();
        if switched_to_boot || safe_root {
            hart_local::acknowledge_safe_root();
        }
        // Safe-root completion owns outgoing attribution release: clear this
        // hart's execution pin on whatever root the completed switch left.
        // Unconditional take — a direct private→private switch leaves no
        // safe-root pending yet still owes the displaced root a release.
        if let Some(space) = hart_local::take_staged_execution_release() {
            let _ = space.set_current_hart(hart, false);
        }
        user_copy::clear_guard_for_context_switch();
    }
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    context_handoff_selftest::observe_origin_ownership_release(hart);

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    log::info!(
        "[selftest] SMP-RETIREMENT: stage=rv64-switch-boundary hart={} selected={} executing={}",
        hart,
        selected,
        hart_local::ready::executing_task_id_for(hart),
    );

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    retirement_selftest::hold_before_switch_completion(hart);
    smp::complete_retirement_switch(hart);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    retirement_selftest::observe_switch_completion(hart);
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
    provenance: hart_local::TrapProvenUserFault,
    cause: usize,
    pc: usize,
    fault_addr: usize,
) {
    let hart = hart_local::current_hart_id();
    let fault = hart_local::DeferredFault::from_user_trap(
        provenance,
        hart_local::ready::current_task_id_for(hart),
        hart_local::current_cell_id(),
        hart_local::current_cell_generation(),
        cause,
        pc,
        fault_addr,
    );

    // This fixed record is the only fault-path publication before the allocator
    // is uncharged from the victim.  Do not move logging, task lookup, or
    // scheduler collection work above this attribution handoff.
    hart_local::defer_fault(fault);
    hart_local::set_current_cell_id(0);

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    retirement_selftest::observe_fault_deferred_record_commit(fault);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    retirement_selftest::observe_fault_kernel_attribution(fault.cell_id);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    retirement_selftest::observe_fault_funnel_entry(fault.tid);

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
        hart_local::TrapProvenUserFault::new(),
        cause,
        pc,
        fault_addr,
    );
}

/// Core scheduling logic: picks next task and performs switch OUTSIDE of the lock.
pub fn yield_cpu() {
    // RV64 cooperative yields can enter with SIE set (not only from trap
    // context). Capture and clear it before any scheduler lock or publication;
    // masking only at `__switch` leaves current/selected scheduler state interruptible.
    #[cfg(target_arch = "riscv64")]
    let outgoing_sstatus = crate::hal::arch::save_and_disable_interrupts();

    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::observe_schedule_attempt();

    // x86_64: disable interrupts for the entire scheduler critical section.
    // Without this, the LAPIC timer fires mid-lock and causes a nested
    // yield_cpu call that deadlocks on the same spinlock (IRQ-in-lock deadlock).
    // RISC-V/AArch64 automatically clear the interrupt-enable bit on trap entry,
    // so they don't have this problem when called from vi_timer_tick.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
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
    let deferred_retired = match hart_local::take_deferred_retirement() {
        Some(hart_local::DeferredRetirement::Fault(fault)) => {
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            retirement_selftest::observe_fault_scheduler_funnel_attempt(fault.tid);
            let mut guard = SCHEDULER.lock();
            let scheduler = guard
                .as_mut()
                .expect("[fault] deferred handoff before scheduler initialization");
            scheduler.retire_deferred_fault(fault);
            Some(fault.tid)
        }
        Some(hart_local::DeferredRetirement::Exit(exit)) => {
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
        retirement_selftest::observe_fault_scheduler_retirement(tid);
        retirement_selftest::hold_after_fault_scheduler_retirement(tid);
    }
    #[cfg(not(all(feature = "test-hooks", target_arch = "riscv64")))]
    let _ = deferred_retired;
    let reaped = {
        if let Some(sched) = SCHEDULER.lock().as_mut() {
            sched.take_reapable_zombies()
        } else {
            alloc::vec::Vec::new()
        }
    };
    drop(reaped);

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
    for retirement in root_retirements {
        for tid in retirement.member_tids.iter().copied() {
            reap_retired_task_resources(tid);
            crate::task::syscall::release_vfs_holder_leases(tid);
        }
        // `take_quiescent_root_retirements` removed these exact zombies while
        // holding SCHEDULER after all remote switch proofs completed. Drop their
        // stacks before exposing the CellId or quota to a new generation.
        drop(retirement.zombies);
        crate::cell::cap_registry::CAP_TABLE
            .lock()
            .revoke_all_for(types::CellId(retirement.owner.cell_id));
        crate::fast_ipc::clear_vfs_if_cell(retirement.owner.cell_id as usize);
        crate::resource_registry::release_for(types::CellId(retirement.owner.cell_id));
        // The quota row is the admission gate for `reserve_next`. Keep it
        // registered until every generation-scoped resource is gone and the
        // retiring owner slot has been released; otherwise another hart can
        // reserve this CellId while the scheduler still identifies it as
        // `Retiring`.
        let owner_slot_released = SCHEDULER
            .lock()
            .as_mut()
            .is_some_and(|sched| sched.finish_root_retirement(retirement.owner));
        if owner_slot_released {
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            log::info!(
                "[selftest] SMP-RETIREMENT: stage=owner-slot-released-before-quota cell={} generation={} root={}",
                retirement.owner.cell_id,
                retirement.owner.generation,
                retirement.owner.root_tid,
            );
            crate::memory::cell_quota::deregister(types::CellId(retirement.owner.cell_id));
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            {
                log::info!(
                    "[selftest] SMP-RETIREMENT: stage=quota-deregistered-after-owner-slot cell={} generation={} root={}",
                    retirement.owner.cell_id,
                    retirement.owner.generation,
                    retirement.owner.root_tid,
                );
                retirement_selftest::observe_cell_id_release(retirement.owner);
            }
        } else {
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

    // Turn completion appends into scheduler wakes. An append may run in
    // interrupt context and must not take SCHEDULER, so it only raises a flag;
    // the wake happens here, the same deferral the two reaps above use. The
    // gate is one relaxed load on the overwhelmingly common empty tick.
    if completion::wakes_pending() {
        if let Some(sched) = SCHEDULER.lock().as_mut() {
            completion::deliver_pending_wakes(sched);
        }
    }

    let hart_id = hart_local::current_hart_id();
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    let switch_info = if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.pick_next_domain(hart_id)
    } else {
        None
    };
    #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
    let switch_info = if let Some(sched) = SCHEDULER.lock().as_mut() {
        sched.pick_next(hart_id)
    } else {
        None
    };
    // The scheduler lock is now released but the terminal Context is still
    // executing. Exercise the real trap admission boundary in this exact
    // pre-switch interval; the regression proves a heartbeat victim cannot
    // become caller 0 before boot owns the incoming Context.
    #[cfg(all(
        feature = "test-hooks",
        target_arch = "riscv64",
        feature = "native-domains"
    ))]
    if let Some(ref plan) = switch_info {
        if plan.incoming.is_null() {
            retirement_selftest::observe_heartbeat_terminal_current();
        }
    }
    #[cfg(all(
        feature = "test-hooks",
        target_arch = "riscv64",
        not(feature = "native-domains")
    ))]
    if let Some((_, next)) = switch_info {
        if next.is_null() {
            retirement_selftest::observe_heartbeat_terminal_current();
        }
    }
    if switch_info.is_none() {
        // Selection is normally published only with a concrete switch tuple.
        // Clear defensively on the no-switch/abort path so a stale pin cannot
        // retain a retired Context indefinitely.
        hart_local::ready::abort_selected_switch(hart_id);
    }
    #[cfg(target_arch = "riscv64")]
    if switch_info.is_none() {
        // No incoming Context will restore status for us. Preserve the caller's
        // exact SIE state instead of unconditionally enabling interrupts.
        unsafe {
            crate::hal::arch::restore_sstatus(outgoing_sstatus);
        }
    }

    #[cfg(target_arch = "x86_64")]
    if switch_info.is_none() {
        unsafe {
            // No switch: re-enable interrupts before returning to the idle loop.
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    if let Some(plan) = switch_info {
        unsafe {
            let final_curr = if plan.outgoing.is_null() {
                &raw mut BOOT_CONTEXTS[hart_id]
            } else {
                plan.outgoing
            };
            let final_next = if plan.incoming.is_null() {
                &raw const BOOT_CONTEXTS[hart_id]
            } else {
                plan.incoming
            };
            if !plan.incoming.is_null() {
                crate::hal::arch::set_kernel_stack((&*plan.incoming).sp);
            }
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            retirement_selftest::hold_after_selection_before_switch(hart_id);
            let (root_ppn, asid) = plan.root_switch();
            crate::hal::arch::Context::switch_with_saved_sstatus(
                final_curr,
                final_next,
                outgoing_sstatus,
                root_ppn,
                asid,
            );
        }
    }
    #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
    if let Some((curr, next)) = switch_info {
        unsafe {
            let final_curr = if curr.is_null() {
                #[cfg(target_arch = "riscv64")]
                {
                    &raw mut BOOT_CONTEXTS[hart_id]
                }
                #[cfg(not(target_arch = "riscv64"))]
                {
                    &raw mut BOOT_CONTEXT
                }
            } else {
                curr
            };

            let final_next = if next.is_null() {
                #[cfg(target_arch = "riscv64")]
                {
                    &raw const BOOT_CONTEXTS[hart_id]
                }
                #[cfg(not(target_arch = "riscv64"))]
                {
                    &raw const BOOT_CONTEXT
                }
            } else {
                next
            };
            if !next.is_null() {
                // Set TSS.rsp0 / KERNEL_GS_BASE for the next task's syscall path.
                // On x86_64 use kernel_trap_sp (= kstack_top - TRAP_FRAME_SIZE, fixed
                // at spawn) so CPU_LOCAL.kernel_rsp never drifts to the deep
                // cooperative-switch RSP saved inside a blocked yield_cpu frame.
                let next_ref = &*next;
                // aarch64 context stores sp as u64 (register-width field);
                // riscv64 already uses usize — keep it cast-free so clippy's
                // same-type-cast lint stays quiet on that target.
                #[cfg(target_arch = "aarch64")]
                crate::hal::arch::set_kernel_stack(next_ref.sp as usize);
                #[cfg(not(any(
                    target_arch = "x86_64",
                    target_arch = "aarch64",
                    target_arch = "riscv32"
                )))]
                crate::hal::arch::set_kernel_stack(next_ref.sp);
                #[cfg(target_arch = "riscv32")]
                crate::hal::arch::set_kernel_stack(next_ref.sp as usize);
                #[cfg(target_arch = "x86_64")]
                crate::hal::arch::set_kernel_stack(next_ref.kernel_trap_sp as usize);
            }
            #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
            retirement_selftest::hold_after_selection_before_switch(hart_id);

            #[cfg(target_arch = "riscv64")]
            crate::hal::arch::Context::switch_with_saved_sstatus(
                final_curr,
                final_next,
                outgoing_sstatus,
                0,
                0,
            );
            #[cfg(not(target_arch = "riscv64"))]
            crate::hal::arch::Context::switch(final_curr, final_next);

            // Execution resumes here when this context is switched BACK to.
            // Re-enable interrupts: the cli above masked IRQs for the lock section;
            // iretq (ring-3 entry) will have re-enabled them on the other CPU path,
            // but on the resume path here we must restore IF explicitly.
            #[cfg(target_arch = "x86_64")]
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }
}

/// Reap state keyed by a concrete task TID. This is intentionally shared by
/// worker and root retirement so a root cannot leave a member's grant, pin,
/// IOMMU domain, BDF, VM, or VFS lease behind for a reused CellId.
fn reap_retired_task_resources(tid: usize) {
    crate::resource_registry::release_bdfs_for(tid);
    // IOFENCE/IOTLB completion is the acknowledgement required before pinned
    // frames may leave quarantine.
    crate::task::drivers::iommu::cleanup_cell(tid as u64);
    crate::task::syscall::release_acked_frames(tid);
    crate::task::syscall::reap_grants_for_task(tid);
    crate::hypervisor::registry::reap_vms_for_task(tid);
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
/// [`scheduler::Scheduler::spawn_with_stacks`] for why the caller allocates.
pub fn spawn_with_stacks(
    name: &str,
    cell_id: CellId,
    allowed_drivers: alloc::vec::Vec<usize>,
    kstack: stack::Stack,
    ustack: stack::Stack,
) -> Result<usize, ViError> {
    match SCHEDULER.lock().as_mut() {
        Some(sched) => Ok(sched.spawn_with_stacks(name, cell_id, allowed_drivers, kstack, ustack)),
        None => Err(ViError::Unknown),
    }
}

pub fn spawn_with_arg(
    name: &str,
    cell_id: CellId,
    allowed_drivers: alloc::vec::Vec<usize>,
    entry: VAddr,
    arg: usize,
) -> Result<usize, ViError> {
    match SCHEDULER.lock().as_mut() {
        Some(sched) => sched.spawn_thread(name, cell_id, allowed_drivers, entry, arg),
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
    hart_local::ready::current_task_id_for(hart_local::current_hart_id())
}

pub fn has_ready_tasks() -> bool {
    hart_local::ready::total_ready_count() > 0
}

// Helper to resolve path relative to CWD
fn resolve_path(cwd: &str, path: &str) -> alloc::string::String {
    if path.starts_with('/') {
        alloc::string::String::from(path)
    } else {
        // Simple path joining
        let mut p = alloc::string::String::from(cwd);
        if !p.ends_with('/') {
            p.push('/');
        }
        p.push_str(path);
        // TODO: Handle ".." and "." canonicalization
        p
    }
}

// --- File System Syscall Handlers ---

#[allow(clippy::result_unit_err)]
pub fn file_open(path: &str) -> core::result::Result<usize, ()> {
    // 1. Resolve path
    let full_path = if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            resolve_path(&task.cwd, path)
        } else {
            // Should not happen
            String::from(path)
        }
    } else {
        return Err(());
    };

    // 2. Open file via VIFS
    // We loop to acquire FS lock to avoid deadlock with scheduler lock?
    // No, here we don't hold scheduler lock while calling FS.

    // Check if VIFS1 is initialized
    use crate::fs::VIFS1;
    let file = {
        let mut fs_lock = VIFS1.lock();
        if let Some(fs) = fs_lock.as_mut() {
            fs.open(&full_path, api::fs::OpenMode::Read)
                .map_err(|_| ())?
        } else {
            return Err(());
        }
    };

    // 3. Store in Task
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            let fd = task.open_files.keys().max().map(|k| k + 1).unwrap_or(3); // Start FD at 3 (0,1,2 reserved)
            task.open_files
                .insert(fd, crate::task::tcb::FileHandle::new(file));
            return Ok(fd);
        }
    }

    // Task terminated concurrently?
    Err(())
}

pub fn file_read(fd: usize, buf: &mut [u8]) -> usize {
    if fd == 0 {
        // Stdin (Keyboard)
        if buf.is_empty() {
            return 0;
        }

        let mut cons = crate::task::drivers::console_drv::CONSOLE.lock();
        cons.poll();
        let b = cons.read_byte();
        if let Some(byte) = b {
            buf[0] = byte;
            return 1;
        }
        return 0;
    }

    // File Read — synchronous. The VIFS1 ramdisk is synchronous, and the async
    // path (read_async → pending_future + state=Polling) called straight back into
    // this same sync `read()` anyway. But it returned a dummy 0 to the caller while
    // the future was never driven to completion, so a blocking reader (e.g. DOOM's
    // WAD load) received 0 bytes and an uninitialized buffer ("doesn't have IWAD").
    // Read directly under the SCHEDULER lock and return the real byte count.
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            if let Some(handle) = task.open_files.get_mut(&fd) {
                return handle.read(buf).unwrap_or(0);
            }
        }
    }
    0
}

pub fn file_write(fd: usize, buf: &[u8]) -> usize {
    if fd == 1 || fd == 2 {
        // Stdout/Stderr
        if let Ok(s) = core::str::from_utf8(buf) {
            crate::task::print_user_log(s);
            return buf.len();
        }
        return 0;
    }
    0
}

pub fn file_close(fd: usize) {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            task.open_files.remove(&fd);
        }
    }
}

pub fn file_readdir(fd: usize, buf: &mut [u8]) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            if let Some(handle) = task.open_files.get_mut(&fd) {
                match handle.read_dir() {
                    Ok(Some(entry)) => {
                        // Serialize DirEntry to buf
                        // Entry size is 64 + 1 + 8 + padding = 73+ ? sizeof(DirEntry)
                        // types::DirEntry is repr(C).
                        // We copy bytes directly.
                        let ptr = &entry as *const _ as *const u8;
                        let size = core::mem::size_of::<types::DirEntry>();
                        if buf.len() < size {
                            return Err(());
                        }

                        unsafe {
                            core::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), size);
                        }
                        return Ok(size);
                    }
                    Ok(None) => return Ok(0), // EOF
                    Err(_) => return Err(()),
                }
            }
        }
    }
    Err(())
}

pub fn file_fstat(_fd: usize, _stat_ptr: usize) -> core::result::Result<usize, ()> {
    Err(())
}

pub fn file_chdir(_path: &str) -> core::result::Result<usize, ()> {
    // TODO: Implement chdir
    Ok(0)
}

pub fn file_seek(fd: usize, offset: isize, whence: usize) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            if let Some(handle) = task.open_files.get_mut(&fd) {
                let pos = match whence {
                    0 => api::fs::SeekFrom::Start(offset as u64),
                    1 => api::fs::SeekFrom::Current(offset as i64),
                    2 => api::fs::SeekFrom::End(offset as i64),
                    _ => return Err(()), // Invalid whence
                };

                if let Ok(new_pos) = handle.seek(pos) {
                    return Ok(new_pos as usize);
                }
            }
        }
    }
    Err(())
}

pub fn file_remove(path: &str) -> core::result::Result<usize, ()> {
    // 1. Resolve path
    let full_path = if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.current_task_mut() {
            resolve_path(&task.cwd, path)
        } else {
            String::from(path)
        }
    } else {
        return Err(());
    };

    use crate::fs::VIFS1;
    let mut fs_lock = VIFS1.lock();
    if let Some(fs) = fs_lock.as_mut() {
        if fs.remove(&full_path).is_ok() {
            return Ok(0);
        }
    }
    Err(())
}

pub fn file_rename(_old: &str, _new: &str) -> core::result::Result<usize, ()> {
    // TODO: Implement rename in ViFileSystem trait first
    Err(())
}

pub fn file_getcwd(_buf: &mut [u8]) -> core::result::Result<usize, ()> {
    Err(())
}
use crate::task::tcb::LeaseAttributes;

pub fn ipc_lend(
    _lender_id: usize,
    target_id: usize,
    ptr: VAddr,
    len: usize,
    flags: u32,
) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(target_task) = sched.tasks.get_mut(&target_id) {
            let lease_id = target_task.add_lease(ptr, len, LeaseAttributes(flags));
            return Ok(lease_id);
        }
    }
    Err(())
}

pub fn ipc_send(
    caller_id: usize,
    target_id: usize,
    msg_ptr: VAddr,
    msg_len: usize,
) -> core::result::Result<usize, IpcSendError> {
    if msg_len > ipc_wire::MAX_IPC_WIRE_PAYLOAD {
        return Err(IpcSendError::Backpressure);
    }

    let (caller_view, header) = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(IpcSendError::TargetGone)?;
        if !sched.tasks.contains_key(&target_id) {
            log::debug!("IPC: Target Task {} not found (cell exited)", target_id);
            return Err(IpcSendError::TargetGone);
        }
        if paused_target_rejects(sched, caller_id, target_id) {
            return Err(IpcSendError::Backpressure);
        }
        let caller = sched.tasks.get(&caller_id).ok_or(IpcSendError::TargetGone)?;
        let caller_view = copy_glue::TaskCopyView::of(caller);
        let (sender_cell_id, sender_generation) = sender_context(sched, caller_id);
        let header = ipc_wire::IpcWireHeader {
            sender_tid: caller_id,
            sender_cell_id,
            sender_generation,
            delivery_id: next_delivery_id(),
        };
        (caller_view, header)
    };

    let msg_bytes = caller_view
        .read_bytes(msg_ptr, msg_len)
        .map_err(|_| IpcSendError::Backpressure)?;
    let wire_msg = ipc_wire::IpcWireMessage::try_new(header, &msg_bytes)
        .map_err(|_| IpcSendError::Backpressure)?;

    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(IpcSendError::TargetGone)?;
    if !sched.tasks.contains_key(&target_id) {
        return Err(IpcSendError::TargetGone);
    }
    if paused_target_rejects(sched, caller_id, target_id) {
        return Err(IpcSendError::Backpressure);
    }

    let target = sched.tasks.get(&target_id).ok_or(IpcSendError::TargetGone)?;
    let target_ready =
        matches!(target.state, TaskState::Recv { mask, .. } if mask == 0 || mask == caller_id);
    let target_frozen = matches!(target.state, TaskState::Frozen { .. });

    // Publish before any blocking decision: queue-full is a Backpressure
    // error, never a block. Once queued, the kernel owns the payload and
    // sender death cannot invalidate it.
    if let Some(target) = sched.tasks.get_mut(&target_id) {
        queue_wire_msg(target, wire_msg, tcb::HOTSWAP_MSG_QUEUE_DEPTH)
            .map_err(|_| IpcSendError::Backpressure)?;
        if target_ready {
            // Caller-context assignment is deferred to the dequeue/commit
            // path so the request generation advances exactly once per
            // accepted message.
            target.state = TaskState::Ready;
        }
    }
    if target_frozen {
        log::debug!(
            "[hotswap] queued msg ({} bytes) from tid={} to frozen tid={}",
            msg_len,
            caller_id,
            target_id
        );
        return Ok(0);
    }
    if target_ready {
        let prio = sched.push_ready(target_id);
        sched.pend_preempt_if_needed(prio);
        return Ok(0);
    }
    arm_ipc_block_handoff(caller_id);
    if let Some(caller) = sched.tasks.get_mut(&caller_id) {
        caller.reply_value = None;
        caller.state = TaskState::Sending {
            target: target_id,
            delivery_id: header.delivery_id,
        };
    }
    Ok(1)
}

/// Post a message to `target_id` without blocking the caller.
///
/// Queues an owned message and wakes the target immediately when it is in `Recv`.
/// Busy targets retain the queued message until their next receive call. The
/// mailbox is bounded by `HOTSWAP_MSG_QUEUE_DEPTH`.
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
        if !sched.tasks.contains_key(&target_id) {
            return Err(());
        }
        if paused_target_rejects(sched, sender_id, target_id) {
            return Err(());
        }
        if msg.len() > ipc_wire::MAX_IPC_WIRE_PAYLOAD {
            return Err(());
        }

        // Known pre-existing contract gap (2026-07-31 Recv buffer-pinning audit):
        // unlike ipc_send/ipc_try_send, this path intentionally preserves its
        // current behavior of matching any Recv without consulting the mask.
        let target_ready = sched
            .tasks
            .get(&target_id)
            .is_some_and(|t| matches!(t.state, TaskState::Recv { .. }));

        let (sender_cell_id, sender_generation) = sender_context(sched, sender_id);
        let header = ipc_wire::IpcWireHeader {
            sender_tid: sender_id,
            sender_cell_id,
            sender_generation,
            delivery_id: next_delivery_id(),
        };
        let wire = ipc_wire::IpcWireMessage::try_new(header, msg)?;
        if let Some(t) = sched.tasks.get_mut(&target_id) {
            queue_wire_msg(t, wire, tcb::HOTSWAP_MSG_QUEUE_DEPTH)?;
            // Caller-context assignment is deferred to the dequeue/commit
            // path so the request generation advances exactly once.
            if target_ready {
                t.state = TaskState::Ready;
            }
        }
        if target_ready {
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
                caller.state = TaskState::Recv {
                    mask,
                    buf_ptr,
                    buf_len,
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
        let snapshot = wire.try_clone()?;
        let receiver_view = copy_glue::TaskCopyView::of(receiver_task);
        (sender_id, header, snapshot, receiver_view)
    };

    // Copy-out: failure leaves queue and sender token untouched.
    let copy_len = core::cmp::min(snapshot.len(), buf_len);
    if copy_len > 0 {
        receiver_view.write_bytes(buf_ptr, &snapshot.as_slice()[..copy_len])?;
    }

    // Commit: remove by delivery token, wake sender, assign caller context.
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(())?;
    if let Some(receiver_task) = sched.tasks.get_mut(&caller_id) {
        let pos = receiver_task
            .pending_msgs
            .iter()
            .position(|message| {
                message.sender_tid == sender_id
                    && message.wire.as_ref().is_some_and(|w| w.header.delivery_id == header.delivery_id)
            });
        if let Some(index) = pos {
            receiver_task.pending_msgs.remove(index);
        }
        receiver_task.set_received_caller_context(sender_id, header.sender_cell_id, header.sender_generation);
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
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(())?;
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
        let snapshot = wire.try_clone()?;
        let receiver_view = copy_glue::TaskCopyView::of(receiver_task);
        (sender_id, header, snapshot, receiver_view)
    };

    let copy_len = core::cmp::min(snapshot.len(), buf_len);
    if copy_len > 0 {
        receiver_view.write_bytes(buf_ptr, &snapshot.as_slice()[..copy_len])?;
    }

    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(())?;
    if let Some(receiver_task) = sched.tasks.get_mut(&caller_id) {
        let pos = receiver_task
            .pending_msgs
            .iter()
            .position(|message| {
                message.sender_tid == sender_id
                    && message.wire.as_ref().is_some_and(|w| w.header.delivery_id == header.delivery_id)
            });
        if let Some(index) = pos {
            receiver_task.pending_msgs.remove(index);
        }
        receiver_task.set_received_caller_context(sender_id, header.sender_cell_id, header.sender_generation);
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
    header: ipc_wire::IpcWireHeader,
) {
    let Some(sender_task) = sched.tasks.get(&sender_id) else {
        return;
    };
    let identity_matches = sender_task.cell_id.0 == header.sender_cell_id
        && sender_task.cell_generation == header.sender_generation;
    let token_matches = matches!(
        sender_task.state,
        TaskState::Sending { target, delivery_id }
            if target == target_id && delivery_id == header.delivery_id
    );
    if identity_matches && token_matches {
        if let Some(sender_task) = sched.tasks.get_mut(&sender_id) {
            sender_task.state = TaskState::Ready;
            sched.push_ready(sender_id);
        }
    }
}

pub fn ipc_try_send(
    caller_id: usize,
    target_id: usize,
    msg_ptr: VAddr,
    msg_len: usize,
) -> core::result::Result<(), ()> {
    enum TrySendAction {
        Publish { caller_view: copy_glue::TaskCopyView, is_input_mailbox_sender: bool },
        Reject,
    }

    let action = {
        let guard = SCHEDULER.lock();
        let sched = guard.as_ref().ok_or(())?;
        if !sched.tasks.contains_key(&target_id) || paused_target_rejects(sched, caller_id, target_id) {
            return Err(());
        }
        if msg_len > ipc_wire::MAX_IPC_WIRE_PAYLOAD {
            return Err(());
        }
        let target_ready = sched
            .tasks
            .get(&target_id)
            .map(|t| matches!(t.state, TaskState::Recv { mask, .. } if mask == 0 || mask == caller_id))
            .unwrap_or(false);
        let input_tid = crate::task::drivers::driver_cell::INPUT_CELL_TID
            .load(core::sync::atomic::Ordering::Relaxed);
        let is_input_mailbox_sender = caller_id == input_tid && input_tid != 0;
        if target_ready || is_input_mailbox_sender {
            let caller_view = sched
                .tasks
                .get(&caller_id)
                .map(|t| copy_glue::TaskCopyView::of(t))
                .ok_or(())?;
            TrySendAction::Publish { caller_view, is_input_mailbox_sender }
        } else {
            TrySendAction::Reject
        }
    };

    let TrySendAction::Publish { caller_view, is_input_mailbox_sender, .. } = action else {
        return Err(());
    };
    let msg_bytes = caller_view.read_bytes(msg_ptr, msg_len).map_err(|_| ())?;
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().ok_or(())?;
    // Revalidate under the second lock: phase-1 target state is stale once
    // released. A non-input-mailbox sender that raced a target leaving Recv
    // must be rejected; the input mailbox path queues regardless of state.
    if !sched.tasks.contains_key(&target_id) || paused_target_rejects(sched, caller_id, target_id) {
        return Err(());
    }
    let target_ready = sched
        .tasks
        .get(&target_id)
        .map(|t| matches!(t.state, TaskState::Recv { mask, .. } if mask == 0 || mask == caller_id))
        .unwrap_or(false);
    if !target_ready && !is_input_mailbox_sender {
        return Err(());
    }
    let (sender_cell_id, sender_generation) = sender_context(sched, caller_id);
    let header = ipc_wire::IpcWireHeader {
        sender_tid: caller_id,
        sender_cell_id,
        sender_generation,
        delivery_id: next_delivery_id(),
    };
    let wire = ipc_wire::IpcWireMessage::try_new(header, &msg_bytes)?;
    if let Some(target) = sched.tasks.get_mut(&target_id) {
        queue_wire_msg(target, wire, tcb::INPUT_EVENT_QUEUE_DEPTH)?;
        // Caller-context assignment is deferred to the dequeue/commit path
        // so the request generation advances exactly once per message.
        if target_ready {
            target.state = TaskState::Ready;
        }
    }
    if target_ready {
        let prio = sched.push_ready(target_id);
        sched.pend_preempt_if_needed(prio);
    }
    Ok(())
}

pub fn ipc_reply(caller_id: usize, result: usize) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        let target_id = sched.tasks.get(&caller_id).and_then(|t| t.current_caller);
        if let Some(tid) = target_id {
            if let Some(t) = sched.tasks.get_mut(&tid) {
                t.state = TaskState::Ready;
                t.reply_value = Some(result);
            }
            let prio = sched.push_ready(tid);
            sched.pend_preempt_if_needed(prio);
            if let Some(task) = sched.tasks.get_mut(&caller_id) {
                task.clear_current_caller_context();
            }
            return Ok(0);
        }
    }
    Err(())
}

pub fn ipc_borrow_read(
    caller_id: usize,
    lease_id: usize,
    offset: usize,
    dst_ptr: VAddr,
    len: usize,
) -> core::result::Result<usize, ()> {
    // Audit guard (2026-07-31): no live production caller currently exists.
    // Re-enabling this path requires a separate lease pin/lifetime review.
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.tasks.get(&caller_id) {
            if let Some(lease) = task.get_lease(lease_id) {
                if !lease.attributes.contains(LeaseAttributes::READ) {
                    return Err(());
                }
                // Use checked_add to prevent `offset + len` wraparound which
                // would otherwise let a caller construct an arbitrary R/W
                // primitive into the lease's surrounding memory.
                let end = offset.checked_add(len).ok_or(())?;
                if end > lease.len {
                    return Err(());
                }
                let src = lease.ptr.checked_add(offset).ok_or(())?;
                unsafe {
                    core::ptr::copy_nonoverlapping(src as *const u8, dst_ptr as *mut u8, len);
                }
                return Ok(len);
            }
        }
    }
    Err(())
}

pub fn ipc_borrow_write(
    caller_id: usize,
    lease_id: usize,
    offset: usize,
    src_ptr: VAddr,
    len: usize,
) -> core::result::Result<usize, ()> {
    // Audit guard (2026-07-31): no live production caller currently exists.
    // Re-enabling this path requires a separate lease pin/lifetime review.
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.tasks.get(&caller_id) {
            if let Some(lease) = task.get_lease(lease_id) {
                if !lease.attributes.contains(LeaseAttributes::WRITE) {
                    return Err(());
                }
                let end = offset.checked_add(len).ok_or(())?;
                if end > lease.len {
                    return Err(());
                }
                let dst = lease.ptr.checked_add(offset).ok_or(())?;
                unsafe {
                    core::ptr::copy_nonoverlapping(src_ptr as *const u8, dst as *mut u8, len);
                }
                return Ok(len);
            }
        }
    }
    Err(())
}

pub fn ipc_grant(
    caller_id: usize,
    target_id: usize,
    ptr: VAddr,
    len: usize,
    flags: u32,
) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(target) = sched.tasks.get_mut(&target_id) {
            let gid = target.add_grant(ptr, len, flags, caller_id);
            return Ok(gid);
        }
    }
    Err(())
}

pub fn ipc_map(caller_id: usize, grant_id: usize) -> core::result::Result<usize, ()> {
    if let Some(sched) = SCHEDULER.lock().as_ref() {
        if let Some(task) = sched.tasks.get(&caller_id) {
            if let Some(grant) = task.get_grant(grant_id) {
                return Ok(grant.ptr);
            }
        }
    }
    Err(())
}

/// Get scheduler statistics
pub fn scheduler_stats() -> (usize, usize) {
    let task_count = if let Some(sched) = SCHEDULER.lock().as_ref() {
        sched.tasks.len()
    } else {
        0
    };
    (task_count, hart_local::ready::total_ready_count())
}

pub fn futex_wait(caller_id: usize, addr: VAddr, val: u32) -> core::result::Result<usize, ()> {
    // Check condition
    unsafe {
        let current_val = *(addr as *const u32);
        if current_val != val {
            return Err(()); // EAGAIN
        }
    }

    if let Some(sched) = SCHEDULER.lock().as_mut() {
        if let Some(task) = sched.tasks.get_mut(&caller_id) {
            task.state = TaskState::FutexWait { addr };
            return Ok(0);
        }
    }
    Err(())
}

pub fn futex_wake(_caller_id: usize, addr: VAddr, count: usize) -> core::result::Result<usize, ()> {
    let mut woken = 0;
    if let Some(sched) = SCHEDULER.lock().as_mut() {
        let mut to_wake = alloc::vec::Vec::new();

        // Scan for waiting tasks
        for (tid, task) in sched.tasks.iter() {
            // Skip self? Futex wake usually doesn't wake self (self is running).
            if let TaskState::FutexWait { addr: wa_addr } = task.state {
                if wa_addr == addr {
                    to_wake.push(*tid);
                    if to_wake.len() >= count {
                        break;
                    }
                }
            }
        }

        woken = to_wake.len();

        // Wake them up
        for tid in to_wake {
            if let Some(task) = sched.tasks.get_mut(&tid) {
                task.state = TaskState::Ready;
                sched.push_ready(tid);
            }
        }
    }
    Ok(woken)
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

static LOG_RING: crate::sync::Spinlock<LogRing> = crate::sync::Spinlock::new(LogRing::new());

/// Drain up to `buf.len()` bytes from the user-log ring into `buf`.
/// Called by the `ReadLog = 237` syscall handler.
pub fn read_log_ring(buf: &mut [u8]) -> usize {
    LOG_RING.lock().drain(buf)
}

/// Tracks whether the console cursor is at the start of a line, so the "USER: "
/// prefix is emitted ONCE per line rather than once per `sys_log` call. Without
/// this, `print()` (no trailing newline — used for the shell prompt and per-key
/// echo) would force a prefix+newline on every byte, so typing "help" rendered as
/// four "USER: h/e/l/p" lines instead of an inline "USER: help".
static USER_LOG_AT_LINE_START: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(true);

pub fn print_user_log(msg: &str) {
    use core::sync::atomic::Ordering;
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

    let mut rest = msg;
    while !rest.is_empty() {
        if USER_LOG_AT_LINE_START.load(Ordering::Relaxed) {
            crate::task::drivers::uart::write_console("USER: ");
            USER_LOG_AT_LINE_START.store(false, Ordering::Relaxed);
        }
        match rest.find('\n') {
            Some(i) => {
                crate::task::drivers::uart::write_console(&rest[..=i]);
                USER_LOG_AT_LINE_START.store(true, Ordering::Relaxed);
                rest = &rest[i + 1..];
            }
            None => {
                crate::task::drivers::uart::write_console(rest);
                rest = "";
            }
        }
    }
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
        let mut frame_guard = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = frame_guard.as_mut().ok_or(ViError::OutOfMemory)?;
        let frame = allocator.allocate_frame().ok_or(ViError::OutOfMemory)?;

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

        crate::memory::paging::map_page(allocator, entry, frame, flags)
            .map_err(|_| ViError::OutOfMemory)?;
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
