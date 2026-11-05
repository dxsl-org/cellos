//! SMP: secondary hart startup and controlled park loop.
//!
//! Phase 01: brings each secondary hart online, installs its trap vector,
//! then parks it in WFI.  Phase 03 replaces the park loop with a per-hart
//! scheduler round.
//!
//! Invariant: hart 0 calls `start_secondaries()` only AFTER `task::init()`
//! completes — the SCHEDULER and heap are live before any secondary runs.

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicBool, Ordering};

mod topology;
pub use topology::{CpuTopology, CPU_TOPOLOGY};

/// Maximum number of logical harts this kernel tracks. Every per-hart static
/// is sized by it, so no heap allocation happens on the boot critical path.
/// This is capacity, not the number of CPUs brought online.
///
/// - x86_64: the HAL's per-CPU table bound, `hal::x86_64::syscall::MAX_CPUS`
///   (64). 64 keeps every `u64` hart mask exact, including the published
///   `ViRetirementV1.remote_hart_mask` (plan 261103-2218 H-S2-5).
/// - x86_64 G1 private lane (`cellos_g1_cpu`): 2, which
///   `task::cpu_accounting` asserts equal to its own hart count.
/// - riscv64 / aarch64 / 32-bit targets: 2 (QEMU virt `-smp 2`, G2 entry).
#[cfg(all(target_arch = "x86_64", not(cellos_g1_cpu)))]
pub const MAX_HARTS: usize = hal::x86_64::syscall::MAX_CPUS;
#[cfg(not(all(target_arch = "x86_64", not(cellos_g1_cpu))))]
pub const MAX_HARTS: usize = 2;
// The HAL owns one GS record, GDT/TSS and IST set per logical CPU.
#[cfg(target_arch = "x86_64")]
const _: () = assert!(MAX_HARTS <= hal::x86_64::syscall::MAX_CPUS);

/// Hart dedicated to RealTime-priority cells.  RT tasks are enqueued here by
/// `push_ready` and never stolen (Phase 03 steal filter excludes RT).
pub const HART_RT: usize = 1;

/// Set to `true` by each secondary hart once its trap vector and timer are ready.
/// Hart 0's bounded wait reads this via `Acquire` to observe all preceding stores.
pub static HART_ONLINE: [AtomicBool; MAX_HARTS] = [const { AtomicBool::new(false) }; MAX_HARTS];

/// Monotonic switch-completion epochs for root-retirement quiescence. A retiring
/// generation cannot release its CellId slot until every requested hart has
/// switched to a different context and published that completion.
/// Per-hart invalidation epochs: a requester publishes a target, the target
/// performs the local flush on its way through the trap path and publishes
/// completion. Only then may the requester recycle a tag or release frames — a
/// firmware call returning is not evidence that another hart stopped using the
/// translation (phase 02 slice 3).
static TLB_FLUSH_REQUEST: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];
static TLB_FLUSH_COMPLETE: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Deliver the kernel's cross-hart IPI to `hart_id`.
///
/// The requester only records an epoch; what makes the target *act* is this
/// interrupt. Every path that needs a remote hart to do work (invalidate its
/// TLB, switch out of a retiring context) goes through here so the delivery
/// mechanism has one definition per architecture.
#[inline]
fn send_ipi(hart_id: usize) {
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    {
        // Boards with no software-interrupt transport retain the request epoch.
        #[cfg(target_arch = "aarch64")]
        if !<hal::aarch64::backend::Board as hal::aarch64::backend::ArmIrqController>::HAS_IPI {
            return;
        }
        if hart_id >= MAX_HARTS || hart_id == crate::task::hart_local::current_hart_id() {
            return;
        }
        #[cfg(target_arch = "riscv64")]
        if let Some((mask, base)) = logical_sbi_target(hart_id) {
            let _ = hal::common::sbi::sbi_send_ipi(mask, base);
        }
        // AArch64: the board's interrupt controller raises the IPI (a GIC SGI).
        #[cfg(target_arch = "aarch64")]
        if let Some(cpu) = crate::task::hart_local::physical_cpu_for(hart_id) {
            <hal::aarch64::backend::Board as hal::aarch64::backend::ArmIrqController>::send_ipi(
                cpu as u32,
            );
        }
    }
    #[cfg(all(target_arch = "x86_64", not(all(test, not(target_os = "none")))))]
    if hart_id < MAX_HARTS {
        // No completion is forged on delivery failure. Keep this path free of
        // locks/logging because a deferred-release requester may own a lock.
        let _ = hal::x86_64::apic::send_ipi(hart_id);
    }
    // Boards with no software-interrupt transport retain the request epoch.
    #[cfg(not(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        all(target_arch = "x86_64", not(all(test, not(target_os = "none"))))
    )))]
    let _ = hart_id;
}

/// Ask `hart_id` to invalidate its local TLB and return the epoch it must publish.
pub fn request_tlb_flush(hart_id: usize) -> usize {
    if hart_id >= MAX_HARTS {
        return 0;
    }
    let epoch = TLB_FLUSH_REQUEST[hart_id].fetch_add(1, Ordering::AcqRel) + 1;
    send_ipi(hart_id);
    epoch
}

/// Is `epoch` known complete on `hart_id`? Epoch 0 means "nothing requested".
pub fn tlb_flush_completed(hart_id: usize, epoch: usize) -> bool {
    epoch == 0
        || TLB_FLUSH_COMPLETE
            .get(hart_id)
            .is_some_and(|complete| complete.load(Ordering::Acquire) >= epoch)
}

/// Service one captured request. A newer request arriving during the local
/// invalidate must NOT be acknowledged by that invalidate.
pub fn service_tlb_flush(hart_id: usize) -> usize {
    let (epoch, _advanced) = service_tlb_flush_captured(hart_id);
    #[cfg(all(feature = "test-hooks", not(target_arch = "x86_64")))]
    if _advanced {
        log::info!(
            "[selftest] TLB-ACK: stage=remote-flush-completed hart={} epoch={}",
            hart_id,
            epoch
        );
    }
    epoch
}

/// [`service_tlb_flush`] for spin loops that may already own other locks
/// (lock acquisition, cross-hart fixture waits): lock-free and log-free.
///
/// Every hart, hart 0 included, is a shootdown target, so a hart spinning with
/// interrupts masked on progress that a peer makes only after this hart's
/// acknowledgement must answer here, or both stall until the peer's bounded
/// probe fails.
#[inline]
pub fn answer_tlb_flush_in_spin(hart_id: usize) {
    let _ = service_tlb_flush_captured(hart_id);
}

/// Returns the captured request epoch and whether this call published it.
fn service_tlb_flush_captured(hart_id: usize) -> (usize, bool) {
    if hart_id >= MAX_HARTS {
        return (0, false);
    }
    let epoch = TLB_FLUSH_REQUEST[hart_id].load(Ordering::Acquire);
    if TLB_FLUSH_COMPLETE[hart_id].load(Ordering::Acquire) < epoch {
        crate::memory::paging::tlb_flush_all();
        return (
            epoch,
            publish_completion(&TLB_FLUSH_COMPLETE[hart_id], epoch),
        );
    }
    (epoch, false)
}

/// Publish `epoch` as complete without ever lowering `complete`; returns
/// whether this call advanced it.
///
/// The service path can be re-entered by an IPI between its request snapshot
/// and this publication (an x86 waiter spins with interrupts enabled), so the
/// nested call may already have published a newer epoch that a plain store of
/// the older snapshot would retract. Release orders the local invalidate
/// before the requester's Acquire observation.
fn publish_completion(complete: &AtomicUsize, epoch: usize) -> bool {
    complete.fetch_max(epoch, Ordering::Release) < epoch
}

/// Test view of the last invalidation epoch `hart_id` confirmed.
#[cfg(feature = "test-hooks")]
pub fn tlb_flush_complete_epoch(hart_id: usize) -> usize {
    TLB_FLUSH_COMPLETE
        .get(hart_id)
        .map_or(0, |complete| complete.load(Ordering::Acquire))
}

/// Does this hart owe the requester an invalidation completion?
pub fn tlb_flush_pending(hart_id: usize) -> bool {
    hart_id < MAX_HARTS
        && TLB_FLUSH_REQUEST[hart_id].load(Ordering::Acquire)
            > TLB_FLUSH_COMPLETE[hart_id].load(Ordering::Acquire)
}

/// Epochs one requester asked a set of remote harts for: one slot per logical
/// hart, 0 = not asked (epoch 0 is never a real request).
///
/// Broadcast-then-wait contract (plan 261103-2218 phase 07b): every request of
/// the set is published and its IPI sent before the requester waits on any
/// answer, so the targets work concurrently and the set completes in about one
/// round trip, not one per hart. The set is fixed when it is built: a hart that
/// comes online later was not asked and is not waited for (it joins only after
/// its own bring-up flush handshake). Completion is per hart and monotone
/// (`publish_completion` never lowers an epoch), so a hart found complete
/// stays complete; the set is complete when no slot is outstanding.
#[derive(Clone, Copy)]
pub struct RemoteEpochs {
    epochs: [usize; MAX_HARTS],
}

impl RemoteEpochs {
    /// Ask each of `targets` once through `request` (which publishes the epoch
    /// and delivers the IPI) and record the epoch it must confirm. Ids at or
    /// above `MAX_HARTS` are ignored.
    pub fn broadcast(
        targets: impl IntoIterator<Item = usize>,
        mut request: impl FnMut(usize) -> usize,
    ) -> Self {
        let mut epochs = [0; MAX_HARTS];
        for hart in targets {
            if let Some(slot) = epochs.get_mut(hart) {
                *slot = request(hart);
            }
        }
        Self { epochs }
    }

