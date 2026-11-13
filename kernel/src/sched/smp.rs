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

#[cfg(any(target_arch = "aarch64", test))]
pub mod mpidr;
mod topology;
pub use topology::{CpuTopology, CPU_TOPOLOGY};

/// AArch64 CPU index ↔ `MPIDR_EL1` affinity, described from the firmware tree
/// by [`crate::boot::firmware_smp::note`] (flat until then; see [`mpidr`]).
#[cfg(target_arch = "aarch64")]
pub static CPU_MPIDRS: mpidr::MpidrTable<MAX_HARTS> = mpidr::MpidrTable::new();

/// Maximum number of logical harts this kernel tracks. Every per-hart static
/// is sized by it, so no heap allocation happens on the boot critical path.
/// This is capacity, not the number of CPUs brought online.
///
/// - x86_64: the HAL's per-CPU table bound, `hal::x86_64::syscall::MAX_CPUS`
///   (64). 64 keeps every `u64` hart mask exact, including the published
///   `ViRetirementV1.remote_hart_mask` (plan 261103-2218 H-S2-5).
/// - x86_64 G1 private lane (`feature = "g1-cpu-accounting"`): 2, which
///   `task::cpu_accounting` asserts equal to its own hart count.
/// - aarch64: 32 (spin-table and PSCI boards with more than two cores).
/// - riscv64 / 32-bit targets: 2 (QEMU virt `-smp 2`, G2 entry).
#[cfg(all(target_arch = "x86_64", not(feature = "g1-cpu-accounting")))]
pub const MAX_HARTS: usize = hal::x86_64::syscall::MAX_CPUS;
#[cfg(all(target_arch = "aarch64", not(feature = "g1-cpu-accounting")))]
pub const MAX_HARTS: usize = 32;
#[cfg(not(any(
    all(target_arch = "x86_64", not(feature = "g1-cpu-accounting")),
    all(target_arch = "aarch64", not(feature = "g1-cpu-accounting"))
)))]
pub const MAX_HARTS: usize = 2;
// The HAL owns one GS record, GDT/TSS and IST set per logical CPU.
#[cfg(target_arch = "x86_64")]
const _: () = assert!(MAX_HARTS <= hal::x86_64::syscall::MAX_CPUS);

/// The first RealTime hart: the remote hart that two-hart fixtures and RV64's
/// SBI targets pair with hart 0. RT placement itself uses [`RT_HARTS`].
pub const HART_RT: usize = 1;

/// Harts that take RealTime-priority tasks (bit n: hart n). Every secondary
/// hart by default, so RT work spreads instead of piling onto hart 1. RT tasks
/// are never stolen (Phase 03 steal filter), so this mask and each task's last
/// hart decide where they run (Phase 10 step 7).
static RT_HARTS: AtomicUsize = AtomicUsize::new(!1);

/// Whether `hart` is in the RealTime set.
pub(crate) fn is_rt_hart(hart: usize) -> bool {
    hart < usize::BITS as usize && RT_HARTS.load(Ordering::Relaxed) & (1 << hart) != 0
}

/// Where a RealTime task becomes runnable: its `last` hart when that hart is an
/// online RT hart, otherwise the online RT hart with the fewest queued RT tasks
/// (lowest hart on ties). `None` when no RT hart is online.
pub(crate) fn rt_destination(last: Option<usize>) -> Option<usize> {
    choose_rt_hart(
        last,
        |hart| is_rt_hart(hart) && HART_ONLINE[hart].load(Ordering::Acquire),
        crate::sched::hart_local::ready::rt_queued,
    )
}

/// [`rt_destination`]'s policy over explicit inputs.
fn choose_rt_hart(
    last: Option<usize>,
    eligible: impl Fn(usize) -> bool,
    queued: impl Fn(usize) -> usize,
) -> Option<usize> {
    if let Some(hart) = last.filter(|&hart| hart < MAX_HARTS && eligible(hart)) {
        return Some(hart);
    }
    (0..MAX_HARTS)
        .filter(|&hart| eligible(hart))
        .min_by_key(|&hart| (queued(hart), hart))
}

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

