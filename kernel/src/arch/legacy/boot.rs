// SPDX-License-Identifier: MPL-2.0
//! `boot`: the 32-bit phases of `kmain` (riscv32 Nano, AArch32, x86_32): bare
//! physical memory, no heap-time devices, no cells.

pub mod cpu_features;
pub mod firmware;

use crate::boot::{
    BootInfoSource, BootStorage, ClockReady, ConsoleReady, IommuReady, IrqReady, PlatformReady,
};
use crate::log_info;
use crate::memory;
use hal::Arch;

/// Write one byte to the early console; the panic handler writes through it
/// too.
#[inline(always)]
pub(crate) fn putc(c: u8) {
    #[cfg(target_arch = "riscv32")]
    {
        let _ = crate::hal::sbi::console_putchar(c);
    }
    #[cfg(target_arch = "arm")]
    {
        crate::hal::uart_pl011::putchar(c);
    }
    #[cfg(target_arch = "x86")]
    {
        crate::hal::uart_16550::putchar(c);
    }
}

/// Write `s` to the early console.
pub(crate) fn puts(s: &str) {
    for c in s.bytes() {
        putc(c);
    }
}

/// What the early phase hands [`platform_init`].
pub struct EarlyPlatform {
    boot_info: BootInfoSource,
}

/// UART, `ARCH.init`, boot memory, the frame allocator and bare physical
/// addressing (RV32 SATP=0, x86_32 CR0.PG=0, AArch32 MMU off).
pub(crate) fn early(_hartid: usize, dtb: usize) -> EarlyPlatform {
    let dtb = crate::boot::effective_dtb(dtb);
    crate::cpu_features::detect(dtb);
    // 0. Initialize UART immediately for early logging
    #[cfg(target_arch = "riscv32")]
    crate::dev::uart::init();
    #[cfg(target_arch = "arm")]
    crate::hal::uart_pl011::init();
    #[cfg(target_arch = "x86")]
    crate::hal::uart_16550::init();

    // 1. Initialize HAL (Architecture specific) - Early Trap Setup
    hal::ARCH.init();

    crate::boot_banner();
    let (boot_info, frame_allocator) = crate::boot_memory(dtb);

    // These targets run without an MMU, so the allocator is published at once.
    // SAFETY: boot, once, before any other frame-allocator access.
    unsafe { memory::frame::install(frame_allocator) };
    log_info("Frame allocator initialized");

    // Bare physical: RV32 Nano (SATP=0), x86_32 (CR0.PG=0), AArch32 (MMU off).
    {
        memory::paging::init_bare();
        log_info("Paging: bare physical");
    }

    EarlyPlatform { boot_info }
}

/// Log backend and snapshot scratch; no devices, no DMA, no boot storage.
pub(crate) fn platform_init(early: EarlyPlatform) -> PlatformReady {
    let EarlyPlatform { boot_info } = early;
    // 5. Hardware Abstraction Layer (HAL) Initialization
    log_info("HAL initialized (PLIC enabled)");

    // 6. Logger & Drivers & FS
    // Registers the log backend on ARM; riscv32 registered it at entry.
    crate::dev::uart::init();

    // Phase 07: reserve the warm-snapshot capture's scratch workspace (a no-op
    // unless the build is `snapshot-qualified`).
    crate::boot::reserve_snapshot_scratch();

    PlatformReady {
        console: ConsoleReady { boot_info },
        irq: IrqReady,
        clock: ClockReady,
        pci: None,
        iommu: IommuReady::Absent {
            reason: "32-bit Nano: bare physical, no DMA-capable driver",
        },
        storage: BootStorage::None,
    }
}

// ── Hooks in the neutral tail of `kmain` ─────────────────────────────────────
// The 32-bit targets have no code at these points; the hooks inside the
// `cellos_wide` spawn block are not called here.

/// After the signed policy is loaded.
pub(crate) fn fixtures_after_policy() {}

/// Right after `task::init`, before every unrelated boot suite.
pub(crate) fn fixtures_after_task_init() {}

/// After the loader corpus, before the trust-model self-tests.
pub(crate) fn fixtures_after_loader_corpus() {}

/// Right after the secondaries start.
pub(crate) fn fixtures_after_secondaries() {}

/// After the native-domain SAS scheduler fixture.
pub(crate) fn fixtures_domain() {}

/// After the IOMMU page-table fixture.
pub(crate) fn fixtures_park() {}

/// After init would be published.
pub(crate) fn fixtures_user_mode() {}

/// The kernel log level once boot is done, unless `[logging]` set one.
pub(crate) fn quiet_log_level() -> log::LevelFilter {
    log::LevelFilter::Warn
}

/// Board probes between the interrupt enable and the idle loop: none.
pub(crate) fn idle_probes() {}