    /// The lowest-numbered asked hart whose epoch `completed` does not confirm
    /// yet, with that epoch; `None` once the whole set has confirmed.
    pub fn first_outstanding(
        &self,
        mut completed: impl FnMut(usize, usize) -> bool,
    ) -> Option<(usize, usize)> {
        self.epochs
            .iter()
            .enumerate()
            .find(|&(hart, &epoch)| epoch != 0 && !completed(hart, epoch))
            .map(|(hart, &epoch)| (hart, epoch))
    }

    /// How many harts were asked.
    pub fn asked(&self) -> usize {
        self.epochs.iter().filter(|epoch| **epoch != 0).count()
    }
}

/// Broadcast a TLB invalidation request to `targets` (see [`RemoteEpochs`]).
pub fn request_tlb_flush_set(targets: impl IntoIterator<Item = usize>) -> RemoteEpochs {
    RemoteEpochs::broadcast(targets, request_tlb_flush)
}

/// The first hart of `set` that has not confirmed its invalidation epoch.
pub fn tlb_flush_set_outstanding(set: &RemoteEpochs) -> Option<(usize, usize)> {
    set.first_outstanding(tlb_flush_completed)
}

/// The logical harts that completed kernel bring-up, the boot hart included.
///
/// Every remote-invalidation and flush-epoch target set is built from this
/// iterator, so a hart missing here keeps stale translations unasked.
pub fn online_harts() -> impl Iterator<Item = usize> {
    (0..MAX_HARTS).filter(|hart| HART_ONLINE[*hart].load(Ordering::Acquire))
}

/// Publish the boot hart (logical 0) as online on every architecture.
///
/// Called by `task::init` right after hart 0's hart-local state is installed,
/// before any secondary starts: from then on a secondary's shootdown names hart
/// 0, which services the request from its trap/IPI path like any other hart.
pub fn publish_boot_hart_online() {
    HART_ONLINE[0].store(true, Ordering::Release);
}

static RETIRE_SWITCH_REQUEST: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];
static RETIRE_SWITCH_COMPLETE: [AtomicUsize; MAX_HARTS] =
    [const { AtomicUsize::new(0) }; MAX_HARTS];

