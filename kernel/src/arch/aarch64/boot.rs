// SPDX-License-Identifier: MPL-2.0
//! `boot`: the aarch64 phases of `kmain` and its hooks in the neutral tail.
//!
//! [`early`] runs up to and including paging; [`platform_init`] runs after
//! the heap, up to and including the VirtIO MMIO bring-up. The remaining
//! functions are called at fixed points of the neutral tail and keep the
//! place the aarch64 code had there.

use crate::boot::{
    BootInfoSource, BootStorage, ClockReady, ConsoleReady, IommuReady, IrqReady, PlatformReady,
};
use crate::log_info;
use crate::memory;
use hal::backend::ArmPlatform as _;
use hal::Arch;

/// Write `s` to the board's early console.
pub(crate) fn puts(s: &str) {
    for c in s.bytes() {
        crate::hal::backend::Board::early_putc(c);
    }
}

/// What the early phase hands [`platform_init`].
pub struct EarlyPlatform {
    boot_info: BootInfoSource,
}

/// DTB, platform facts, board console, `ARCH.init` (CFI, MTE, IRQ controller,
/// timer, vectors), boot memory and the kernel page tables.
pub(crate) fn early(_hartid: usize, dtb: usize) -> EarlyPlatform {
    let dtb = crate::boot::effective_dtb(dtb);
    crate::cpu_features::detect(dtb);
    // Parse DTB for MMIO bases before any driver or paging init.
    crate::platform::init(dtb);
    // 0. Initialize UART immediately for early logging
    crate::hal::backend::Board::console_init();

    // 1. Initialize HAL (Architecture specific) - Early Trap Setup
    hal::ARCH.init();

    crate::boot_banner();
    let (boot_info, frame_allocator) = crate::boot_memory(dtb);
    let mmap_entries = boot_info.get().memory_map();

    // The allocator stays local until paging is active: the Spinlock uses
    // exclusive atomics that require Normal memory attributes.

    // 3. Paging (Virtual Memory) Setup
    {
        let mut frame_allocator = frame_allocator;
        log_info("Initializing paging...");
        let root_table_phys =
            memory::paging::init_kernel_paging(&mut frame_allocator, mmap_entries)
                .expect("Failed to initialize paging");
        log_info("Paging initialized");
        log_info("Activating paging...");
        unsafe {
            memory::paging::activate_paging(root_table_phys);
        }
        *memory::paging::KERNEL_ROOT.lock() = Some(root_table_phys);
        crate::hal::domain::record_kernel_ttbr0(root_table_phys);
        unsafe {
            core::ptr::write(
                &mut *memory::frame::FRAME_ALLOCATOR.lock(),
                Some(frame_allocator),
            );
        }
        log_info("Paging activated");
        log_info("Frame allocator initialized");
    }

    EarlyPlatform { boot_info }
}

/// Log backend, EL2 monitor probe, snapshot scratch, console RX IRQ and the
/// VirtIO MMIO devices. VirtIO MMIO DMA starts here with no IOMMU.
pub(crate) fn platform_init(early: EarlyPlatform) -> PlatformReady {
    let EarlyPlatform { boot_info } = early;
    // 5. Hardware Abstraction Layer (HAL) Initialization
    log_info("HAL initialized (PLIC enabled)");

    // 6. Logger & Drivers & FS
    // Registers the log backend.
    crate::dev::uart::init();
    {
        use hal::backend::{ArmEl2Monitor, El2Monitor};
        if El2Monitor::PRESENT {
            let current_el = crate::arch::cpu::current_el();
            log::info!(
                "[pi-monitor] entry EL={} host EL={} HVC ready={}",
                El2Monitor::entry_el(),
                current_el >> 2,
                El2Monitor::is_ready()
            );
            let (booted_el2, hvc_result, op0_seen) = El2Monitor::probe_status();
            log::info!(
                "[pi-monitor] probe booted_el2={} hvc_result=0x{:x} op0_seen={}",
                booted_el2,
                hvc_result,
                op0_seen
            );
            crate::arch::vm::pi_monitor_smoke();
        }
    }

    // Phase 07: reserve the warm-snapshot capture's scratch workspace from the
    // frame allocator — the capture stages its own stack and buffers there, so
    // they can never be inside a captured run.  After the frame allocator and
    // the heap (and now the log backend), before any cell or capture can run.
    // A no-op unless the build is `snapshot-qualified`; a build without a
    // workspace refuses every capture.
    crate::boot::reserve_snapshot_scratch();
    if matches!(
        <hal::backend::Board as hal::backend::ArmPlatform>::DTB_POLICY,
        hal::backend::DtbPolicy::Optional
    ) {
        let (start, end) = crate::boot::fallback_dtb_ram_range();
        log::info!("[boot] DTB RAM range {:#x}..{:#x}", start, end);
    }
    if crate::hal::backend::Board::CONSOLE_RX_IRQ {
        // IRQ delivery must start only after the heap-backed queue exists.
        crate::dev::uart::init_input();
        crate::hal::backend::Board::console_enable_rx_irq();
    }
    crate::dev::init();

    PlatformReady {
        console: ConsoleReady { boot_info },
        irq: IrqReady,
        clock: ClockReady,
        // ARM64 virt uses VirtIO MMIO (not PCIe); accessing 0x3F000000 on QEMU
        // virt 7+ triggers a Synchronous External Abort — no Platform Cell.
        pci: None,
        iommu: IommuReady::Absent {
            reason: "no IOMMU: VirtIO MMIO DMA started in dev::init",
        },
        storage: BootStorage::VirtioMmio,
    }
}

