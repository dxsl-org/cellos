// SPDX-License-Identifier: MPL-2.0
//! aarch64 items of `crate::sched::smp`, compiled as its `aarch64` child module
//! (`#[path]`), so they keep reading that module's private items unchanged.

use super::*;

/// AArch64 CPU index ↔ `MPIDR_EL1` affinity, described from the firmware tree
/// by [`crate::arch::boot::firmware::firmware_smp::note`] (flat until then; see [`mpidr`]).
pub static CPU_MPIDRS: mpidr::MpidrTable<MAX_HARTS> = mpidr::MpidrTable::new();

/// IPIs each hart has taken through its interrupt controller. The timer tick
/// also services flush requests, so only this count tells [`run_ipi_selftest`]
/// that the SGI itself — not the next tick — reached the hart.
pub(super) static IPI_TAKEN: [AtomicUsize; MAX_HARTS] = [const { AtomicUsize::new(0) }; MAX_HARTS];

/// Record that `hart_id` took the kernel's IPI. Called by the IPI handler
/// (`vi_ipi_service`) on the hart itself, before it services the request, so
/// the requester's Acquire of that completion also makes this count visible.
#[inline]
pub(crate) fn note_ipi_taken(hart_id: usize) {
    if let Some(taken) = IPI_TAKEN.get(hart_id) {
        taken.fetch_add(1, Ordering::Relaxed);
    }
}

/// Boot state for one secondary core, published before `PSCI_CPU_ON`.
///
/// Written once by the boot hart and read by a core whose caches are off, so
/// the write is followed by a clean to the point of coherency. `static mut`
/// with a single writer and no reader until the core is started is the whole
/// synchronisation argument.
pub(super) static mut SECONDARY_CONTEXT: [hal::aarch64::boot::SecondaryContext; MAX_HARTS] =
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
pub(crate) fn usable_psci_conduit() -> Result<hal::aarch64::psci::Conduit, &'static str> {
    use hal::aarch64::backend::{ArmPlatform, Board, SecondaryStart};
    use hal::aarch64::psci::Conduit;

    let conduit = match crate::arch::boot::firmware::firmware_smp::psci_conduit() {
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
pub(super) fn start_psci_secondaries() {
    use crate::arch::boot::firmware::firmware_smp;
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
pub(super) fn start_spin_table_secondaries() {
    use crate::arch::boot::firmware::firmware_smp;
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
pub(super) fn write_release_word(release_addr: usize, entry: u64) -> bool {
    use crate::memory::paging::{self, Flags, PAGE_SIZE};

    if !release_addr.is_multiple_of(8) {
        return false;
    }
    let page = release_addr & !(PAGE_SIZE - 1);
    // A firmware word inside the cell slots or the stack arena: its VA is a
    // cell image or a stack, not the word.
    if paging::identity_map_refused("spin-table release word", page, PAGE_SIZE) {
        return false;
    }
    let transient = paging::virt_to_phys(page).is_none();
    if transient {
        let flags = Flags::from_bits(
            Flags::VALID | Flags::READ | Flags::WRITE | Flags::ACCESSED | Flags::DIRTY,
        );
        // AArch64 maps and unmaps without a deferred invalidation, so the
        // batch stays empty.
        let mut batch = crate::memory::frame_run::Batch::new();
        let mapped = crate::memory::frame::with(|allocator| {
            paging::map_page(allocator, &mut batch, page, page, flags).is_ok()
        });
        let _ = batch.complete();
        if mapped != Some(true) {
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
        let _ = paging::unmap_page(&mut crate::memory::frame_run::Batch::new(), page);
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
#[cfg(not(cellos_posture = "release"))]
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

/// Rust entry point of a secondary core started by `PSCI_CPU_ON`.
///
/// Reached from `_secondary_entry` with the boot core's translation regime
/// installed, this hart's stack in place, interrupts masked, and `hart_id` in
/// x0. Returns only by never returning: the hart parks and lives out its life
/// in the timer/IPI trap path, which is where its scheduler round runs.
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