#[cfg(target_arch = "x86_64")]
static RETIRE_SWITCH_INFLIGHT: [AtomicUsize; MAX_HARTS] =
    [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Capture before scheduler selection: a later request may concern the incoming
/// task and must wait for that task's next switch-away.
#[cfg(target_arch = "x86_64")]
pub fn capture_retirement_boundary(hart: usize) {
    RETIRE_SWITCH_INFLIGHT[hart].store(
        RETIRE_SWITCH_REQUEST[hart].load(Ordering::Acquire),
        Ordering::Release,
    );
}

#[cfg(target_arch = "x86_64")]
pub fn complete_captured_retirement_boundary(hart: usize) {
    let epoch = RETIRE_SWITCH_INFLIGHT[hart].swap(0, Ordering::AcqRel);
    RETIRE_SWITCH_COMPLETE[hart].fetch_max(epoch, Ordering::Release);
}

/// Test-hooks: how many preemption requests were pended for each logical hart.
///
/// The RT-wake fixture needs the *decision* observable, not the interrupt: it
/// asserts that consuming a message pends exactly one for the sender's target
/// hart, that a stale delivery token pends none, and that the decision follows the
/// target hart's running priority rather than the waking hart's.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static PREEMPT_PENDS: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[inline]
pub(crate) fn note_preempt_pend(hart_id: usize) {
    if hart_id < MAX_HARTS {
        PREEMPT_PENDS[hart_id].fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[inline]
pub(crate) fn preempt_pends_for(hart_id: usize) -> usize {
    PREEMPT_PENDS
        .get(hart_id)
        .map(|count| count.load(Ordering::Acquire))
        .unwrap_or(0)
}

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
pub(crate) fn reset_preempt_pends() {
    for count in PREEMPT_PENDS.iter() {
        count.store(0, Ordering::Release);
    }
}

/// Request that `hart_id` schedules through a retirement boundary and return
/// the epoch that its incoming context must complete.
pub fn request_retirement_switch(hart_id: usize) -> usize {
    if hart_id >= MAX_HARTS {
        return 0;
    }
    let epoch = RETIRE_SWITCH_REQUEST[hart_id].fetch_add(1, Ordering::AcqRel) + 1;
    #[cfg(feature = "test-hooks")]
    log::info!(
        "[selftest] SMP-RETIREMENT: stage=remote-switch-requested hart={} epoch={}",
        hart_id,
        epoch
    );
    send_ipi(hart_id);
    epoch
}

/// Publish the requested epoch from the incoming side of `Context::switch`.
///
/// The release store is deliberately after the raw context switch has changed
/// stacks: an IPI/trap entry only proves that the outgoing task entered the
/// kernel, while this proves that its saved context no longer executes.
pub fn complete_retirement_switch(hart_id: usize) {
    if hart_id < MAX_HARTS {
        let epoch = RETIRE_SWITCH_REQUEST[hart_id].load(Ordering::Acquire);
        let completed = RETIRE_SWITCH_COMPLETE[hart_id].load(Ordering::Acquire);
        if completed < epoch {
            RETIRE_SWITCH_COMPLETE[hart_id].store(epoch, Ordering::Release);
            #[cfg(feature = "test-hooks")]
            log::info!(
                "[selftest] SMP-RETIREMENT: stage=remote-switch-completed hart={} epoch={}",
                hart_id,
                epoch
            );
        }
    }
}

/// Does `hart_id` still owe a completion for a requested retirement switch?
pub fn retirement_switch_pending(hart_id: usize) -> bool {
    hart_id < MAX_HARTS
        && RETIRE_SWITCH_REQUEST[hart_id].load(Ordering::Acquire)
            > RETIRE_SWITCH_COMPLETE[hart_id].load(Ordering::Acquire)
}

pub fn retirement_switch_completed(hart_id: usize, epoch: usize) -> bool {
    epoch == 0
        || RETIRE_SWITCH_COMPLETE
            .get(hart_id)
            .is_some_and(|complete| complete.load(Ordering::Acquire) >= epoch)
}

/// Publish the RV64 boot hart's numbering into [`CPU_TOPOLOGY`]: the boot hart
/// is logical 0 and the hart it displaced takes its number
/// ([`topology::boot_swapped_physical`]). Called once by `kmain` on the boot
/// hart, before any secondary is started.
#[cfg(target_arch = "riscv64")]
pub fn set_boot_physical_hart(physical_hart: usize) {
    assert!(
        physical_hart < MAX_HARTS,
        "unsupported RV64 boot hart {physical_hart}"
    );
    for logical in 0..MAX_HARTS {
        assert!(
            CPU_TOPOLOGY.publish(
                logical,
                topology::boot_swapped_physical(physical_hart, logical)
            ),
            "RV64 boot hart {physical_hart} conflicts with the published topology"
        );
    }
}

#[cfg(target_arch = "riscv64")]
pub fn boot_physical_hart() -> Option<usize> {
    logical_to_physical(0)
}

/// Physical id (RV64 hart id, x86 LAPIC ID, AArch64 CPU index) of
/// `logical_hart`, if [`CPU_TOPOLOGY`] maps it.
pub fn logical_to_physical(logical_hart: usize) -> Option<usize> {
    CPU_TOPOLOGY.physical(logical_hart)
}

/// Logical hart that owns `physical_hart`, if [`CPU_TOPOLOGY`] maps it.
pub fn physical_to_logical(physical_hart: usize) -> Option<usize> {
    CPU_TOPOLOGY.logical(physical_hart)
}

#[cfg(target_arch = "riscv64")]
pub fn logical_sbi_target(logical_hart: usize) -> Option<(usize, usize)> {
    logical_to_physical(logical_hart).map(|physical| (1, physical))
}

/// Return the one remote RV64 hart this boot can have, if it is online.
///
/// Every hart, the boot hart included, publishes readiness in `HART_ONLINE`
/// with Release. RV64 runs at most two harts (`MAX_HARTS = 2`), so the remote
/// hart is the RT hart for hart 0 and hart 0 for everyone else.
#[cfg(target_arch = "riscv64")]
pub fn remote_online_sbi_target() -> Option<(usize, usize)> {
    const _: () = assert!(MAX_HARTS == 2);
    let current = crate::task::hart_local::current_hart_id();
    let remote = if current == 0 { HART_RT } else { 0 };
    let online = HART_ONLINE[remote].load(Ordering::Acquire);
    online.then(|| logical_sbi_target(remote)).flatten()
}

/// Return the harts that completed kernel bring-up for the current boot.
///
/// Hart zero is the active boot hart and is counted unconditionally, which
/// equals `online_harts().count()` once `task::init` has published it; each
/// secondary contributes only after publishing `HART_ONLINE`, so test evidence
/// cannot confuse configured SMP capacity with observed runtime availability.
/// `TelemetrySnapshot` reports it as `online_cpus`.
pub(crate) fn online_hart_count() -> usize {
    1 + HART_ONLINE
        .iter()
        .skip(1)
        .filter(|online| online.load(Ordering::Acquire))
        .count()
}

/// How many 10 ms ticks hart 0 waits for each secondary to come online before
/// logging a warning and continuing single-hart.  500 ms is generous for QEMU
/// and for a firmware PSCI call that has to power a core up.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
const SECONDARY_BOOT_TIMEOUT_TICKS: usize = 50;

/// Called by hart 0 **after** `task::init()` to bring secondary harts online.
///
/// Each secondary is started via SBI HSM `hart_start`.  Hart 0 then spins
/// (bounded) waiting for each secondary to set `HART_ONLINE[hart_id]`.
/// If a secondary fails to start or times out, a warning is logged and the
/// system continues single-hart — graceful degradation, never a panic.
#[cfg(target_arch = "riscv64")]
pub fn start_secondaries() {
    use crate::task::stack::Stack;
    use crate::task::STACK_PAGES;
    use hal::common::sbi::{sbi_hart_get_status, sbi_hart_start, sbi_rfence_available};

    let Some(boot_physical) = boot_physical_hart() else {
        log::warn!("[smp] boot physical hart was not published");
        return;
    };
    log::info!("[smp] physical {} -> logical 0 boot", boot_physical);

    match sbi_rfence_available() {
        Ok(true) => {}
        Ok(false) => {
            log::warn!("[smp] SBI RFENCE unavailable — keeping Cellos single-hart");
            return;
        }
        Err(error) => {
            log::warn!(
                "[smp] SBI RFENCE probe failed (err={}) — keeping Cellos single-hart",
                error
            );
            return;
        }
    }

    extern "C" {
        // Physical asm label defined in hal/arch/riscv/src/rv64/boot.rs.
        // Runs bare (SATP=0); no relocation or BSS clear.
        fn _secondary_entry();
    }

    for (hart_id, online) in HART_ONLINE.iter().enumerate().skip(1) {
        let Some(physical_hart) = logical_to_physical(hart_id) else {
            log::warn!("[smp] logical hart {} has no physical mapping", hart_id);
            continue;
        };
        // Allocate a dedicated kernel stack for this hart.  Leak it — it lives
        // for the entire lifetime of the hart.
        let stack = match Stack::new_kernel(STACK_PAGES) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[smp] hart {} stack alloc failed: {:?}", hart_id, e);
                continue;
            }
        };
        let stack_top = stack.top;
        core::mem::forget(stack);

        let Ok(state) = sbi_hart_get_status(physical_hart) else {
            log::warn!(
                "[smp] physical hart {} HSM status unavailable",
                physical_hart
            );
            continue;
        };
        log::info!(
            "[smp] physical {} -> logical {} HSM state = {}",
            physical_hart,
            hart_id,
            state
        );
        if state != 1 {
            log::warn!("[smp] physical hart {} is not HSM STOPPED", physical_hart);
            continue;
        }

        // SAFETY: _secondary_entry is a physical-address asm label; the kernel
        // is loaded at 0x80200000 with slide=0 so physical == virtual.
        // stack_top is the usable top of a freshly-allocated kernel stack.
        // SAFETY: casting function pointer to integer — use double-cast through
        // *const () to avoid the "direct cast of function item" lint.
        let entry_paddr = _secondary_entry as *const () as usize;
        match sbi_hart_start(physical_hart, entry_paddr, stack_top) {
            Ok(()) => log::info!(
                "[smp] hart {} start requested (entry={:#x})",
                physical_hart,
                entry_paddr
            ),
            Err(e) => {
                log::warn!("[smp] hart {} SBI hart_start failed: err={}", hart_id, e);
                continue;
            }
        }

        // Bounded spin: wait for the secondary to signal it is online.
        let deadline = crate::task::system_ticks() + SECONDARY_BOOT_TIMEOUT_TICKS;
        loop {
            if online.load(Ordering::Acquire) {
                log::info!("[smp] hart {} online, parked", hart_id);
                break;
            }
            if crate::task::system_ticks() >= deadline {
                log::warn!(
                    "[smp] hart {} did not come online in time — continuing single-hart",
                    hart_id
                );
                break;
            }
            core::hint::spin_loop();
        }
    }
}

/// Boot state for one secondary core, published before `PSCI_CPU_ON`.
///
/// Written once by the boot hart and read by a core whose caches are off, so
/// the write is followed by a clean to the point of coherency. `static mut`
/// with a single writer and no reader until the core is started is the whole
/// synchronisation argument.
#[cfg(target_arch = "aarch64")]
static mut SECONDARY_CONTEXT: [hal::aarch64::boot::SecondaryContext; MAX_HARTS] =
    [hal::aarch64::boot::SecondaryContext {
        hart_id: 0,
        stack_top: 0,
        mair: 0,
        tcr: 0,
        ttbr0: 0,
        sctlr: 0,
        el2_host: 0,
    }; MAX_HARTS];

/// Called by hart 0 **after** `task::init()` to bring secondary cores online.
///
/// How a core comes online is the board backend's choice
/// ([`SecondaryStart`](hal::aarch64::backend::SecondaryStart)): through PSCI,
/// or not at all, in which case the board stays single-hart and says why.
#[cfg(target_arch = "aarch64")]
pub fn start_secondaries() {
    use hal::aarch64::backend::{ArmPlatform, Board, SecondaryStart};
    match <Board as ArmPlatform>::SECONDARY_START {
        SecondaryStart::Psci => start_psci_secondaries(),
        SecondaryStart::None(reason) => log::info!("[smp] {}", reason),
    }
}

/// AArch64 starts a core through firmware (`PSCI_CPU_ON` over the conduit the
/// device tree declares). Everything the new core needs — its stack, the boot
/// core's live translation regime, the hardening bits in `SCTLR_EL1` — travels
/// in one context block it reads with the MMU still off; `_secondary_entry`
/// installs the regime and calls [`smp_aarch64_secondary_main`].
///
/// Bounded wait, graceful degradation, never a panic: a core that does not come
/// online within the deadline leaves the system single-hart with a warning.
#[cfg(target_arch = "aarch64")]
fn start_psci_secondaries() {
    use crate::boot::firmware_smp;
    use crate::task::stack::Stack;
    use crate::task::STACK_PAGES;

    let at_el2 = hal::aarch64::el2::is_el2();
    // Which instruction carries a PSCI call: the firmware tree's answer when
    // there is one, otherwise what the architecture allows — with EL3 the
    // conduit is EL3 firmware's (`smc`); without EL3 an `smc` is undefined and
    // the only conduit that can exist is the hypervisor's (`hvc`).
    let conduit = match firmware_smp::psci_conduit() {
        Some(declared) => declared,
        None => {
            let inferred = hal::aarch64::psci::inferred_conduit();
            log::info!(
                "[smp] no firmware tree declares PSCI; EL3 {} — inferring conduit {:?}",
                if hal::aarch64::psci::el3_implemented() {
                    "implemented"
                } else {
                    "absent"
                },
                inferred
            );
            inferred
        }
    };
    // A conduit that cannot work is reported, never attempted: `smc` without EL3
    // and `hvc` from EL2 both trap as undefined instructions.
    let usable = match (conduit, at_el2) {
        // EL3 firmware answers an SMC from either exception level.
        (hal::aarch64::psci::Conduit::Smc, _) => true,
        // An HVC from EL1 traps to EL2, where the PSCI implementation lives; an
        // HVC from EL2 would target EL3, which by construction has no PSCI here.
        (hal::aarch64::psci::Conduit::Hvc, false) => true,
        (hal::aarch64::psci::Conduit::Hvc, true) => false,
    };
    if !usable {
        log::warn!(
            "[smp] the kernel runs at EL2 and its conduit is HVC, which would target EL3 — \
             keeping Cellos single-hart"
        );
        return;
    }
    log::info!(
        "[smp] PSCI conduit {:?}, kernel at {}",
        conduit,
        if at_el2 { "EL2" } else { "EL1" }
    );
    // The logical hart equals the physical CPU index on every AArch64 platform
    // this kernel supports, so the boot core has to *be* CPU 0 for the mapping
    // to hold; anything else is reported rather than half-applied.
    let boot_cpu = crate::task::hart_local::physical_cpu_index();
    if boot_cpu != 0 {
        log::warn!(
            "[smp] boot core reports CPU index {} (expected 0) — keeping Cellos single-hart",
            boot_cpu
        );
        return;
    }
    // How many cores to try. The tree is authoritative when it says anything;
    // with no tree the firmware's own answer to `CPU_ON` is what tells us
    // whether a core exists, so every hart this kernel models is attempted and
    // a refusal ends the search.
    let described = firmware_smp::cpu_count();
    if described == 1 {
        log::info!("[smp] firmware tree describes 1 CPU — single-hart");
        return;
    }
    let attempt = if described == 0 {
        MAX_HARTS
    } else {
        MAX_HARTS.min(described)
    };
    match hal::aarch64::psci::version(conduit) {
        Some(version) => log::info!(
            "[smp] PSCI {}.{} over {:?}, {} CPU(s) described",
            version >> 16,
            version & 0xFFFF,
            conduit,
            described
        ),
        None => {
            log::warn!("[smp] firmware does not answer PSCI_VERSION — keeping Cellos single-hart");
            return;
        }
    }

    extern "C" {
        /// Physical entry point defined in hal/arch/arm/src/aarch64/boot.rs.
        /// Runs with the MMU off; installs the boot core's translation regime.
        fn _secondary_entry();
    }
    let entry = _secondary_entry as *const () as usize;
    let kernel_root = match *crate::memory::paging::KERNEL_ROOT.lock() {
        Some(root) => root,
        None => {
            log::warn!("[smp] kernel page tables are not published — keeping Cellos single-hart");
            return;
        }
    };

    for hart_id in 1..attempt {
        // The core is still off: this mapping has to exist before it reads it.
        if !crate::task::hart_local::publish_physical_cpu(hart_id, hart_id) {
            log::warn!(
                "[smp] hart {} conflicts with the published topology — not started",
                hart_id
            );
            continue;
        }
        let stack = match Stack::new_kernel(STACK_PAGES) {
            Ok(stack) => stack,
            Err(error) => {
                log::warn!("[smp] hart {} stack alloc failed: {:?}", hart_id, error);
                continue;
            }
        };
        let stack_top = stack.top;
        // Leaked on purpose: a hart's stack lives as long as the hart does.
        core::mem::forget(stack);

        let context = hal::aarch64::boot::SecondaryContext::for_hart(
            hart_id as u64,
            stack_top as u64,
            kernel_root as u64,
        );
        // SAFETY: single writer (hart 0, before the core exists), and no reader
        // until `PSCI_CPU_ON` succeeds.
        let context_addr = unsafe {
            let slot = core::ptr::addr_of_mut!(SECONDARY_CONTEXT[hart_id]);
            slot.write(context);
            slot as usize
        };
        // The core reads this with caches off: publish it to the point of
        // coherency, and order that before the firmware call.
        hal::aarch64::cache::clean_data_cache_range(
            context_addr,
            core::mem::size_of::<hal::aarch64::boot::SecondaryContext>(),
        );
        core::sync::atomic::fence(Ordering::SeqCst);

        // Aff0 is the CPU index on every supported platform; the firmware call
        // takes the full MPIDR, which those platforms build from it alone.
        let mpidr = hart_id as u64;
        match hal::aarch64::psci::cpu_on(conduit, mpidr, entry, context_addr as u64) {
            Ok(()) => log::info!(
                "[smp] hart {} start requested (cpu={} entry={:#x})",
                hart_id,
                hart_id,
                entry
            ),
            Err(status) => {
                // With nothing describing the CPU population, "no such core" is
                // the expected answer on a smaller machine — and it also ends the
                // search, because cores are numbered from zero.
                // PSCI 1.0: -2 INVALID_PARAMETERS, -7 NOT_PRESENT.
                let missing = described == 0 && (status == -2 || status == -7);
                if missing {
                    log::info!(
                        "[smp] hart {}: firmware reports no such core — single-hart",
                        hart_id
                    );
                    break;
                }
                log::warn!(
                    "[smp] hart {} PSCI CPU_ON refused: {} ({})",
                    hart_id,
                    hal::aarch64::psci::status_name(status),
                    status
                );
                continue;
            }
        }

        let deadline = crate::task::system_ticks() + SECONDARY_BOOT_TIMEOUT_TICKS;
        loop {
            if HART_ONLINE[hart_id].load(Ordering::Acquire) {
                log::info!("[smp] hart {} online, parked", hart_id);
                break;
            }
            if crate::task::system_ticks() >= deadline {
                log::warn!(
                    "[smp] hart {} did not come online in time — continuing single-hart",
                    hart_id
                );
                break;
            }
            core::hint::spin_loop();
        }
    }
}

/// Prove the hart that just came online actually takes the kernel's IPI.
///
/// The one cross-hart path a boot log cannot witness on its own: the request is
/// delivered by a GIC SGI, the target answers from *its* trap path, and this
/// hart's bounded wait ends only when that epoch lands. Without it, a hart that
/// came online but is deaf to the IPI looks exactly like a healthy one — and
/// every later remote invalidation would silently fall back to the retained-frame
/// path. Bounded and non-fatal: a failed probe is reported, it does not stop the
/// boot.
#[cfg(target_arch = "aarch64")]
pub fn run_ipi_selftest() {
    let Some(remote) = online_harts().find(|hart| *hart != 0) else {
        log::info!("[selftest] SMP-IPI: skipped, no remote hart online");
        return;
    };
    let epoch = request_tlb_flush(remote);
    let deadline = crate::task::system_ticks() + SECONDARY_BOOT_TIMEOUT_TICKS;
    let mut acknowledged = false;
    while crate::task::system_ticks() < deadline {
        if tlb_flush_completed(remote, epoch) {
            acknowledged = true;
            break;
        }
        core::hint::spin_loop();
    }
    if acknowledged {
        log::info!("[selftest] SMP-IPI: PASS hart={} epoch={}", remote, epoch);
    } else {
        log::error!(
            "[selftest] SMP-IPI: FAIL hart={} epoch={} (no acknowledgement)",
            remote,
            epoch
        );
    }
}

/// No-op on targets with one CPU.
#[cfg(not(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
)))]
pub fn start_secondaries() {}

