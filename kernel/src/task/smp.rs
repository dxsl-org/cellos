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

/// Maximum number of harts this kernel tracks.  2 covers QEMU virt `-smp 2`
/// (G2 entry target).  Constant so secondary stacks and HART_ONLINE are
/// statically sized — no heap allocation during the boot critical path.
pub const MAX_HARTS: usize = 2;

/// Hart dedicated to RealTime-priority cells.  RT tasks are enqueued here by
/// `push_ready` and never stolen (Phase 03 steal filter excludes RT).
pub const HART_RT: usize = 1;

/// Set to `true` by each secondary hart once its trap vector and timer are ready.
/// Hart 0's bounded wait reads this via `Acquire` to observe all preceding stores.
pub static HART_ONLINE: [AtomicBool; MAX_HARTS] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Monotonic switch-completion epochs for root-retirement quiescence. A retiring
/// generation cannot release its CellId slot until every requested hart has
/// switched to a different context and published that completion.
/// Per-hart invalidation epochs: a requester publishes a target, the target
/// performs the local flush on its way through the trap path and publishes
/// completion. Only then may the requester recycle a tag or release frames — a
/// firmware call returning is not evidence that another hart stopped using the
/// translation (phase 02 slice 3).
static TLB_FLUSH_REQUEST: [AtomicUsize; MAX_HARTS] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static TLB_FLUSH_COMPLETE: [AtomicUsize; MAX_HARTS] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// Deliver the kernel's cross-hart IPI to `hart_id`.
///
/// The requester only records an epoch; what makes the target *act* is this
/// interrupt. Every path that needs a remote hart to do work (invalidate its
/// TLB, switch out of a retiring context) goes through here so the delivery
/// mechanism has one definition per architecture.
#[inline]
fn send_ipi(hart_id: usize) {
    #[cfg(any(
        target_arch = "riscv64",
        all(target_arch = "aarch64", not(feature = "board-rpi3"))
    ))]
    {
        if hart_id >= MAX_HARTS || hart_id == crate::task::hart_local::current_hart_id() {
            return;
        }
        #[cfg(target_arch = "riscv64")]
        if let Some((mask, base)) = logical_sbi_target(hart_id) {
            let _ = hal::common::sbi::sbi_send_ipi(mask, base);
        }
        // AArch64: the BCM2836 (RPi3) local controller has no software-interrupt
        // path in this kernel, and that board starts no secondary, so the GIC SGI
        // is the only delivery mechanism that exists.
        #[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
        if let Some(cpu) = crate::task::hart_local::physical_cpu_for(hart_id) {
            hal::aarch64::gic::send_sgi(cpu as u32, hal::aarch64::gic::SGI_IPI);
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
        all(target_arch = "x86_64", not(all(test, not(target_os = "none")))),
        all(target_arch = "aarch64", not(feature = "board-rpi3"))
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
    if hart_id >= MAX_HARTS {
        return 0;
    }
    let epoch = TLB_FLUSH_REQUEST[hart_id].load(Ordering::Acquire);
    if TLB_FLUSH_COMPLETE[hart_id].load(Ordering::Acquire) < epoch {
        crate::memory::paging::tlb_flush_all();
        TLB_FLUSH_COMPLETE[hart_id].store(epoch, Ordering::Release);
        #[cfg(all(feature = "test-hooks", not(target_arch = "x86_64")))]
        log::info!(
            "[selftest] TLB-ACK: stage=remote-flush-completed hart={} epoch={}",
            hart_id,
            epoch
        );
    }
    epoch
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

/// The logical harts that completed kernel bring-up.
pub fn online_harts() -> impl Iterator<Item = usize> {
    (0..MAX_HARTS).filter(|hart| HART_ONLINE[*hart].load(Ordering::Acquire))
}

static RETIRE_SWITCH_REQUEST: [AtomicUsize; MAX_HARTS] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static RETIRE_SWITCH_COMPLETE: [AtomicUsize; MAX_HARTS] =
    [AtomicUsize::new(0), AtomicUsize::new(0)];

#[cfg(target_arch = "x86_64")]
static RETIRE_SWITCH_INFLIGHT: [AtomicUsize; MAX_HARTS] =
    [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Capture before scheduler selection: a later request may concern the incoming
/// task and must wait for that task's next switch-away.
#[cfg(target_arch = "x86_64")]
pub fn capture_retirement_boundary(hart: usize) {
    RETIRE_SWITCH_INFLIGHT[hart].store(
        RETIRE_SWITCH_REQUEST[hart].load(Ordering::Acquire), Ordering::Release,
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
static PREEMPT_PENDS: [AtomicUsize; MAX_HARTS] = [AtomicUsize::new(0), AtomicUsize::new(0)];

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

#[cfg(target_arch = "riscv64")]
static BOOT_PHYSICAL_HART: AtomicUsize = AtomicUsize::new(usize::MAX);

#[cfg(target_arch = "riscv64")]
pub fn set_boot_physical_hart(physical_hart: usize) {
    assert!(
        physical_hart < MAX_HARTS,
        "unsupported RV64 boot hart {physical_hart}"
    );
    BOOT_PHYSICAL_HART.store(physical_hart, Ordering::Release);
}

#[cfg(target_arch = "riscv64")]
pub fn boot_physical_hart() -> Option<usize> {
    let physical = BOOT_PHYSICAL_HART.load(Ordering::Acquire);
    (physical < MAX_HARTS).then_some(physical)
}

#[cfg(target_arch = "riscv64")]
pub fn logical_to_physical(logical_hart: usize) -> Option<usize> {
    let boot = BOOT_PHYSICAL_HART.load(Ordering::Acquire);
    match logical_hart {
        0 if boot < MAX_HARTS => Some(boot),
        HART_RT if boot < MAX_HARTS => Some(boot ^ 1),
        _ => None,
    }
}

#[cfg(target_arch = "riscv64")]
pub fn physical_to_logical(physical_hart: usize) -> Option<usize> {
    let boot = BOOT_PHYSICAL_HART.load(Ordering::Acquire);
    if physical_hart == boot {
        Some(0)
    } else if physical_hart < MAX_HARTS && physical_hart == (boot ^ 1) {
        Some(HART_RT)
    } else {
        None
    }
}

#[cfg(target_arch = "riscv64")]
pub fn logical_sbi_target(logical_hart: usize) -> Option<(usize, usize)> {
    logical_to_physical(logical_hart).map(|physical| (1, physical))
}

/// Return every online RV64 hart except the one executing this call.
///
/// Hart 0 is running whenever this kernel reaches normal execution but is not
/// represented by `HART_ONLINE`; secondary harts publish readiness with Release.
#[cfg(target_arch = "riscv64")]
pub fn remote_online_sbi_target() -> Option<(usize, usize)> {
    let current = crate::task::hart_local::current_hart_id();
    let remote = if current == 0 { HART_RT } else { 0 };
    let online = remote == 0 || HART_ONLINE[remote].load(Ordering::Acquire);
    online.then(|| logical_sbi_target(remote)).flatten()
}

/// Return the harts that completed kernel bring-up for the current boot.
///
/// Hart zero is the active boot hart; each secondary contributes only after
/// publishing `HART_ONLINE`, so test evidence cannot confuse configured SMP
/// capacity with observed runtime availability.
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    any(target_arch = "riscv64", target_arch = "x86_64")
))]
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
#[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
static mut SECONDARY_CONTEXT: [hal::aarch64::boot::SecondaryContext; MAX_HARTS] =
    [hal::aarch64::boot::SecondaryContext {
        hart_id: 0,
        stack_top: 0,
        mair: 0,
        tcr: 0,
        ttbr0: 0,
        sctlr: 0,
    }; MAX_HARTS];

/// Called by hart 0 **after** `task::init()` to bring secondary cores online.
///
/// AArch64 starts a core through firmware (`PSCI_CPU_ON` over the conduit the
/// device tree declares). Everything the new core needs — its stack, the boot
/// core's live translation regime, the hardening bits in `SCTLR_EL1` — travels
/// in one context block it reads with the MMU still off; `_secondary_entry`
/// installs the regime and calls [`smp_aarch64_secondary_main`].
///
/// Bounded wait, graceful degradation, never a panic: a core that does not come
/// online within the deadline leaves the system single-hart with a warning.
#[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
pub fn start_secondaries() {
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
        crate::task::hart_local::publish_physical_cpu(hart_id, hart_id);
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

/// RPi3: the BCM2836 local controller has no software-interrupt path here, and
/// the board's secondaries are parked by firmware, so it stays single-hart.
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub fn start_secondaries() {
    log::info!("[smp] BCM2836 has no SGI path in this kernel — keeping Cellos single-hart");
}

/// No-op on targets with one CPU.
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64", target_arch = "x86_64")))]
pub fn start_secondaries() {}

#[cfg(target_arch = "x86_64")]
static X86_READY: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static X86_QUALIFIED: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static X86_BOOT_COMPLETE: AtomicBool = AtomicBool::new(false);
#[cfg(target_arch = "x86_64")]
static X86_PREEMPT: [AtomicBool; MAX_HARTS] = [const { AtomicBool::new(false) }; MAX_HARTS];

#[cfg(target_arch = "x86_64")]
pub fn x86_substrate_qualified() -> bool {
    X86_BOOT_COMPLETE.load(Ordering::Acquire)
        && (online_harts().count() == 1 || X86_QUALIFIED.load(Ordering::Acquire))
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

#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct X86SecondaryContext {
    stack_top: usize,
    kernel_cr3: usize,
    logical: usize,
    lapic_id: usize,
}

#[cfg(target_arch = "x86_64")]
static mut X86_SECONDARY: X86SecondaryContext = X86SecondaryContext {
    stack_top: 0, kernel_cr3: 0, logical: 1, lapic_id: 0,
};

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
fn x86_bounded_wait(mut done: impl FnMut() -> bool) {
    // A bounded instruction budget does not depend on timer interrupts being
    // delivered to the waiting CPU. Fail closed once a callback was released.
    for _ in 0..100_000_000usize {
        if done() { return; }
        core::hint::spin_loop();
    }
    panic!("[x86-smp] active AP safety handshake timed out");
}

#[cfg(target_arch = "x86_64")]
pub fn start_secondaries() {
    use crate::boot::limine;
    let bsp_id = core::arch::x86_64::__cpuid(1).ebx >> 24;
    HART_ONLINE[0].store(true, Ordering::Release);
    #[cfg(feature = "test-hooks")]
    x86_handoff_fixture();
    let finish_up = || {
        X86_BOOT_COMPLETE.store(true, Ordering::Release);
        log::info!("[x86-smp] topology online=1 processor_ids={} qualified=yes", bsp_id);
    };
    if !hal::x86_64::apic::register_cpu(0, bsp_id) {
        // UP does not use APIC-directed remote invalidations.
        finish_up();
        return;
    }
    let Some(response) = limine::get_mp_response() else { finish_up(); return };
    let count = response.cpu_count as usize;
    if response.flags != 0 || response.bsp_lapic_id != bsp_id
        || count == 0 || count > 256 || !limine::mp_record_valid(response.cpus, count) {
        log::warn!("[x86-smp] unsupported MP topology; no AP activated");
        finish_up();
        return;
    }
    let mut ids = [false; 256];
    let mut processors = [0u32; 256];
    let mut bsp_seen = false;
    let mut secondary: *mut limine::LimineMpInfo = core::ptr::null_mut();
    for index in 0..count {
        let info = unsafe { *response.cpus.add(index) };
        if !limine::mp_record_valid(info, 1) { finish_up(); return; }
        let cpu = unsafe { &*info };
        let id = cpu.lapic_id as usize;
        if id > 255 || ids[id]
            || processors[..index].contains(&cpu.processor_id)
            || (cpu.lapic_id != bsp_id && cpu.goto_address.load(Ordering::Acquire) != 0) {
            log::warn!("[x86-smp] invalid MP CPU record; no AP activated");
            finish_up(); return;
        }
        ids[id] = true;
        processors[index] = cpu.processor_id;
        if cpu.lapic_id == bsp_id { bsp_seen = true; }
        else if secondary.is_null() { secondary = info; }
    }
    if !bsp_seen || secondary.is_null() { finish_up(); return; }
    let ap_id = unsafe { (*secondary).lapic_id };
    if !hal::x86_64::apic::register_cpu(1, ap_id) { finish_up(); return; }
    let stack = match crate::task::stack::Stack::new_kernel(crate::task::STACK_PAGES) {
        Ok(stack) => stack,
        Err(_) => { finish_up(); return; }
    };
    let kernel_cr3 = hal::domain::kernel_cr3();
    assert!(kernel_cr3 != 0, "AP requires permanent shared kernel root");
    let stack_top = stack.top;
    core::mem::forget(stack);
    extern "C" { fn vi_x86_secondary_entry(); }
    unsafe {
        X86_SECONDARY = X86SecondaryContext { stack_top, kernel_cr3, logical: 1, lapic_id: ap_id as usize };
        (*secondary).extra_argument = (&raw const X86_SECONDARY) as u64;
        (*secondary).goto_address.store(vi_x86_secondary_entry as *const () as usize, Ordering::Release);
    }
    // The BSP must receive the AP's reciprocal proof even if boot entered this
    // routine with interrupts masked. Restore its original state afterwards.
    let flags: usize;
    unsafe { core::arch::asm!("pushfq", "pop {}", "sti", out(reg) flags); }
    x86_bounded_wait(|| X86_READY.load(Ordering::Acquire));
    let epoch = request_tlb_flush(1);
    x86_bounded_wait(|| tlb_flush_completed(1, epoch));
    X86_QUALIFIED.store(true, Ordering::Release);
    x86_bounded_wait(|| HART_ONLINE[1].load(Ordering::Acquire));
    #[cfg(feature = "test-hooks")]
    x86_shootdown_fixture_primary();
    X86_BOOT_COMPLETE.store(true, Ordering::Release);
    log::info!("[x86-smp] transport IPI-invalidate bidirectional=pass");
    log::info!("[x86-smp] topology online=2 processor_ids={};{} qualified=yes", bsp_id, ap_id);
    if flags & (1 << 9) == 0 { unsafe { core::arch::asm!("cli", options(nomem, nostack)); } }
}

#[cfg(target_arch = "x86_64")]
extern "C" fn x86_secondary_main(logical: usize, lapic_id: usize, kernel_cr3: usize) -> ! {
    assert!(logical == 1);
    if !unsafe { hal::x86_64::init_secondary_cpu(logical, lapic_id as u32, kernel_cr3) } {
        panic!("[x86-smp] active AP feature/isolation initialization failed");
    }
    crate::task::hart_local::install(logical);
    // Establish this CPU's own idle activation before enabling delivery.
    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    let epoch = request_tlb_flush(0);
    x86_bounded_wait(|| tlb_flush_completed(0, epoch));
    X86_READY.store(true, Ordering::Release);
    x86_bounded_wait(|| X86_QUALIFIED.load(Ordering::Acquire));
    HART_ONLINE[logical].store(true, Ordering::Release);
    x86_bounded_wait(|| {
        #[cfg(feature = "test-hooks")]
        x86_shootdown_fixture_secondary();
        X86_BOOT_COMPLETE.load(Ordering::Acquire)
    });
    loop {
        crate::task::yield_cpu();
        unsafe { core::arch::asm!("sti", "hlt", options(nomem, nostack)); }
    }
}

// This behavioral fixture caches a genuinely GLOBAL shared translation on the
// AP, changes its binding on the BSP, then forces the AP to contend on the same
// page-table lock with IF clear. Success requires lock-spin local invalidation;
// an ISR-only acknowledgement deadlocks or retains the old binding.
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
    if X86_PROBE_STAGE.load(Ordering::Acquire) != 1 { return; }
    X86_PROBE_BEFORE.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) }, Ordering::Release,
    );
    X86_PROBE_STAGE.store(2, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 3);
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }
    X86_PROBE_STAGE.store(4, Ordering::Release);
    let root = crate::memory::paging::KERNEL_ROOT.lock();
    X86_PROBE_AFTER.store(
        unsafe { core::ptr::read_volatile(X86_PROBE_VA as *const usize) }, Ordering::Release,
    );
    drop(root);
    X86_PROBE_STAGE.store(5, Ordering::Release);
    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
}