/// IPIs each hart has taken through its interrupt controller. The timer tick
/// also services flush requests, so only this count tells [`run_ipi_selftest`]
/// that the SGI itself — not the next tick — reached the hart.
#[cfg(target_arch = "aarch64")]
static IPI_TAKEN: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Record that `hart_id` took the kernel's IPI. Called by the IPI handler
/// (`vi_ipi_service`) on the hart itself, before it services the request, so
/// the requester's Acquire of that completion also makes this count visible.
#[cfg(target_arch = "aarch64")]
#[inline]
pub(crate) fn note_ipi_taken(hart_id: usize) {
    if let Some(taken) = IPI_TAKEN.get(hart_id) {
        taken.fetch_add(1, Ordering::Relaxed);
    }
}

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
        if hart_id >= MAX_HARTS || hart_id == crate::sched::hart_local::current_hart_id() {
            return;
        }
        #[cfg(target_arch = "riscv64")]
        if let Some((mask, base)) = logical_sbi_target(hart_id) {
            let _ = hal::common::sbi::sbi_send_ipi(mask, base);
        }
        // AArch64: the board's interrupt controller raises the IPI (a GIC SGI),
        // addressed by CPU index or by affinity, whichever it uses.
        #[cfg(target_arch = "aarch64")]
        if let Some(cpu) = crate::arch::hart_local::physical_cpu_for(hart_id) {
            if let Some(mpidr) = CPU_MPIDRS.mpidr(cpu) {
                <hal::aarch64::backend::Board as hal::aarch64::backend::ArmIrqController>::send_ipi(
                    cpu as u32, mpidr,
                );
            }
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
    let current = crate::sched::hart_local::current_hart_id();
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
/// and for an SBI `hart_start` that has to power a core up. (AArch64 waits
/// on the architected counter instead, which runs with IRQs masked.)
#[cfg(target_arch = "riscv64")]
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
        SecondaryStart::SpinTable => start_spin_table_secondaries(),
        SecondaryStart::None(reason) => log::info!("[smp] {}", reason),
    }
}