// x86 AP bring-up handshake (phase 07b: every MP CPU up to `MAX_HARTS`).
//
// The BSP starts APs one at a time, in Limine MP order, each through the same
// four-step exchange; the next AP is released only after the previous one is
// online, so each handshake word below names at most one AP in flight and is
// a logical id (0 = none yet), never a flag that a later AP could confuse with
// an earlier one's:
//   1. AP: own flush proof on the BSP (request_tlb_flush(0) acknowledged), then
//      `X86_READY = logical` (or `X86_AP_ROOT_REFUSED = logical` and park).
//   2. BSP: flush proof on the AP (request_tlb_flush(logical) acknowledged),
//      then `X86_QUALIFIED = logical`.
//   3. AP: `HART_ONLINE[logical] = true`.
//   4. BSP: one catch-up flush acknowledged by the AP (it was outside every
//      shootdown target set until step 3), then the next AP.
// A refusal (root operation) or a start failure (stack, topology) ends
// bring-up, so the online set is always the contiguous range 0..online and
// the RT hart (1) is online whenever any AP is. An AP that was released but
// never answers a step within its bounded budget panics the boot
// (`x86_bounded_wait`): fail-closed, never a silently missing CPU.
#[cfg(target_arch = "x86_64")]
static X86_READY: AtomicUsize = AtomicUsize::new(0);
/// Highest logical AP the BSP has qualified (step 2); monotone, because APs
/// are qualified in increasing logical order.
#[cfg(target_arch = "x86_64")]
static X86_QUALIFIED: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_arch = "x86_64")]
static X86_BOOT_COMPLETE: AtomicBool = AtomicBool::new(false);
/// Logical id of the AP that answered step 1 with a refusal instead of
/// `X86_READY`: it cannot enter the x86 root operation the BSP latched, so it
/// parks and never goes online.
#[cfg(target_arch = "x86_64")]
static X86_AP_ROOT_REFUSED: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_arch = "x86_64")]
static X86_PREEMPT: [AtomicBool; MAX_HARTS] = [const { AtomicBool::new(false) }; MAX_HARTS];

/// Budget of one bounded handshake wait (`x86_bounded_wait`).
#[cfg(target_arch = "x86_64")]
const X86_WAIT_STEPS: usize = 100_000_000;

#[cfg(target_arch = "x86_64")]
pub fn x86_substrate_qualified() -> bool {
    // Every online AP passed step 2 before it published `HART_ONLINE`.
    X86_BOOT_COMPLETE.load(Ordering::Acquire)
        && (online_harts().count() == 1 || X86_QUALIFIED.load(Ordering::Acquire) != 0)
}

#[cfg(target_arch = "x86_64")]
pub fn x86_scheduler_ready() -> bool {
    X86_BOOT_COMPLETE.load(Ordering::Acquire)
}

#[cfg(target_arch = "x86_64")]
pub fn request_preemption(hart: usize) {
    if hart < MAX_HARTS {
        X86_PREEMPT[hart].store(true, Ordering::Release);
        send_ipi(hart);
    }
}

#[cfg(target_arch = "x86_64")]
pub fn take_preemption(hart: usize) -> bool {
    hart < MAX_HARTS && X86_PREEMPT[hart].swap(false, Ordering::AcqRel)
}

/// What the BSP hands one AP through `LimineMpInfo.extra_argument`; read only
/// by `vi_x86_secondary_entry` (field offsets 0/8/16/24 are its contract).
/// `AtomicUsize` has the layout of `usize`, so the asm reads plain words.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct X86SecondaryContext {
    stack_top: AtomicUsize,
    kernel_cr3: AtomicUsize,
    logical: AtomicUsize,
    lapic_id: AtomicUsize,
}

/// One context per logical AP slot, so no AP's handoff is ever rewritten for
/// another. Each slot is written once by the BSP before the Release store to
/// that AP's Limine `goto_address`.
#[cfg(target_arch = "x86_64")]
static X86_SECONDARY: [X86SecondaryContext; MAX_HARTS] = [const {
    X86SecondaryContext {
        stack_top: AtomicUsize::new(0),
        kernel_cr3: AtomicUsize::new(0),
        logical: AtomicUsize::new(0),
        lapic_id: AtomicUsize::new(0),
    }
}; MAX_HARTS];