#[cfg(all(target_arch = "x86_64", feature = "test-hooks"))]
fn x86_shootdown_fixture_primary() {
    use crate::memory::frame::{phys_to_virt, FRAME_ALLOCATOR};
    use crate::memory::paging::{KERNEL_ROOT, PAGE_SIZE};
    use hal::paging::{PTE_PRESENT, PTE_WRITABLE, PTE_NX, walk_read, walk_create};
    let flags = PTE_PRESENT | PTE_WRITABLE | PTE_NX | (1 << 8);
    let (old, new) = {
        let mut allocator = FRAME_ALLOCATOR.lock();
        let allocator = allocator.as_mut().expect("fixture frame allocator");
        let old = allocator.allocate_frame().expect("fixture old frame");
        let new = allocator.allocate_frame().expect("fixture replacement frame");
        unsafe {
            core::ptr::write_volatile(phys_to_virt(old) as *mut usize, 0x1357);
            core::ptr::write_volatile(phys_to_virt(new) as *mut usize, 0x2468);
        }
        let root = KERNEL_ROOT.lock();
        let root_pointer = phys_to_virt(root.expect("fixture root")) as *const u64;
        assert!(unsafe { walk_read(root_pointer, X86_PROBE_VA) }.is_none_or(|pte| pte & PTE_PRESENT == 0),
            "shootdown fixture VA already mapped");
        drop(root);
        crate::memory::paging::map_page_x86(allocator, X86_PROBE_VA, old, flags).expect("fixture map");
        (old, new)
    };
    X86_PROBE_STAGE.store(1, Ordering::Release);
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 2);
    {
        let root = KERNEL_ROOT.lock();
        let root_pointer = phys_to_virt(root.expect("fixture root")) as *mut u64;
        let mut no_allocate = || None;
        let leaf = unsafe { walk_create(root_pointer, X86_PROBE_VA, &mut no_allocate) }.expect("fixture leaf");
        unsafe { core::ptr::write_volatile(leaf, new as u64 | flags); }
        X86_PROBE_STAGE.store(3, Ordering::Release);
        x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 4);
        crate::memory::tlb_shootdown::flush_range(X86_PROBE_VA, PAGE_SIZE);
    }
    x86_bounded_wait(|| X86_PROBE_STAGE.load(Ordering::Acquire) == 5);
    assert_eq!(X86_PROBE_BEFORE.load(Ordering::Acquire), 0x1357);
    assert_eq!(X86_PROBE_AFTER.load(Ordering::Acquire), 0x2468);
    crate::memory::paging::unmap_page_x86(X86_PROBE_VA).expect("fixture unmap");
    let mut allocator = FRAME_ALLOCATOR.lock();
    let allocator = allocator.as_mut().expect("fixture frame allocator");
    allocator.deallocate_frame(old);
    allocator.deallocate_frame(new);
    log::info!("S22-X86-REMOTE-SHOOTDOWN: PASS harts=2 global=true irq_masked_lock=true before=4951 after=9320");
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
    assert!(retirement_switch_completed(0, X86_HANDOFF_FIRST_EPOCH.load(Ordering::Acquire)));
    assert!(!retirement_switch_completed(0, X86_HANDOFF_LATE_EPOCH.load(Ordering::Acquire)));
    ready::steal_from_busiest(1);
    assert_eq!(ready::pick_local_eligible(1), Some(90_001));
    ready::begin_outgoing_context_save(0, 90_002);
    capture_retirement_boundary(0);
    let contexts = (&raw mut X86_HANDOFF_CONTEXTS).cast::<hal::arch::Context>();
    unsafe { hal::arch::Context::switch(contexts.add(1), contexts); }
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
    extern "C" { fn vi_x86_handoff_fixture_entry(); }
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
    unsafe { hal::arch::Context::switch(contexts, contexts.add(1)); }
    assert_eq!(ready::current_task_id_for(0), 0);
    assert_eq!(ready::executing_task_id_for(0), 0);
    assert!(retirement_switch_completed(0, X86_HANDOFF_LATE_EPOCH.load(Ordering::Acquire)));
    assert_eq!(ready::total_ready_count(), 0);
    unsafe { hal::arch::restore_sstatus(flags); }
    drop(stack);
    log::info!("S22-X86-CONTEXT-HANDOFF: PASS raw_switch=true save_guard=true steal_after_save=true late_retirement_deferred=true");
}

/// Rust entry point of a secondary core started by `PSCI_CPU_ON`.
///
/// Reached from `_secondary_entry` with the boot core's translation regime
/// installed, this hart's stack in place, interrupts masked, and `hart_id` in
/// x0. Returns only by never returning: the hart parks and lives out its life
/// in the timer/IPI trap path, which is where its scheduler round runs.
#[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
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
