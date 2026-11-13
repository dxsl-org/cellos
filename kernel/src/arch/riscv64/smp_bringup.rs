// SPDX-License-Identifier: MPL-2.0
//! riscv64 items of `crate::sched::smp`, compiled as its `rv` child module
//! (`#[path]`), so they keep reading that module's private items unchanged.

use super::*;

/// Test-hooks: how many preemption requests were pended for each logical hart.
///
/// The RT-wake fixture needs the *decision* observable, not the interrupt: it
/// asserts that consuming a message pends exactly one for the sender's target
/// hart, that a stale delivery token pends none, and that the decision follows the
/// target hart's running priority rather than the waking hart's.
#[cfg(feature = "test-hooks")]
pub(super) static PREEMPT_PENDS: [AtomicUsize; MAX_HARTS] =
    [const { AtomicUsize::new(0) }; MAX_HARTS];

#[cfg(feature = "test-hooks")]
#[inline]
pub(crate) fn note_preempt_pend(hart_id: usize) {
    if hart_id < MAX_HARTS {
        PREEMPT_PENDS[hart_id].fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(feature = "test-hooks")]
#[inline]
pub(crate) fn preempt_pends_for(hart_id: usize) -> usize {
    PREEMPT_PENDS
        .get(hart_id)
        .map(|count| count.load(Ordering::Acquire))
        .unwrap_or(0)
}

#[cfg(feature = "test-hooks")]
pub(crate) fn reset_preempt_pends() {
    for count in PREEMPT_PENDS.iter() {
        count.store(0, Ordering::Release);
    }
}

/// Publish the RV64 boot hart's numbering into [`CPU_TOPOLOGY`]: the boot hart
/// is logical 0 and the hart it displaced takes its number
/// ([`topology::boot_swapped_physical`]). Called once by `kmain` on the boot
/// hart, before any secondary is started.
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

pub fn boot_physical_hart() -> Option<usize> {
    logical_to_physical(0)
}

pub fn logical_sbi_target(logical_hart: usize) -> Option<(usize, usize)> {
    logical_to_physical(logical_hart).map(|physical| (1, physical))
}

/// Return the one remote RV64 hart this boot can have, if it is online.
///
/// Every hart, the boot hart included, publishes readiness in `HART_ONLINE`
/// with Release. RV64 runs at most two harts (`MAX_HARTS = 2`), so the remote
/// hart is the RT hart for hart 0 and hart 0 for everyone else.
pub fn remote_online_sbi_target() -> Option<(usize, usize)> {
    const _: () = assert!(MAX_HARTS == 2);
    let current = crate::sched::hart_local::current_hart_id();
    let remote = if current == 0 { HART_RT } else { 0 };
    let online = HART_ONLINE[remote].load(Ordering::Acquire);
    online.then(|| logical_sbi_target(remote)).flatten()
}

/// How many 10 ms ticks hart 0 waits for each secondary to come online before
/// logging a warning and continuing single-hart.  500 ms is generous for QEMU
/// and for an SBI `hart_start` that has to power a core up. (AArch64 waits
/// on the architected counter instead, which runs with IRQs masked.)
pub(super) const SECONDARY_BOOT_TIMEOUT_TICKS: usize = 50;

/// Called by hart 0 **after** `task::init()` to bring secondary harts online.
///
/// Each secondary is started via SBI HSM `hart_start`.  Hart 0 then spins
/// (bounded) waiting for each secondary to set `HART_ONLINE[hart_id]`.
/// If a secondary fails to start or times out, a warning is logged and the
/// system continues single-hart — graceful degradation, never a panic.
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

/// `smp_hart_entry`'s riscv64 bring-up, up to interrupts enabled: map the
/// physical hart to its logical id, enter the shared kernel root, install the
/// hart-local state and trap vector, arm the tick, enable interrupts. Returns
/// the logical hart.
pub(super) fn secondary_start(physical_hart: usize) -> usize {
    let hart_id = physical_to_logical(physical_hart)
        .unwrap_or_else(|| panic!("unmapped RV64 physical hart {}", physical_hart));

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
    {
        crate::arch::clock::start_tick();
    }

    // `ARCH.init()` enables SSIE in `sie`, but it deliberately leaves the
    // per-hart global SIE bit clear. A secondary otherwise wakes from WFI with
    // a pending IPI but never takes the SSIP trap that enters `yield_cpu()`;
    // explicitly enable delivery before advertising this hart as schedulable.
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
    hart_id
}