// Limine enters with IF clear on its own permanent bootloader stack. Read all
// handoff fields before the root change, adopt our dedicated stack before Rust
// touches any stack, and never return to bootloader state. Both roots retain
// the loaded kernel mapping and HHDM.
#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".global vi_x86_secondary_entry",
    ".type vi_x86_secondary_entry,@function",
    "vi_x86_secondary_entry:",
    "endbr64",
    "cli",
    "mov rax, [rdi + 24]",
    "mov rdx, [rax]",
    "mov rcx, [rax + 8]",
    "mov rdi, [rax + 16]",
    "mov rsi, [rax + 24]",
    "mov cr3, rcx",
    "mov rsp, rdx",
    "and rsp, -16",
    "xor rbp, rbp",
    "mov rdx, rcx",
    "call {entry}",
    "ud2",
    entry = sym x86_secondary_main,
);

#[cfg(target_arch = "x86_64")]
fn x86_bounded_wait(done: impl FnMut() -> bool) {
    x86_bounded_wait_steps(X86_WAIT_STEPS, done);
}

#[cfg(target_arch = "x86_64")]
fn x86_bounded_wait_steps(steps: usize, mut done: impl FnMut() -> bool) {
    // A bounded instruction budget does not depend on timer interrupts being
    // delivered to the waiting CPU. Fail closed once a callback was released.
    for _ in 0..steps {
        if done() {
            return;
        }
        core::hint::spin_loop();
    }
    panic!("[x86-smp] active AP safety handshake timed out");
}

/// x86 IDT fixture only: it runs inside `task::init`, before
/// `start_secondaries`, on the BSP alone and never returns. Publish the same
/// uniprocessor boot-complete state that `start_secondaries` publishes when no
/// AP exists, so `yield_cpu` schedules the fixture's Ring-3 tasks. `task::init`
/// has already published the BSP in `HART_ONLINE`.
#[cfg(all(target_arch = "x86_64", feature = "x86-idt-cpl3-test"))]
pub(crate) fn x86_idt_fixture_uniprocessor_ready() {
    X86_BOOT_COMPLETE.store(true, Ordering::Release);
}

/// `processor_ids=` of the topology line: the BSP's LAPIC ID, then each online
/// AP's, in logical order.
#[cfg(target_arch = "x86_64")]
struct X86OnlineApicIds {
    bsp_id: u32,
}

#[cfg(target_arch = "x86_64")]
impl core::fmt::Display for X86OnlineApicIds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.bsp_id)?;
        for hart in online_harts().filter(|hart| *hart != 0) {
            if let Some(id) = logical_to_physical(hart) {
                write!(f, ";{}", id)?;
            }
        }
        Ok(())
    }
}

/// Bring every Limine MP CPU online, up to `MAX_HARTS` logical harts.
///
/// Logical 0 is the BSP; APs take logical 1, 2, … in Limine MP order (1 is
/// the RT hart, the rest are normal harts). APs are started one at a time
/// through the handshake documented at `X86_READY`; a CPU beyond `MAX_HARTS`
/// is logged `cpu N ignored: MAX_HARTS` and left in Limine's park loop, a
/// LAPIC ID above 255 stays offline (no interrupt remapping, phase 12), and
/// the first AP that refuses root operation or cannot be started ends
/// bring-up with the harts already online.
///
/// `task::quiesce` fails closed above 8 online harts; on x86 it already refuses
/// every multi-hart set (no park hook), so bring-up does not cap below
/// `MAX_HARTS` for it (phase 07b decision D2).
#[cfg(target_arch = "x86_64")]
pub fn start_secondaries() {
    use crate::boot::limine;
    let bsp_id = hal::x86_64::apic::current_id();
    #[cfg(feature = "test-hooks")]
    x86_handoff_fixture();
    let finish_up = || {
        X86_BOOT_COMPLETE.store(true, Ordering::Release);
        log::info!(
            "[x86-smp] topology online=1 processor_ids={} qualified=yes",
            bsp_id
        );
    };
    // Logical 0 is the BSP. `CPU_TOPOLOGY` is the kernel's logical -> LAPIC map;
    // the HAL keeps its own write-once copy (`apic::register_cpu`) because its
    // IPI path cannot read kernel state. Both refuse a LAPIC ID above 255 and a
    // conflicting pair, so either refusal keeps the system uniprocessor.
    if !CPU_TOPOLOGY.publish(0, bsp_id as usize) || !hal::x86_64::apic::register_cpu(0, bsp_id) {
        // UP does not use APIC-directed remote invalidations.
        finish_up();
        return;
    }
    let Some(response) = limine::get_mp_response() else {
        finish_up();
        return;
    };
    let count = response.cpu_count as usize;
    // Response flag bit 0 = Limine enabled x2APIC; it must match this CPU.
    let x2apic_flag = response.flags & 1 != 0;
    if response.flags & !1 != 0
        || x2apic_flag != hal::x86_64::apic::x2apic_active()
        || response.bsp_lapic_id != bsp_id
        || count == 0
        || count > 256
        || !limine::mp_record_valid(response.cpus, count)
    {
        log::warn!("[x86-smp] unsupported MP topology; no AP activated");
        finish_up();
        return;
    }
    // Validate every record before any AP is released: one bad record keeps
    // the whole machine uniprocessor, as before.
    let mut ids = [false; 256];
    let mut processors = [0u32; 256];
    let mut bsp_seen = false;
    let mut secondaries: [*mut limine::LimineMpInfo; 256] = [core::ptr::null_mut(); 256];
    let mut secondary_count = 0;
    for index in 0..count {
        let info = unsafe { *response.cpus.add(index) };
        if !limine::mp_record_valid(info, 1) {
            finish_up();
            return;
        }
        let cpu = unsafe { &*info };
        let id = cpu.lapic_id as usize;
        if id > 255 {
            // Needs interrupt remapping for IOAPIC/MSI destinations (phase 12).
            log::warn!(
                "[x86-smp] cpu apic_id={} offline: needs interrupt remapping",
                cpu.lapic_id
            );
            continue;
        }
        if ids[id]
            || processors[..index].contains(&cpu.processor_id)
            || (cpu.lapic_id != bsp_id && cpu.goto_address.load(Ordering::Acquire) != 0)
        {
            log::warn!("[x86-smp] invalid MP CPU record; no AP activated");
            finish_up();
            return;
        }
        ids[id] = true;
        processors[index] = cpu.processor_id;
        if cpu.lapic_id == bsp_id {
            bsp_seen = true;
        } else {
            secondaries[secondary_count] = info;
            secondary_count += 1;
        }
    }
    if !bsp_seen || secondary_count == 0 {
        finish_up();
        return;
    }
    let kernel_cr3 = hal::domain::kernel_cr3();
    assert!(kernel_cr3 != 0, "AP requires permanent shared kernel root");
    // The BSP must receive each AP's reciprocal proof even if boot entered this
    // routine with interrupts masked. Restore its original state afterwards.
    let flags: usize;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", "sti", out(reg) flags);
    }
    for &info in &secondaries[..secondary_count] {
        let (ap_id, processor_id) = unsafe { ((*info).lapic_id, (*info).processor_id) };
        // The next free logical index; the pair is published before the AP is
        // released, so the AP's own check in `x86_secondary_main` reads a
        // complete mapping.
        let logical = CPU_TOPOLOGY.count();
        if logical >= MAX_HARTS {
            log::warn!(
                "[x86-smp] cpu {} ignored: MAX_HARTS (apic_id={})",
                processor_id,
                ap_id
            );
            continue;
        }
        if !x86_start_ap(info, logical, ap_id, kernel_cr3) {
            break;
        }
    }
    let online = online_hart_count();
    let broadcast_ns = (online > 1).then(x86_broadcast_flush_proof);
    #[cfg(feature = "test-hooks")]
    if online > 1 {
        x86_shootdown_fixture_primary();
    }
    X86_BOOT_COMPLETE.store(true, Ordering::Release);
    if let Some(ack_ns) = broadcast_ns {
        log::info!("[x86-smp] transport IPI-invalidate bidirectional=pass");
        log::info!(
            "[x86-smp] transport IPI-invalidate broadcast targets={} ack_ns={}",
            online - 1,
            ack_ns
        );
    }
    log::info!(
        "[x86-smp] topology online={} processor_ids={} qualified=yes",
        online,
        X86OnlineApicIds { bsp_id }
    );
    if flags & (1 << 9) == 0 {
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack));
        }
    }
}

