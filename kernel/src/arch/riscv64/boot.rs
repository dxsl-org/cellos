// SPDX-License-Identifier: MPL-2.0
//! `boot`: the riscv64 phases of `kmain` and its hooks in the neutral tail.
//!
//! [`early`] runs up to and including paging; [`platform_init`] runs after
//! the heap, up to and including the VirtIO MMIO bring-up and the deferred
//! IOMMU. The remaining functions are called at fixed points of the neutral
//! tail and keep the place the riscv64 code had there.

pub mod cpu_features;
pub mod firmware;
pub mod platform;

use crate::boot::{
    BootInfoSource, BootStorage, ClockReady, ConsoleReady, IommuReady, IrqReady, PciReady,
    PlatformReady,
};
use crate::log_info;
use crate::memory;
use hal::Arch;

/// The kernel root, kept for the fault snapshot's page-table walk.
#[cfg(feature = "test-hooks")]
static TEST_SAFE_ROOT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Write one byte to the SBI console; the panic handler writes through it too.
#[inline(always)]
pub(crate) fn putc(c: u8) {
    let _ = crate::hal::sbi::console_putchar(c);
}

/// Write `s` to the SBI console.
pub(crate) fn puts(s: &str) {
    for c in s.bytes() {
        putc(c);
    }
}

/// What the early phase hands [`platform_init`].
pub struct EarlyPlatform {
    boot_info: BootInfoSource,
}

/// Boot hart, DTB, platform facts, PLIC base, UART, `ARCH.init`, boot memory
/// and the kernel page tables (SUM on).
pub(crate) fn early(hartid: usize, dtb: usize) -> EarlyPlatform {
    crate::sched::smp::set_boot_physical_hart(hartid);
    let dtb = crate::boot::effective_dtb(dtb);
    crate::cpu_features::detect(dtb);
    // Parse DTB for MMIO bases before any driver or paging init.
    platform::init(dtb);
    // Set runtime PLIC base before the later RV64 PLIC initialization consumes it.
    crate::platform::with(|p| hal::common::plic::set_plic_base(p.plic_base));
    // 0. Initialize UART immediately for early logging
    crate::dev::uart::init();

    // 1. Initialize HAL (Architecture specific) - Early Trap Setup
    hal::ARCH.init();

    crate::boot_banner();
    if crate::cpu_features::has_h_ext() {
        puts("[cpu] H-extension: detected\n");
    } else {
        puts("[cpu] H-extension: not present\n");
    }

    let (boot_info, frame_allocator) = crate::boot_memory(dtb);
    let mmap_entries = boot_info.get().memory_map();

    // The allocator stays local until paging is active (build-then-publish,
    // as on AArch64).

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
        #[cfg(feature = "test-hooks")]
        TEST_SAFE_ROOT.store(root_table_phys, core::sync::atomic::Ordering::Release);
        // SAFETY: boot, once, before any other frame-allocator access.
        unsafe { memory::frame::install(frame_allocator) };
        log_info("Paging activated");
        log_info("Frame allocator initialized");
        // Set sstatus.SUM=1 so S-mode (kernel) can access USER-mapped pages throughout
        // the kernel lifetime. VirtIO/peripheral MMIO is mapped USER=1 for Driver Cells
        // (U-mode). Without SUM=1 the kernel's tech-debt VirtIO drivers fault at early-boot
        // MMIO init. In Cellos's SAS+LBI model security comes from Rust type safety, not
        // hardware USER-bit separation for kernel-vs-cell — SUM=1 is safe and intentional.
        crate::arch::user_access::boot_open();
    }

    EarlyPlatform { boot_info }
}

/// PLIC, log backend, snapshot scratch, UART RX, the VirtIO MMIO devices, and
/// the IOMMU left pending for the Platform Cell.
pub(crate) fn platform_init(early: EarlyPlatform) -> PlatformReady {
    let EarlyPlatform { boot_info } = early;
    // 5. Hardware Abstraction Layer (HAL) Initialization
    // Initialize PLIC for RISC-V external IRQs.
    if crate::board::active().has_driver(cellos_boards::DriverId::PlicSifive) {
        if let Some((context, irqs, irq_count)) = platform::riscv_plic_init_data() {
            crate::hal::common::plic::init(context, &irqs[..irq_count]);
        } else {
            log::warn!("[plic] no active RV64 context mapping; external IRQs stay disabled");
        }
    } else {
        log::info!("[plic] disabled by active board descriptor");
    }
    log_info("HAL initialized (PLIC enabled)");

    // 6. Logger & Drivers & FS
    // Registered at entry, so the call is a no-op here.
    crate::dev::uart::init();

    // Phase 07: reserve the warm-snapshot capture's scratch workspace from the
    // frame allocator — the capture stages its own stack and buffers there, so
    // they can never be inside a captured run.  After the frame allocator and
    // the heap (and now the log backend), before any cell or capture can run.
    // A no-op unless the build is `snapshot-qualified`; a build without a
    // workspace refuses every capture.
    crate::boot::reserve_snapshot_scratch();
    crate::dev::uart::init_input();
    crate::dev::init();

    crate::dev::iommu::set_deferred_init_pending();
    // activate_isolation() runs inside iommu::try_deferred_init(), after init()
    // probes the IOMMU device the Platform Cell registered. Nothing is probed
    // here, so there is nothing to activate yet.

    PlatformReady {
        console: ConsoleReady { boot_info },
        irq: IrqReady,
        clock: ClockReady,
        pci: Some(PciReady::Gpex),
        iommu: IommuReady::Deferred,
        storage: BootStorage::VirtioMmio,
    }
}