/// The PSCI conduit the kernel may call, or why none is usable.
///
/// The firmware tree's declared conduit when there is one; otherwise, on a
/// board that starts its cores through PSCI, what the architecture allows —
/// with EL3 the conduit is EL3 firmware's (`smc`); without EL3 an `smc` is
/// undefined and the only conduit that can exist is the hypervisor's (`hvc`).
/// A board that starts its cores another way (spin table, single hart) and
/// whose tree declares no PSCI has none. A conduit that cannot work is
/// refused, never attempted: an `hvc` from EL2 would target EL3, which by
/// construction has no PSCI here, and traps as undefined. An SMC is answered
/// by EL3 firmware from either exception level; an HVC from EL1 traps to EL2,
/// where the PSCI implementation lives.
///
/// The secondary-core start and the init-death reset both call PSCI through
/// this one answer.
#[cfg(target_arch = "aarch64")]
pub(crate) fn usable_psci_conduit() -> Result<hal::aarch64::psci::Conduit, &'static str> {
    use hal::aarch64::backend::{ArmPlatform, Board, SecondaryStart};
    use hal::aarch64::psci::Conduit;

    let conduit = match crate::boot::firmware_smp::psci_conduit() {
        Some(declared) => declared,
        None if matches!(
            <Board as ArmPlatform>::SECONDARY_START,
            SecondaryStart::Psci
        ) =>
        {
            hal::aarch64::psci::inferred_conduit()
        }
        None => return Err("the board has no PSCI and no firmware tree declares it"),
    };
    if conduit == Conduit::Hvc && hal::aarch64::el2::is_el2() {
        return Err("the kernel runs at EL2 and its conduit is HVC, which would target EL3");
    }
    Ok(conduit)
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
    if firmware_smp::psci_conduit().is_none() {
        log::info!(
            "[smp] no firmware tree declares PSCI; EL3 {} — inferring conduit {:?}",
            if hal::aarch64::psci::el3_implemented() {
                "implemented"
            } else {
                "absent"
            },
            hal::aarch64::psci::inferred_conduit()
        );
    }
    let conduit = match usable_psci_conduit() {
        Ok(conduit) => conduit,
        Err(why) => {
            log::warn!("[smp] {why} — keeping Cellos single-hart");
            return;
        }
    };
    log::info!(
        "[smp] PSCI conduit {:?}, kernel at {}",
        conduit,
        if at_el2 { "EL2" } else { "EL1" }
    );
    // Logical hart N runs CPU index N (the N-th `/cpus` node), so the boot core
    // has to *be* CPU 0 for the mapping to hold; anything else is reported
    // rather than half-applied.
    let boot_cpu = crate::arch::hart_local::physical_cpu_index();
    if boot_cpu != Some(0) {
        log::warn!(
            "[smp] boot core reports CPU index {:?} (expected 0) — keeping Cellos single-hart",
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
        // PSCI names the core by its full affinity, which only the firmware
        // tree knows once a machine has more than one cluster.
        let Some(mpidr) = CPU_MPIDRS.mpidr(hart_id) else {
            log::info!(
                "[smp] hart {}: no CPU {} described — done",
                hart_id,
                hart_id
            );
            break;
        };
        // The core is still off: this mapping has to exist before it reads it.
        if !crate::arch::hart_local::publish_physical_cpu(hart_id, hart_id) {
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

        match hal::aarch64::psci::cpu_on(conduit, mpidr, entry, context_addr as u64) {
            Ok(()) => log::info!(
                "[smp] hart {} start requested (cpu={} mpidr={:#x} entry={:#x})",
                hart_id,
                hart_id,
                mpidr,
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

        // Bounded wait using the hardware monotonic counter (CNTPCT_EL0) which
        // advances even with interrupts masked on the boot hart.
        let timeout_ticks = (hal::aarch64::timer::counter_frequency_hz() / 2).max(1);
        let start = hal::aarch64::timer::counter_now();
        loop {
            if HART_ONLINE[hart_id].load(Ordering::Acquire) {
                log::info!("[smp] hart {} online, parked", hart_id);
                break;
            }
            if hal::aarch64::timer::counter_now().wrapping_sub(start) >= timeout_ticks {
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

/// AArch64 starts a core through the device tree's spin table: the core spins
/// on its `cpu-release-addr` with the MMU off until that word holds an entry
/// address. A release passes no argument, so `_spin_table_entry` loads the
/// context from the single slot [`hal::aarch64::boot::publish_spin_table_context`]
/// fills — which is why cores are released one at a time and the first one
/// that does not come online ends the bring-up.
///
/// The release word is written by physical address ([`write_release_word`]),
/// as the kernel image itself is reached by physical address in the entry path.
#[cfg(target_arch = "aarch64")]
fn start_spin_table_secondaries() {
    use crate::boot::firmware_smp;
    use crate::task::stack::Stack;
    use crate::task::STACK_PAGES;

    let described = firmware_smp::cpu_count();
    if described <= 1 {
        log::info!(
            "[smp] spin-table: {} CPU(s) described — single-hart",
            described
        );
        return;
    }
    // Release addresses are indexed by `/cpus` node order, with the boot core
    // as node 0, exactly as the logical hart equals the CPU index.
    let boot_cpu = crate::arch::hart_local::physical_cpu_index();
    if boot_cpu != Some(0) {
        log::warn!(
            "[smp] boot core reports CPU index {:?} (expected 0) — keeping Cellos single-hart",
            boot_cpu
        );
        return;
    }
    let attempt = MAX_HARTS.min(described);

    extern "C" {
        /// Spin-table trampoline in hal/arch/arm/src/aarch64/boot.rs: loads
        /// the published context into x0, then runs `_secondary_entry`.
        fn _spin_table_entry();
    }
    let entry = _spin_table_entry as *const () as usize;
    let kernel_root = match *crate::memory::paging::KERNEL_ROOT.lock() {
        Some(root) => root,
        None => {
            log::warn!("[smp] spin-table: kernel page tables not published");
            return;
        }
    };

    for hart_id in 1..attempt {
        let release_addr = match firmware_smp::cpu_release_addr(hart_id) {
            Some(addr) => addr as usize,
            None => {
                log::info!("[smp] hart {hart_id}: no cpu-release-addr in FDT");
                continue;
            }
        };

        if !crate::arch::hart_local::publish_physical_cpu(hart_id, hart_id) {
            log::warn!("[smp] hart {hart_id} conflicts with the published topology — not started");
            continue;
        }

        let stack = match Stack::new_kernel(STACK_PAGES) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[smp] hart {hart_id} stack alloc failed: {e:?}");
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
        // SAFETY: single writer (hart 0, before the core is released), and no
        // reader until the release word below is written.
        let context_addr = unsafe {
            let slot = core::ptr::addr_of_mut!(SECONDARY_CONTEXT[hart_id]);
            slot.write(context);
            slot as usize
        };
        // The core reads both with caches off: clean them to the point of
        // coherency before the release (each clean ends in `dsb sy`).
        hal::aarch64::cache::clean_data_cache_range(
            context_addr,
            core::mem::size_of::<hal::aarch64::boot::SecondaryContext>(),
        );
        hal::aarch64::boot::publish_spin_table_context(context_addr as u64);

        if !write_release_word(release_addr, entry as u64) {
            log::warn!(
                "[smp] hart {hart_id}: release word {release_addr:#x} is unaligned or cannot be \
                 mapped — stopping bring-up"
            );
            return;
        }
        // SEV only wakes cores parked in WFE; the preceding `dsb sy` orders the
        // release word before it.
        crate::arch::smp::send_event();
        log::info!("[smp] hart {hart_id} released via spin table at {release_addr:#x}");

        let timeout_ticks = (hal::aarch64::timer::counter_frequency_hz() / 2).max(1);
        let start = hal::aarch64::timer::counter_now();
        while !HART_ONLINE[hart_id].load(Ordering::Acquire) {
            if hal::aarch64::timer::counter_now().wrapping_sub(start) >= timeout_ticks {
                // A late core would read the next core's context from the
                // shared slot, so no further core is released.
                log::warn!("[smp] hart {hart_id} did not come online in time — stopping bring-up");
                return;
            }
            core::hint::spin_loop();
        }
        log::info!("[smp] hart {hart_id} online, parked");
    }
}

/// Write a spin-table release word and clean it to the point of coherency.
///
/// The word is firmware memory the kernel's identity map need not cover: on
/// the Raspberry Pi it sits in page 0 (0xd8–0xf0, beside the armstub), below
/// the kernel image and unmapped as the null guard. Such a page is mapped
/// kernel-only for the write and unmapped again, so the guard is back before
/// the next core is released. `false` when the word is not 8-byte aligned (the
/// binding requires it) or its page cannot be mapped.
#[cfg(target_arch = "aarch64")]
fn write_release_word(release_addr: usize, entry: u64) -> bool {
    use crate::memory::paging::{self, Flags, PAGE_SIZE};

    if !release_addr.is_multiple_of(8) {
        return false;
    }
    let page = release_addr & !(PAGE_SIZE - 1);
    let transient = paging::virt_to_phys(page).is_none();
    if transient {
        let mut frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let Some(allocator) = frames.as_mut() else {
            return false;
        };
        let flags = Flags::from_bits(
            Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
        );
        if paging::map_page(allocator, page, page, flags).is_err() {
            return false;
        }
    }
    // SAFETY: `release_addr` is the firmware's aligned spin-table word for
    // this core, identity-mapped (permanently or just above) for the write.
    unsafe {
        core::ptr::write_volatile(release_addr as *mut u64, entry);
    }
    hal::aarch64::cache::clean_data_cache_range(release_addr, 8);
    if transient {
        // Never fails for a mapped page; it ends in a broadcast TLB invalidate.
        let _ = paging::unmap_page(page);
    }
    true
}

/// Prove every hart that came online actually takes the kernel's IPI.
///
/// The one cross-hart path a boot log cannot witness on its own: the request is
/// delivered by a GIC SGI, the target answers from *its* trap path, and this
/// hart's bounded wait ends only when that epoch lands. Without it, a hart that
/// came online but is deaf to the IPI looks exactly like a healthy one — and
/// every later remote invalidation would silently fall back to the retained-frame
/// path. Every remote hart is probed because the SGI target is per core: a
/// wrong affinity (another cluster, Aff0 ≥ 16) fails only for the cores it
/// names. Bounded and non-fatal: a failed probe is reported, it does not stop
/// the boot.
#[cfg(all(target_arch = "aarch64", not(cellos_posture = "release")))]
pub fn run_ipi_selftest() {
    let mut probed = 0;
    for remote in online_harts().filter(|hart| *hart != 0) {
        probed += 1;
        let taken_before = IPI_TAKEN[remote].load(Ordering::Relaxed);
        let epoch = request_tlb_flush(remote);
        // The architected counter, not the tick: the boot hart runs this with
        // IRQs masked, so `system_ticks` would never reach a deadline.
        let timeout_ticks = (hal::aarch64::timer::counter_frequency_hz() / 2).max(1);
        let start = hal::aarch64::timer::counter_now();
        let (mut acknowledged, mut sgi_taken) = (false, false);
        while hal::aarch64::timer::counter_now().wrapping_sub(start) < timeout_ticks {
            acknowledged = tlb_flush_completed(remote, epoch);
            sgi_taken = IPI_TAKEN[remote].load(Ordering::Relaxed) != taken_before;
            if acknowledged && sgi_taken {
                break;
            }
            core::hint::spin_loop();
        }
        match (acknowledged, sgi_taken) {
            (true, true) => {
                log::info!("[selftest] SMP-IPI: PASS hart={} epoch={}", remote, epoch)
            }
            // Its timer tick serviced the request; the IPI never arrived.
            (true, false) => log::error!(
                "[selftest] SMP-IPI: FAIL hart={} epoch={} (acknowledged by tick, no IPI taken)",
                remote,
                epoch
            ),
            _ => log::error!(
                "[selftest] SMP-IPI: FAIL hart={} epoch={} (no acknowledgement)",
                remote,
                epoch
            ),
        }
    }
    if probed == 0 {
        log::info!("[selftest] SMP-IPI: skipped, no remote hart online");
    }
}

/// No-op on targets with one CPU.
#[cfg(not(cellos_wide))]
pub fn start_secondaries() {}

#[cfg(target_arch = "x86_64")]
#[path = "../arch/x86_64/smp_bringup.rs"]
mod x86;
#[cfg(target_arch = "x86_64")]
pub use x86::*;

/// Rust entry point of a secondary core started by `PSCI_CPU_ON`.
///
/// Reached from `_secondary_entry` with the boot core's translation regime
/// installed, this hart's stack in place, interrupts masked, and `hart_id` in
/// x0. Returns only by never returning: the hart parks and lives out its life
/// in the timer/IPI trap path, which is where its scheduler round runs.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "C" fn smp_aarch64_secondary_main(hart_id: usize) -> ! {
    crate::sched::hart_local::install(hart_id);
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
        // WFI suspends until the next interrupt; no state changes.
        crate::arch::cpu::halt();
        core::hint::spin_loop();
    }
}

/// Returns `true` when [`HART_RT`], the hart two-hart fixtures pair with hart
/// 0, came online. RT placement does not use it: see [`rt_destination`].
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
        }
        crate::arch::user_access::secondary_open();
    }
    // Install the trap vector (each hart has its own stvec CSR).
    // `hal::ARCH.init()` sets stvec + enables SSIE.
    #[cfg(target_arch = "riscv64")]
    {
        crate::sched::hart_local::install(hart_id);
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
        crate::arch::clock::start_tick();
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
    crate::arch::memory::tlb_shootdown_selftest::run_secondary(hart_id);
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    crate::arch::cell::provenance_selftest::run_secondary(hart_id);

    // Per-hart scheduler loop.  The timer ISR (vi_timer_tick) calls yield_cpu()
    // which runs pick_next for THIS hart (work-stealing from hart 0 if idle).
    // Between ticks we sit in WFI to save power.  Interrupts are enabled on
    // entry (ARCH.init() sets sstatus.SIE=1), so WFI fires on the timer ISR.
    loop {
        #[cfg(feature = "test-hooks")]
        crate::loader::atomic_publication_tests::observe_schedule_attempt();
        // wfi suspends until the next interrupt; state is unchanged.
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        crate::arch::cpu::halt();
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

#[cfg(test)]
mod rt_placement_tests {
    use super::choose_rt_hart;

    const QUEUED: [usize; 4] = [9, 2, 0, 0];

    fn queued(hart: usize) -> usize {
        QUEUED.get(hart).copied().unwrap_or(usize::MAX)
    }

    #[test]
    fn an_eligible_last_hart_keeps_its_task() {
        assert_eq!(
            choose_rt_hart(Some(1), |h| (1..4).contains(&h), queued),
            Some(1)
        );
    }

    #[test]
    fn otherwise_the_least_loaded_lowest_eligible_hart_wins() {
        // Last hart 0 is not in the RT set; harts 2 and 3 tie on load.
        assert_eq!(
            choose_rt_hart(Some(0), |h| (1..4).contains(&h), queued),
            Some(2)
        );
        assert_eq!(choose_rt_hart(None, |h| h == 1 || h == 3, queued), Some(3));
    }

    #[test]
    fn no_eligible_hart_means_no_rt_destination() {
        assert_eq!(choose_rt_hart(Some(1), |_| false, queued), None);
    }
}