/// Start one AP as `logical` and run the four-step handshake (see
/// `X86_READY`). Returns `false`, leaving that AP offline, when it cannot be
/// started or refuses root operation; the caller then stops bring-up.
#[cfg(target_arch = "x86_64")]
fn x86_start_ap(
    info: *mut crate::boot::limine::LimineMpInfo,
    logical: usize,
    ap_id: u32,
    kernel_cr3: usize,
) -> bool {
    let stack = match crate::task::stack::Stack::new_kernel(crate::task::STACK_PAGES) {
        Ok(stack) => stack,
        Err(_) => {
            log::warn!(
                "[x86-smp] AP {} stack allocation failed; bring-up stops",
                ap_id
            );
            return false;
        }
    };
    if !CPU_TOPOLOGY.publish(logical, ap_id as usize)
        || !hal::x86_64::apic::register_cpu(logical, ap_id)
    {
        log::warn!(
            "[x86-smp] AP {} refused as logical {}; bring-up stops",
            ap_id,
            logical
        );
        return false;
    }
    let stack_top = stack.top;
    core::mem::forget(stack);
    extern "C" {
        fn vi_x86_secondary_entry();
    }
    let context = &X86_SECONDARY[logical];
    // Relaxed: the Release store to `goto_address` below publishes these.
    context.stack_top.store(stack_top, Ordering::Relaxed);
    context.kernel_cr3.store(kernel_cr3, Ordering::Relaxed);
    context.logical.store(logical, Ordering::Relaxed);
    context.lapic_id.store(ap_id as usize, Ordering::Relaxed);
    unsafe {
        (*info).extra_argument = core::ptr::from_ref(context) as u64;
        (*info).goto_address.store(
            vi_x86_secondary_entry as *const () as usize,
            Ordering::Release,
        );
    }
    // Step 1 answered by this AP (no earlier AP can still write these words:
    // it is online or parked before this one is released).
    x86_bounded_wait(|| {
        X86_READY.load(Ordering::Acquire) == logical
            || X86_AP_ROOT_REFUSED.load(Ordering::Acquire) == logical
    });
    if X86_AP_ROOT_REFUSED.load(Ordering::Acquire) == logical {
        log::warn!(
            "[x86-smp] AP {} refused x86 root operation; it stays offline",
            ap_id
        );
        return false;
    }
    // Step 2: this AP answers the BSP's IPI-driven invalidation.
    let epoch = request_tlb_flush(logical);
    x86_bounded_wait(|| tlb_flush_completed(logical, epoch));
    X86_QUALIFIED.store(logical, Ordering::Release);
    // Step 3.
    x86_bounded_wait(|| HART_ONLINE[logical].load(Ordering::Acquire));
    // Step 4: catch-up invalidation. Until `HART_ONLINE[logical]` was visible,
    // every shootdown (another AP's, or the BSP's own) excluded this AP from its
    // target set, so a translation changed between step 2 and step 3 may still
    // be cached here. One more acknowledged flush, requested only after the
    // BSP observed the AP online, closes that window: every later shootdown
    // targets it.
    let epoch = request_tlb_flush(logical);
    x86_bounded_wait(|| tlb_flush_completed(logical, epoch));
    true
}

/// One broadcast invalidation to every online AP at once, the shape every
/// later shootdown takes (`tlb_shootdown::await_remote_invalidation`).
/// Returns its acknowledgement latency in clock nanoseconds (0 without a
/// clocksource). The APs still spin in their boot-complete wait with IF set.
#[cfg(target_arch = "x86_64")]
fn x86_broadcast_flush_proof() -> usize {
    let started = hal::x86_64::clock::now_ns();
    let requested = request_tlb_flush_set(online_harts().filter(|hart| *hart != 0));
    x86_bounded_wait(|| tlb_flush_set_outstanding(&requested).is_none());
    hal::x86_64::clock::now_ns().saturating_sub(started) as usize
}

#[cfg(target_arch = "x86_64")]
extern "C" fn x86_secondary_main(logical: usize, lapic_id: usize, kernel_cr3: usize) -> ! {
    // Any AP slot below MAX_HARTS is admissible, but only on the CPU whose LAPIC
    // ID the BSP published for it before releasing this one.
    assert!(
        logical != 0 && logical < MAX_HARTS && logical_to_physical(logical) == Some(lapic_id),
        "[x86-smp] AP logical {} lapic {} is not the published topology",
        logical,
        lapic_id
    );
    if !unsafe { hal::x86_64::init_secondary_cpu(logical, lapic_id as u32, kernel_cr3) } {
        panic!("[x86-smp] active AP feature/isolation initialization failed");
    }
    crate::task::hart_local::install(logical);
    // HypervisorCap is system-wide once the BSP latched root operation, so this
    // CPU must enter the same root operation (own HSAVE/VMXON frame) before it
    // may go online. A refused AP parks with IF clear and is never published.
    if crate::cpu_features::has_x86_virt() {
        if let Err(reason) = crate::cpu_features::enter_x86_root_on_current_cpu() {
            log::warn!(
                "[x86-smp] AP {} x86 root operation refused: {}",
                lapic_id,
                reason
            );
            X86_AP_ROOT_REFUSED.store(logical, Ordering::Release);
            loop {
                unsafe {
                    core::arch::asm!("cli", "hlt", options(nomem, nostack));
                }
            }
        }
    }
    // Establish this CPU's own idle activation before enabling delivery.
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    // Per-AP clock state: `init_secondary_cpu` ran the TSC skew check, armed
    // this CPU's LAPIC/TSC-deadline tick and, when the BSP uses kvmclock,
    // registered this vCPU's own pvclock slot (the MSR is per vCPU).
    log::info!(
        "[x86-smp] AP logical={} apic_id={} pvclock={}",
        logical,
        lapic_id,
        if hal::x86_64::clock::kvmclock::registered_area().is_some() {
            "own"
        } else {
            "none"
        }
    );
    // Step 1 (see `X86_READY`).
    let epoch = request_tlb_flush(0);
    x86_bounded_wait(|| tlb_flush_completed(0, epoch));
    X86_READY.store(logical, Ordering::Release);
    x86_bounded_wait(|| X86_QUALIFIED.load(Ordering::Acquire) >= logical);
    // Step 3.
    HART_ONLINE[logical].store(true, Ordering::Release);
    // Later APs are brought up while this one waits here. Each later AP costs
    // the BSP at most four bounded waits, plus the closing broadcast and
    // shootdown fixture, so five budgets per hart slot bound this wait.
    x86_bounded_wait_steps(X86_WAIT_STEPS.saturating_mul(5 * MAX_HARTS), || {
        // The shootdown fixture pairs the BSP with the RT hart only; any other
        // AP just answers its IPIs from here.
        #[cfg(feature = "test-hooks")]
        if logical == HART_RT {
            x86_shootdown_fixture_secondary();
        }
        X86_BOOT_COMPLETE.load(Ordering::Acquire)
    });
    loop {
        crate::task::yield_cpu();
        unsafe {
            core::arch::asm!("sti", "hlt", options(nomem, nostack));
        }
    }
}

// This behavioral fixture caches a genuinely GLOBAL shared translation on the
// AP, changes its binding on the BSP, then forces the AP to contend on the same
// page-table lock with IF clear. Success requires lock-spin local invalidation;
// an ISR-only acknowledgement deadlocks or retains the old binding. A second
// round rebinds the page while the AP, IF still clear, only takes that lock
// *uncontended* — a long masked kernel stretch such as a spawn reading its
// image. The BSP's shootdown must still be acknowledged before its probe
// expires: every acquisition, not only a contended one, is a service point.
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
const X86_PROBE_VA: usize = 0x0600_0000;
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
static X86_PROBE_STAGE: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
static X86_PROBE_BEFORE: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
static X86_PROBE_AFTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
fn x86_shootdown_fixture_secondary() {
    if X86_PROBE_STAGE.load(Ordering::Acquire) != 1 {
        return;
    }
    X86_PROBE_BEFORE.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) },
        Ordering::Release,
    );
    X86_PROBE_STAGE.store(2, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 3);
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
    X86_PROBE_STAGE.store(4, Ordering::Release);
    let root = crate::memory::paging::KERNEL_ROOT.lock();
    X86_PROBE_AFTER.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) },
        Ordering::Release,
    );
    drop(root);
    X86_PROBE_STAGE.store(5, Ordering::Release);
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
    // Round two: VA -> new is cached here. With IF clear again, take the lock
    // only uncontended until the BSP's shootdown of the rebinding returns.
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 6);
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
    X86_PROBE_STAGE.store(7, Ordering::Release);
    x86_bounded_wait(|| {
        drop(crate::memory::paging::KERNEL_ROOT.lock());
        X86_PROBE_STAGE.load(Ordering::Acquire) == 8
    });
    // Round one's BEFORE value is already checked; reuse the slot.
    X86_PROBE_BEFORE.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) },
        Ordering::Release,
    );
    X86_PROBE_STAGE.store(9, Ordering::Release);
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
fn x86_shootdown_fixture_primary() {
    use crate::memory::frame::{phys_to_virt, FRAME_ALLOCATOR};
    use crate::memory::paging::{KERNEL_ROOT, PAGE_SIZE};
    use hal::paging::{walk_create, walk_read, PTE_NX, PTE_PRESENT, PTE_WRITABLE};
    let flags = PTE_PRESENT | PTE_WRITABLE | PTE_NX | (1 << 8);
    let (old, new) = {
        let mut allocator = FRAME_ALLOCATOR.lock();
        let allocator = allocator.as_mut().expect("fixture frame allocator");
        let old = allocator.allocate_frame().expect("fixture old frame");
        let new = allocator
            .allocate_frame()
            .expect("fixture replacement frame");
        unsafe {
            core::ptr::write_volatile(phys_to_virt(old) as *mut usize, 0x1357);
            core::ptr::write_volatile(phys_to_virt(new) as *mut usize, 0x2468);
        }
        let root = KERNEL_ROOT.lock();
        let root_pointer = phys_to_virt(root.expect("fixture root")) as *const u64;
        assert!(
            unsafe { walk_read(root_pointer, X86_PROBE_VA) }
                .is_none_or(|pte| pte & PTE_PRESENT == 0),
            "shootdown fixture VA already mapped"
        );
        drop(root);
        crate::memory::paging::map_page_x86(allocator, X86_PROBE_VA, old, flags)
            .expect("fixture map");
        (old, new)
    };
    X86_PROBE_STAGE.store(1, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 2);
    {
        let root = KERNEL_ROOT.lock();
        let root_pointer = phys_to_virt(root.expect("fixture root")) as *mut u64;
        let mut no_allocate = || None;
        let leaf = unsafe { walk_create(root_pointer, X86_PROBE_VA, &mut no_allocate) }
            .expect("fixture leaf");
        unsafe {
            core::ptr::write_volatile(leaf, new as u64 | flags);
        }
        X86_PROBE_STAGE.store(3, Ordering::Release);
        x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 4);
        crate::memory::tlb_shootdown::flush_range(X86_PROBE_VA, PAGE_SIZE);
    }
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 5);
    assert_eq!(X86_PROBE_BEFORE.load(Ordering::Acquire), 0x1357);
    assert_eq!(X86_PROBE_AFTER.load(Ordering::Acquire), 0x2468);
    // Round two: rebind to `old` and shoot it down while the AP holds IF clear
    // and never contends. `flush_range` panics if the AP stays silent.
    {
        let root = KERNEL_ROOT.lock();
        let root_pointer = phys_to_virt(root.expect("fixture root")) as *mut u64;
        let mut no_allocate = || None;
        let leaf = unsafe { walk_create(root_pointer, X86_PROBE_VA, &mut no_allocate) }
            .expect("fixture leaf");
        unsafe {
            core::ptr::write_volatile(leaf, old as u64 | flags);
        }
    }
    X86_PROBE_STAGE.store(6, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 7);
    crate::memory::tlb_shootdown::flush_range(X86_PROBE_VA, PAGE_SIZE);
    X86_PROBE_STAGE.store(8, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 9);
    assert_eq!(X86_PROBE_BEFORE.load(Ordering::Acquire), 0x1357);
    crate::memory::paging::unmap_page_x86(X86_PROBE_VA).expect("fixture unmap");
    let mut allocator = FRAME_ALLOCATOR.lock();
    let allocator = allocator.as_mut().expect("fixture frame allocator");
    allocator.deallocate_frame(old);
    allocator.deallocate_frame(new);
    // Both shootdowns went to every online AP; the RT hart is the one that
    // held IF clear and observed the binding.
    let harts = online_hart_count();
    log::info!("S22-X86-REMOTE-SHOOTDOWN: PASS harts={} global=true irq_masked_lock=true before=4951 after=9320", harts);
    log::info!(
        "S22-X86-MASKED-SHOOTDOWN: PASS harts={} irq_masked_uncontended_lock=true after=4951",
        harts
    );
}

