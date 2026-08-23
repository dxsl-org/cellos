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
static RETIRE_SWITCH_REQUEST: [AtomicUsize; MAX_HARTS] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static RETIRE_SWITCH_COMPLETE: [AtomicUsize; MAX_HARTS] =
    [AtomicUsize::new(0), AtomicUsize::new(0)];

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
    #[cfg(target_arch = "riscv64")]
    if hart_id != crate::task::hart_local::current_hart_id() {
        if let Some((mask, base)) = logical_sbi_target(hart_id) {
            let _ = hal::common::sbi::sbi_send_ipi(mask, base);
        }
    }
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
    target_arch = "riscv64"
))]
pub(crate) fn online_hart_count() -> usize {
    1 + HART_ONLINE
        .iter()
        .skip(1)
        .filter(|online| online.load(Ordering::Acquire))
        .count()
}

/// How many 10 ms ticks hart 0 waits for each secondary to come online before
/// logging a warning and continuing single-hart.  500 ms is generous for QEMU.
/// Only consumed by `start_secondaries`, which is riscv64-only (SBI HSM). Gated
/// to avoid a dead-code warning on aarch64/x86_64.
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

/// No-op on non-riscv64 targets.
#[cfg(not(target_arch = "riscv64"))]
pub fn start_secondaries() {}

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
