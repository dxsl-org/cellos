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

#[cfg(target_arch = "aarch64")]
#[path = "../arch/aarch64/smp_bringup.rs"]
mod aarch64;
#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

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

/// Deliver the kernel's cross-hart IPI to `hart_id`.
///
/// The requester only records an epoch; what makes the target *act* is this
/// interrupt. Every path that needs a remote hart to do work (invalidate its
/// TLB, switch out of a retiring context) goes through here so the delivery
/// mechanism has one definition per architecture.
#[inline]
pub(crate) fn send_ipi(hart_id: usize) {
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
    // RV64 `fence.i` requests share the TLB request's trap path and spin answers.
    crate::arch::icache::service_sync_request(hart_id);
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
pub(crate) fn publish_completion(complete: &AtomicUsize, epoch: usize) -> bool {
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

#[cfg(target_arch = "riscv64")]
#[path = "../arch/riscv64/smp_bringup.rs"]
mod rv;
#[cfg(target_arch = "riscv64")]
pub use rv::*;

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

/// Physical id (RV64 hart id, x86 LAPIC ID, AArch64 CPU index) of
/// `logical_hart`, if [`CPU_TOPOLOGY`] maps it.
pub fn logical_to_physical(logical_hart: usize) -> Option<usize> {
    CPU_TOPOLOGY.physical(logical_hart)
}

/// Logical hart that owns `physical_hart`, if [`CPU_TOPOLOGY`] maps it.
pub fn physical_to_logical(physical_hart: usize) -> Option<usize> {
    CPU_TOPOLOGY.logical(physical_hart)
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

/// No-op on targets with one CPU.
#[cfg(not(cellos_wide))]
pub fn start_secondaries() {}

#[cfg(target_arch = "x86_64")]
#[path = "../arch/x86_64/smp_bringup.rs"]
mod x86;
#[cfg(target_arch = "x86_64")]
pub use x86::*;

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
    let hart_id = rv::secondary_start(physical_hart);
    #[cfg(not(target_arch = "riscv64"))]
    let hart_id = physical_hart;

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