#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
static mut X86_HANDOFF_CONTEXTS: [core::mem::MaybeUninit<hal::arch::Context>; 2] =
    [core::mem::MaybeUninit::uninit(); 2];
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
static X86_HANDOFF_FIRST_EPOCH: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
static X86_HANDOFF_LATE_EPOCH: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
core::arch::global_asm!(
    ".global vi_x86_handoff_fixture_entry",
    "vi_x86_handoff_fixture_entry:",
    "endbr64",
    "jmp {entry}",
    entry = sym x86_handoff_fixture_incoming,
);

#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
extern "C" fn x86_handoff_fixture_incoming() -> ! {
    use crate::task::hart_local::ready;
    assert_eq!(ready::selected_task_id_for(0), 0);
    assert_eq!(ready::executing_task_id_for(0), 90_002);
    assert_eq!(ready::outgoing_context_save_task_id_for(0), 0);
    assert!(!ready::owned_by_another_hart(1, 90_001));
    assert!(retirement_switch_completed(
        0,
        X86_HANDOFF_FIRST_EPOCH.load(Ordering::Acquire)
    ));
    assert!(!retirement_switch_completed(
        0,
        X86_HANDOFF_LATE_EPOCH.load(Ordering::Acquire)
    ));
    ready::steal_from_busiest(1);
    assert_eq!(ready::pick_local_eligible(1), Some(90_001));
    ready::begin_outgoing_context_save(0, 90_002);
    capture_retirement_boundary(0);
    let contexts = (&raw mut X86_HANDOFF_CONTEXTS).cast::<hal::arch::Context>();
    unsafe {
        hal::arch::Context::switch(contexts.add(1), contexts);
    }
    panic!("handoff fixture unexpectedly re-entered");
}

/// A real raw switch proves the outgoing Ready Context cannot be stolen before
/// save, becomes stealable on the incoming stack, and cannot acknowledge a
/// retirement requested after selection. Synthetic TIDs never enter SCHEDULER.
#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
fn x86_handoff_fixture() {
    use crate::task::hart_local::ready;
    let stack = crate::task::stack::Stack::new_kernel(2).expect("handoff fixture stack");
    let flags = hal::arch::save_and_disable_interrupts();
    assert_eq!(ready::total_ready_count(), 0);
    let contexts = (&raw mut X86_HANDOFF_CONTEXTS).cast::<hal::arch::Context>();
    extern "C" {
        fn vi_x86_handoff_fixture_entry();
    }
    unsafe {
        contexts.write(hal::arch::Context::default());
        contexts.add(1).write(hal::arch::Context::default());
        (*contexts.add(1)).sp = (stack.top - 8) as u64;
        (*contexts.add(1)).rip = vi_x86_handoff_fixture_entry as *const () as u64;
        core::ptr::write_volatile((stack.top - 8) as *mut usize, 0);
    }
    ready::set_current_task_id(0, 90_001);
    ready::set_executing_task_id(0, 90_001);
    ready::begin_outgoing_context_save(0, 90_001);
    ready::push_on_hart(0, 90_001, api::TaskPriority::Normal as u8);
    X86_HANDOFF_FIRST_EPOCH.store(request_retirement_switch(0), Ordering::Release);
    capture_retirement_boundary(0);
    ready::set_current_task_id(0, 90_002);
    ready::set_selected_task_id(0, 90_002);
    X86_HANDOFF_LATE_EPOCH.store(request_retirement_switch(0), Ordering::Release);
    ready::steal_from_busiest(1);
    assert_eq!(ready::pick_local_eligible(1), None);
    assert!(ready::owned_by_another_hart(1, 90_001));
    unsafe {
        hal::arch::Context::switch(contexts, contexts.add(1));
    }
    assert_eq!(ready::current_task_id_for(0), 0);
    assert_eq!(ready::executing_task_id_for(0), 0);
    assert!(retirement_switch_completed(
        0,
        X86_HANDOFF_LATE_EPOCH.load(Ordering::Acquire)
    ));
    assert_eq!(ready::total_ready_count(), 0);
    unsafe {
        hal::arch::restore_sstatus(flags);
    }
    drop(stack);
    log::info!("S22-X86-CONTEXT-HANDOFF: PASS raw_switch=true save_guard=true steal_after_save=true late_retirement_deferred=true");
}

/// Rust entry point of a secondary core started by `PSCI_CPU_ON`.
///
/// Reached from `_secondary_entry` with the boot core's translation regime
/// installed, this hart's stack in place, interrupts masked, and `hart_id` in
/// x0. Returns only by never returning: the hart parks and lives out its life
/// in the timer/IPI trap path, which is where its scheduler round runs.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "C" fn smp_aarch64_secondary_main(hart_id: usize) -> ! {
    crate::task::hart_local::install(hart_id);
    hal::aarch64::init_secondary_hart();
    {
        use hal::Arch;
        hal::ARCH.enable_interrupts();
        if !hal::ARCH.interrupts_enabled() {
            panic!("[smp] hart {} could not enable interrupts", hart_id);
        }
    }
    if hart_id < MAX_HARTS {
        log::info!("[smp] hart {} trap-ready, interrupts-enabled", hart_id);
        HART_ONLINE[hart_id].store(true, Ordering::Release);
    }
    loop {
        // SAFETY: WFI suspends until the next interrupt; no state changes.
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
        core::hint::spin_loop();
    }
}

/// Returns `true` when the RT hart (hart 1) successfully came online.
///
/// Used by the scheduler to fall back to hart 0 on single-hart systems
/// (e.g. QEMU without `-smp 2`) so RT-priority tasks still get scheduled.
#[inline]
pub fn is_rt_hart_online() -> bool {
    HART_ONLINE[HART_RT].load(core::sync::atomic::Ordering::Relaxed)
}