// ── Hooks in the neutral tail of `kmain` ─────────────────────────────────────
// Each runs where the riscv64 code it holds ran before the split; an empty hook
// marks a point where only another architecture has code.

/// After the signed policy is loaded.
pub(crate) fn fixtures_after_policy() {
    // Phase 20: hot-migration state-transfer self-test.
    crate::cell::state_stash::self_test();
}

/// Right after `task::init`, before every unrelated boot suite.
pub(crate) fn fixtures_after_task_init() {
    // This test-only branch must precede every unrelated boot suite: its
    // terminal is the sole evidence for opcode 214's SAS ownership path.
    #[cfg(all(feature = "native-domains", feature = "getrandom-sas-test"))]
    {
        crate::sched::smp::start_secondaries();
        // Erase the diverging return type so the generic boot path remains
        // typechecked without a feature-specific unreachable-code warning.
        let exit = |success| -> () { crate::qemu_exit(success) };
        exit(crate::task::user_copy_tests::run_getrandom_primary());
    }
}

/// After the loader corpus, before the trust-model self-tests.
pub(crate) fn fixtures_after_loader_corpus() {
    #[cfg(feature = "test-hooks")]
    if crate::arch::task::path_selftest::self_test() {
        log_info("cwd-path self-test PASS");
    } else {
        log_info("cwd-path self-test FAIL");
    }
    #[cfg(feature = "test-hooks")]
    if crate::arch::task::fstat_selftest::self_test() {
        log_info("fstat self-test PASS");
    } else {
        log_info("fstat self-test FAIL");
    }
}

/// Right after the secondaries start.
pub(crate) fn fixtures_after_secondaries() {
    #[cfg(feature = "test-hooks")]
    crate::arch::memory::tlb_shootdown_selftest::run_primary();
    #[cfg(feature = "test-hooks")]
    crate::arch::cell::provenance_selftest::run_primary();
    #[cfg(all(feature = "native-domains", feature = "test-hooks"))]
    crate::memory::address_space::address_space_tests::run_primary();

    #[cfg(feature = "test-hooks")]
    crate::loader::atomic_publication_tests::run_governed_success_after_secondaries();
}

/// Board MMIO the domain supervisor registry keeps shared with the kernel:
/// CLINT, PLIC, the UART page (when there is a UART) and the RTC page.
#[cfg(feature = "native-domains")]
pub(crate) fn supervisor_device_spans() -> impl Iterator<Item = (usize, usize)> {
    let (clint_base, plic_base, plic_size, uart_region, rtc_region) = crate::platform::with(|p| {
        (
            p.clint_base,
            p.plic_base,
            p.plic_size,
            p.uart_base & !0xFFFF,
            p.rtc_base & !0xFFF,
        )
    });
    [
        Some((clint_base, clint_base + 0x10000)),
        Some((plic_base, plic_base + plic_size)),
        (uart_region != 0).then_some((uart_region, uart_region + 0x1000)),
        Some((rtc_region, rtc_region + 0x1000)),
    ]
    .into_iter()
    .flatten()
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
    #[cfg(all(feature = "native-domains", feature = "test-hooks"))]
    crate::arch::task::context_handoff_selftest::run_primary();
    #[cfg(feature = "test-hooks")]
    crate::arch::task::retirement_selftest::run_primary();
    #[cfg(all(feature = "native-domains", feature = "test-hooks"))]
    {
        crate::task::user_copy_tests::run_primary();
        crate::task::ipc_wire_selftest::run_primary(crate::sched::smp::online_hart_count());
        crate::loader::domain_admission::run_selftest();
        crate::ipc::futex::run_selftest();
        crate::task::domain_grant::run_selftest();
        crate::task::grant_gate_selftest::run_primary();
        crate::task::rt_wake_selftest::self_test();
    }
}

/// After the IOMMU page-table fixture: the all-hart park hook.
pub(crate) fn fixtures_park() {
    // Phase 07 step 3: the all-hart park hook, driven on the real harts of this
    // boot. The fixture requests, verifies and releases a park through the same
    // protocol and the same trap-path hook the capture preflight uses, and emits
    // `S22-RV64-PARK: PASS harts=<n>` (or `FAIL`) itself — the native-domain lane
    // requires that terminal, so a fixture that stops running, or a call site that
    // stops being reached, turns the lane red instead of silently disappearing.
    // Placed with the other boot fixtures: after every fixture that needs the
    // second hart (the park is a bounded stop, not a background stall) and before
    // init exists, so it has the machine to itself.
    #[cfg(feature = "test-hooks")]
    if !crate::sched::quiesce::lane::run_primary() {
        // The fixture's own marker is the verdict: either `S22-RV64-PARK: FAIL`
        // (a step of the witness did not hold) or `S22-RV64-PARK: UNAVAILABLE`
        // (the readiness gate found the second hart not taking traps before any
        // request was made, so this boot could not witness the hook at all).
        log::error!("park-hook fixture did not witness the park hook; see its marker");
    }
}