// ── Hooks in the neutral tail of `kmain` ─────────────────────────────────────
// Each runs where the aarch64 code it holds ran before the split; an empty hook
// marks a point where only another architecture has code.

/// After the signed policy is loaded.
pub(crate) fn fixtures_after_policy() {
    // Phase 20: hot-migration state-transfer self-test.
    crate::cell::state_stash::self_test();
}

/// Right after `task::init`, before every unrelated boot suite.
pub(crate) fn fixtures_after_task_init() {}

/// After the loader corpus, before the trust-model self-tests.
pub(crate) fn fixtures_after_loader_corpus() {}

/// Right after the secondaries start.
pub(crate) fn fixtures_after_secondaries() {
    // AArch64 has no equivalent of the RV64 shootdown fixture at boot, so the
    // SGI delivery path is exercised here: a hart that is online but deaf to the
    // kernel's cross-hart IPI would otherwise look perfectly healthy.
    #[cfg(not(cellos_posture = "release"))]
    crate::sched::smp::run_ipi_selftest();
}

/// Board MMIO the domain supervisor registry keeps shared with the kernel.
#[cfg(feature = "native-domains")]
pub(crate) fn supervisor_device_spans() -> impl Iterator<Item = (usize, usize)> {
    <hal::backend::Board as hal::backend::ArmResources>::SUPERVISOR_DEVICE_MMIO
        .iter()
        .map(|span| (span.base, span.end))
}

/// The browser-engine profile's Tier 2 admission posture: x86_64 only.
#[cfg(all(
    feature = "native-domains",
    any(feature = "policy-required", feature = "production-relay-image")
))]
pub(crate) fn browser_engine_admission() -> bool {
    false
}

/// After the native-domain SAS scheduler fixture.
pub(crate) fn fixtures_domain() {
    // Phase 03 domain grant lifecycle on AArch64 test images. The same
    // kernel-side fixtures the RV64 lane runs drive the production
    // `handle_syscall` entry points with real `TaskAddressSpace::Domain` tasks:
    // rights, transactional publish, revoke and the deferred outcome. They are
    // not the two-cell public handoff, which stays RV64-only (`grant-pair`,
    // `cells/tests/tier2-grant-*`).
    //
    // Skipped at EL2 for the same reason `enable_for_boot` refuses there: the
    // EL2 switch has no root argument, so the machine cannot enter a private
    // root and the fixture would report against a root it cannot run.
    #[cfg(all(feature = "native-domains", feature = "test-hooks"))]
    if crate::hal::aarch64::el2::is_el2() {
        log::info!("[grant] AArch64 EL2 (development-Silo machine): domain grant fixtures skipped");
    } else {
        crate::task::domain_grant::run_selftest();
        crate::task::grant_gate_selftest::run_primary();
    }
}

/// After the IOMMU page-table fixture: the all-hart park hook (riscv64).
pub(crate) fn fixtures_park() {}

/// Layer-2 hardware security self-test (MTE), after all hardware init and
/// before the first cell spawn.
pub(crate) fn fixtures_hardware() {
    #[cfg(feature = "test-hooks")]
    crate::arch::layer2_selftest::run_mte_selftest();
}

/// The framebuffer console: x86_64 only.
pub(crate) fn framebuffer_console(_console: &ConsoleReady) {}

/// Board diagnostic probe byte (`V`: init published, `F`: init failed).
pub(crate) fn diag_probe(byte: u8) {
    crate::hal::backend::Board::diag_probe(byte);
}

/// After init is published: the riscv64 U-mode probes.
pub(crate) fn fixtures_user_mode() {}

/// The kernel log level once boot is done, unless `[logging]` set one. Test
/// images keep Info: their lane reads witnesses emitted after init.
pub(crate) fn quiet_log_level() -> log::LevelFilter {
    #[cfg(feature = "test-hooks")]
    return log::LevelFilter::Info;
    #[cfg(not(feature = "test-hooks"))]
    return log::LevelFilter::Warn;
}

/// Board probes between the interrupt enable and the idle loop. Boards with
/// idle probes never return from here.
pub(crate) fn idle_probes() {
    // Probe 'Q': fires ONLY if no IRQ preempted the code between daifclr and here.
    // Q fires  → BCM2835 IRQ is not actually delivered to CPU on QEMU (pending but silent).
    // Q absent → IRQ fired immediately after daifclr (expected); check if H/G appear.
    crate::hal::backend::Board::diag_probe(b'Q');

    // Board IRQ-state snapshot immediately after enabling IRQs (RPi3: the
    // "K<src_bit8><pend_bit1><src_nibble>" probe; a no-op elsewhere).
    crate::hal::backend::Board::irq_snapshot_probe();

    // Probe 'L': fires once per idle loop iteration, for the board's first
    // `IDLE_PROBES` iterations only (RPi3: 4) to avoid flood.
    // If 'L' never appears after 'K', the code never reaches the idle loop.
    if <hal::backend::Board as hal::backend::ArmPlatform>::IDLE_PROBES != 0 {
        static IDLE_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        loop {
            let n = IDLE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            // reason: `IDLE_PROBES` is a per-board const; on boards with no idle
            // probe it is 0, and this whole block is then dead.
            #[allow(clippy::absurd_extreme_comparisons)]
            if n < <hal::backend::Board as hal::backend::ArmPlatform>::IDLE_PROBES {
                crate::hal::backend::Board::diag_probe(b'L');
            }
            crate::task::yield_cpu();
            crate::hal::ARCH.wait_for_interrupt();
        }
    }
}