/// Entry point for secondary harts, called from `_secondary_entry` asm.
///
/// a0 = hart_id (set by OpenSBI per SBI HSM §9.1.1).
///
/// Installs the trap vector, enables the timer, runs the per-hart scheduler loop.
#[no_mangle]
pub extern "C" fn smp_hart_entry(physical_hart: usize) -> ! {
    #[cfg(target_arch = "riscv64")]
    let hart_id = physical_to_logical(physical_hart)
        .unwrap_or_else(|| panic!("unmapped RV64 physical hart {}", physical_hart));
    #[cfg(not(target_arch = "riscv64"))]
    let hart_id = physical_hart;

    #[cfg(target_arch = "riscv64")]
    {
        let root = crate::memory::paging::KERNEL_ROOT
            .lock()
            .expect("RV64 secondary started before kernel paging root");
        // SAFETY: the boot hart published a complete shared root before HSM
        // startup; it maps this entry code, stack, and all kernel globals.
        unsafe {
            crate::memory::paging::activate_paging(root);
            core::arch::asm!(
                "csrs sstatus, {sum}",
                sum = in(reg) 0x40000usize,
                options(nostack)
            );
        }
    }
    // Install the trap vector (each hart has its own stvec CSR).
    // `hal::ARCH.init()` sets stvec + enables SSIE.
    #[cfg(target_arch = "riscv64")]
    {
        crate::task::hart_local::install(hart_id);
        use hal::Arch;
        hal::ARCH.init();
        // ARCH.init installs the bootstrap-safe default vector. Restore this
        // secondary's logical vector before any interrupt is enabled.
        hal::trap::init_for_hart(hart_id);
    }

    // Enable S-mode timer interrupt and arm the first tick on this hart.
    // Each hart has its own mtimecmp register via SBI; arming here starts
    // the 10ms preemption slice for this hart.
    #[cfg(target_arch = "riscv64")]
    {
        // SAFETY: csrs on sie is always legal from S-mode (RISC-V priv spec §4.1.3).
        unsafe {
            core::arch::asm!("csrs sie, {stie}", stie = in(reg) 0x20usize);
        }
        let next = hal::common::timer::read_mtime() + hal::common::timer::TICKS_PER_10MS;
        hal::common::sbi::set_timer(next);
    }

    // `ARCH.init()` enables SSIE in `sie`, but it deliberately leaves the
    // per-hart global SIE bit clear. A secondary otherwise wakes from WFI with
    // a pending IPI but never takes the SSIP trap that enters `yield_cpu()`;
    // explicitly enable delivery before advertising this hart as schedulable.
    #[cfg(target_arch = "riscv64")]
    {
        use hal::Arch;
        hal::ARCH.enable_interrupts();
        if !hal::ARCH.interrupts_enabled() {
            panic!(
                "[smp] hart {} could not enable supervisor interrupts",
                hart_id
            );
        }
    }

    // Signal hart 0's bounded wait only after this hart can actually take the
    // dispatch IPI and run its local scheduler.
    if hart_id < MAX_HARTS {
        log::info!(
            "[smp] physical {} -> logical {} trap-ready, interrupts-enabled",
            physical_hart,
            hart_id
        );
        #[cfg(feature = "test-hooks")]
        log::info!(
            "[selftest] SMP-RETIREMENT: stage=hart{}-interrupts-enabled",
            hart_id
        );
        HART_ONLINE[hart_id].store(true, Ordering::Release);
    }

    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::memory::tlb_shootdown_selftest::run_secondary(hart_id);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::cell::provenance_selftest::run_secondary(hart_id);

    // Per-hart scheduler loop.  The timer ISR (vi_timer_tick) calls yield_cpu()
    // which runs pick_next for THIS hart (work-stealing from hart 0 if idle).
    // Between ticks we sit in WFI to save power.  Interrupts are enabled on
    // entry (ARCH.init() sets sstatus.SIE=1), so WFI fires on the timer ISR.
    loop {
        #[cfg(feature = "test-hooks")]
        crate::loader::atomic_publication_tests::observe_schedule_attempt();
        // SAFETY: wfi suspends until the next interrupt; state is unchanged.
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack))
        };
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An IPI-nested service publishes a newer epoch before the interrupted
    /// outer service publishes its older snapshot; completion must not regress.
    #[test]
    fn nested_service_completion_is_not_retracted_by_stale_snapshot() {
        let complete = AtomicUsize::new(4);
        let outer_snapshot = 5;
        assert!(publish_completion(&complete, 6));
        assert!(!publish_completion(&complete, outer_snapshot));
        assert_eq!(complete.load(Ordering::Acquire), 6);
        assert!(publish_completion(&complete, 7));
        assert_eq!(complete.load(Ordering::Acquire), 7);
    }

    /// H-8: the boot publication `task::init` makes on every architecture puts
    /// hart 0 in `online_harts()`, so a shootdown issued from hart 1 (the
    /// `filter(!= me)` every remote set uses) targets it, and the count the
    /// telemetry reports agrees with the set.
    #[test]
    fn boot_hart_publication_names_hart_zero_in_remote_sets() {
        publish_boot_hart_online();
        assert_eq!(online_harts().next(), Some(0));
        assert!(online_harts()
            .filter(|hart| *hart != 1)
            .any(|hart| hart == 0));
        assert_eq!(online_harts().count(), online_hart_count());
    }

    /// Four simulated harts: requester 0 broadcasts to 1, 2 and 3. Every
    /// request is issued before any completion is consulted, the set waits on
    /// the lowest silent hart, and it completes only when all three confirmed.
    #[test]
    fn epoch_set_broadcasts_before_waiting_and_completes_over_four_harts() {
        const HARTS: usize = 4;
        let me = 0;
        let request = [const { AtomicUsize::new(0) }; HARTS];
        let complete = [const { AtomicUsize::new(0) }; HARTS];
        let mut issued = alloc::vec::Vec::new();
        let set = RemoteEpochs::broadcast((0..HARTS).filter(|hart| *hart != me), |hart| {
            issued.push(hart);
            request[hart].fetch_add(1, Ordering::AcqRel) + 1
        });
        assert_eq!(issued, [1, 2, 3], "one request per remote, in id order");
        assert_eq!(set.asked(), 3);
        let completed = |hart: usize, epoch: usize| complete[hart].load(Ordering::Acquire) >= epoch;
        assert_eq!(set.first_outstanding(completed), Some((1, 1)));
        // Out of order: 3 and 1 answer, 2 stays silent.
        assert!(publish_completion(&complete[3], 1));
        assert!(publish_completion(&complete[1], 1));
        assert_eq!(set.first_outstanding(completed), Some((2, 1)));
        assert!(publish_completion(&complete[2], 1));
        assert_eq!(set.first_outstanding(completed), None);
        // The requester never asked itself.
        assert_eq!(request[me].load(Ordering::Acquire), 0);
    }

    /// A completion published for an older request does not confirm a newer
    /// set, and ids outside `MAX_HARTS` are never asked.
    #[test]
    fn epoch_set_ignores_stale_completions_and_out_of_range_targets() {
        let request = [const { AtomicUsize::new(0) }; 4];
        let complete = [const { AtomicUsize::new(0) }; 4];
        for hart in 1..4 {
            request[hart].store(5, Ordering::Release);
            complete[hart].store(5, Ordering::Release);
        }
        let set = RemoteEpochs::broadcast([1, 2, 3, MAX_HARTS, usize::MAX], |hart| {
            request[hart].fetch_add(1, Ordering::AcqRel) + 1
        });
        assert_eq!(set.asked(), 3);
        let completed = |hart: usize, epoch: usize| complete[hart].load(Ordering::Acquire) >= epoch;
        assert_eq!(set.first_outstanding(completed), Some((1, 6)));
        for hart in 1..4 {
            publish_completion(&complete[hart], 6);
        }
        assert_eq!(set.first_outstanding(completed), None);
    }

    /// The real TLB mailboxes, on the top four hart slots (no other host test
    /// touches them): a set broadcast over four simulated harts stays
    /// outstanding until each target publishes, and each publication only
    /// retires its own slot.
    #[test]
    fn tlb_flush_set_over_four_simulated_harts() {
        let harts = MAX_HARTS - 4..MAX_HARTS;
        let set = request_tlb_flush_set(harts.clone());
        assert_eq!(set.asked(), 4);
        for hart in harts.clone() {
            assert!(tlb_flush_pending(hart));
        }
        let first = harts.start;
        assert_eq!(
            tlb_flush_set_outstanding(&set).map(|(hart, _)| hart),
            Some(first)
        );
        for hart in harts.clone().rev().skip(1) {
            // Simulated service on `hart`: snapshot its request and publish it.
            let epoch = TLB_FLUSH_REQUEST[hart].load(Ordering::Acquire);
            publish_completion(&TLB_FLUSH_COMPLETE[hart], epoch);
        }
        assert_eq!(
            tlb_flush_set_outstanding(&set).map(|(hart, _)| hart),
            Some(harts.end - 1),
            "the last silent hart is reported"
        );
        let last = harts.end - 1;
        publish_completion(
            &TLB_FLUSH_COMPLETE[last],
            TLB_FLUSH_REQUEST[last].load(Ordering::Acquire),
        );
        assert_eq!(tlb_flush_set_outstanding(&set), None);
        for hart in harts {
            assert!(!tlb_flush_pending(hart));
        }
    }

    /// Retirement-switch epochs over four simulated harts, through the same set
    /// shape the scheduler's per-hart retirement wait uses: each hart completes
    /// only its own epoch, and a request made after a completion is outstanding
    /// again until that hart's next switch.
    #[test]
    fn retirement_switch_set_over_four_simulated_harts() {
        let harts = MAX_HARTS - 8..MAX_HARTS - 4;
        let set = RemoteEpochs::broadcast(harts.clone(), request_retirement_switch);
        assert_eq!(set.asked(), 4);
        assert_eq!(
            set.first_outstanding(retirement_switch_completed)
                .map(|(hart, _)| hart),
            Some(harts.start)
        );
        for hart in harts.clone().take(3) {
            complete_retirement_switch(hart);
        }
        let last = harts.end - 1;
        assert!(retirement_switch_pending(last));
        assert_eq!(
            set.first_outstanding(retirement_switch_completed)
                .map(|(hart, _)| hart),
            Some(last)
        );
        complete_retirement_switch(last);
        assert_eq!(set.first_outstanding(retirement_switch_completed), None);
        let again = RemoteEpochs::broadcast([harts.start], request_retirement_switch);
        assert_eq!(
            again
                .first_outstanding(retirement_switch_completed)
                .map(|(hart, _)| hart),
            Some(harts.start)
        );
        assert!(set.first_outstanding(retirement_switch_completed).is_none());
        complete_retirement_switch(harts.start);
        assert_eq!(again.first_outstanding(retirement_switch_completed), None);
    }
}