/// Layer-2 hardware security self-tests: none on riscv64.
pub(crate) fn fixtures_hardware() {}

/// The framebuffer console: x86_64 only.
pub(crate) fn framebuffer_console(_console: &ConsoleReady) {}

/// Board diagnostic probe byte: aarch64 boards only.
pub(crate) fn diag_probe(_byte: u8) {}

/// After init is published: the U-mode probes.
pub(crate) fn fixtures_user_mode() {
    // Ring-3 smoke test: spawn a minimal U-mode task that logs and exits.
    // RISC-V only — task writes RISC-V machine code directly.
    // Expected serial output: "Hi from U-mode!\n" followed by task exit.
    #[cfg(feature = "test-hooks")]
    match crate::task::user_hello::spawn() {
        Ok(tid) => {
            puts("[task] spawning user_hello at ");
            // Print tid as decimal (max 20 digits for usize)
            let mut buf = [0u8; 20];
            let mut n = tid;
            let mut i = 20usize;
            if n == 0 {
                i -= 1;
                buf[i] = b'0';
            } else {
                while n > 0 {
                    i -= 1;
                    buf[i] = b'0' + (n % 10) as u8;
                    n /= 10;
                }
            }
            let _ = core::str::from_utf8(&buf[i..]).map(puts);
            puts("\n");
            let _ = tid; // suppress unused warning
        }
        Err(_) => log_info("[task] user_hello spawn failed"),
    }

    // Deliberately underflow a U-mode test task into the upper guard page. The
    // fault handler must terminate only that task and let boot continue.
    #[cfg(feature = "test-hooks")]
    if crate::arch::task::stack_overflow_probe::spawn().is_err() {
        log_info("[stack-guard] deliberate overflow probe spawn FAIL");
    }
}

/// The kernel log level once boot is done, unless `[logging]` set one.
pub(crate) fn quiet_log_level() -> log::LevelFilter {
    log::LevelFilter::Warn
}

/// Board probes between the interrupt enable and the idle loop: none.
pub(crate) fn idle_probes() {}

/// Emit an allocation-free RV64 fault snapshot before the panic path.
#[cfg(feature = "test-hooks")]
#[no_mangle]
pub extern "C" fn vi_rv64_kernel_fault_snapshot(satp: usize, stval: usize) {
    fn put_hex(value: usize) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for shift in (0..16).rev().map(|n| n * 4) {
            let _ = crate::hal::sbi::console_putchar(HEX[(value >> shift) & 0xf]);
        }
    }

    fn put_label(label: &[u8], value: usize) {
        for byte in label {
            let _ = crate::hal::sbi::console_putchar(*byte);
        }
        put_hex(value);
    }

    unsafe fn read_pte(table_phys: usize, index: usize) -> usize {
        (table_phys as *const usize).add(index).read_volatile()
    }

    fn next_table(entry: usize) -> usize {
        ((entry >> 10) & 0x003f_ffff_ffff_ffff) << 12
    }

    for byte in b"\n[selftest] RV64-FAULT satp=0x" {
        let _ = crate::hal::sbi::console_putchar(*byte);
    }
    put_hex(satp);
    for byte in b" stval=0x" {
        let _ = crate::hal::sbi::console_putchar(*byte);
    }
    put_hex(stval);

    let kernel_root = TEST_SAFE_ROOT.load(core::sync::atomic::Ordering::Acquire);
    if kernel_root != 0 {
        let kernel_satp = (8usize << 60) | (kernel_root >> 12);
        // SAFETY: this test-only panic snapshot switches to the known-safe
        // shared root so it can inspect private page-table memory by physical VA.
        unsafe {
            core::arch::asm!(
                "csrw satp, {satp}",
                "sfence.vma zero, zero",
                satp = in(reg) kernel_satp,
                options(nostack)
            );
        }
        let root_phys = (satp & ((1usize << 44) - 1)) << 12;
        let vpn2 = (stval >> 30) & 0x1ff;
        let vpn1 = (stval >> 21) & 0x1ff;
        let vpn0 = (stval >> 12) & 0x1ff;
        let pte2 = unsafe { read_pte(root_phys, vpn2) };
        let pte1 = if pte2 & 1 != 0 {
            unsafe { read_pte(next_table(pte2), vpn1) }
        } else {
            0
        };
        let pte0 = if pte1 & 1 != 0 {
            unsafe { read_pte(next_table(pte1), vpn0) }
        } else {
            0
        };
        put_label(b" pte2=0x", pte2);
        put_label(b" pte1=0x", pte1);
        put_label(b" pte0=0x", pte0);
    }
}
