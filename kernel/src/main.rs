// SPDX-License-Identifier: MPL-2.0
//! Cellos Kernel - Entry point

#![cfg_attr(not(all(test, not(target_os = "none"))), no_std)]
#![cfg_attr(not(all(test, not(target_os = "none"))), no_main)]
#![cfg_attr(not(all(test, not(target_os = "none"))), feature(alloc_error_handler))]

#[cfg(all(feature = "board-vf2", feature = "board-pioneer"))]
compile_error!(
    "Conflicting RISC-V board features: `board-vf2` and `board-pioneer` cannot be enabled together."
);

#[cfg(all(feature = "board-rpi3", feature = "board-rpi4"))]
compile_error!(
    "Conflicting AArch64 board features: `board-rpi3` and `board-rpi4` cannot be enabled together."
);

#[cfg(all(
    feature = "production-relay-image",
    any(
        feature = "dev-policy-key",
        feature = "dev-signing-key",
        feature = "dev-weak-rng",
        feature = "test-hooks",
        feature = "maintenance-mode"
    )
))]
compile_error!("production relay kernel excludes development, test, and recovery features");

extern crate alloc;

#[cfg(not(all(test, not(target_os = "none"))))]
use core::panic::PanicInfo;

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static TEST_SAFE_ROOT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

// Core kernel modules
pub mod acpi;
#[cfg(feature = "test-hooks")]
mod admission;
pub mod audit;
mod board;
pub mod boot;
pub mod cell;
pub mod ed25519; // Ed25519 verify (no_std) for signed operator policy (P5 spike)
pub mod fast_ipc; // Kernel-owned fast-IPC dispatch table (canonical instance)
pub mod fs; // Filesystem
pub mod hypervisor; // EL2 VMM kernel support (Phase 03+)
pub mod layer2_selftest; // Layer-2 hardware security self-tests (test-hooks only)
pub mod loader;
pub mod measurement_log; // Per-Cell integrity measurement (IMA-style, TPM-free)
pub mod memory;
pub mod policy; // Signed operator policy (P5b) — headless consent
pub mod resource_registry;
pub mod sha256; // Self-contained SHA-256 for measurement
pub mod signing; // Cell binary signing (Ed25519) — verification gate at spawn time
pub mod snapshot;
pub mod task; // Renamed from 'process'
              // pub mod arch; // Moved to HAL
pub extern crate hal; // HAL (Architecture specific)
use boot::BootInfo;
use hal::Arch;

// Internal utilities
mod cpu_features;
pub mod platform;
mod sync;
#[cfg(test)]
pub(crate) static TEST_STATE_LOCK: sync::Spinlock<()> = sync::Spinlock::new(());

/// Signal QEMU to exit with a success (0) or failure (1) code.
///
/// Only available under the `test-hooks` feature — never call this in
/// production builds. The kernel integration-test harness uses this
/// instead of parsing serial output for "PASS"/"FAIL" banners.
///
/// Device addresses: RISC-V = SiFive test 0x100000, ARM64 = semihosting,
/// x86_64 = isa-debug-exit (iobase 0xF4).
#[cfg(feature = "test-hooks")]
pub fn qemu_exit(success: bool) -> ! {
    use qemu_exit::QEMUExit;
    #[cfg(target_arch = "riscv64")]
    {
        // SAFETY: 0x100000 is the SiFive test device address on the QEMU `virt`
        // machine used by this test-only exit path.
        unsafe {
            qemu_exit::RISCV64::new(0x100000).exit(if success { 0 } else { 1 });
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        qemu_exit::AArch64::new().exit(if success { 0 } else { 1 });
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: 0xF4 is the QEMU `isa-debug-exit` I/O port wired in the x86
        // test harness (`-device isa-debug-exit,iobase=0xf4`); writing it exits
        // QEMU. Constructing the port handle is `unsafe` (arbitrary port I/O).
        unsafe {
            qemu_exit::X86::new(0xF4, 0).exit(if success { 0 } else { 1 });
        }
    }
    // Fallback for other arches: spin forever so the test times out clearly.
    #[allow(clippy::empty_loop)]
    // reason: on riscv/aarch64/x86_64 the arch-specific `.exit()` above diverges,
    // so this loop is unreachable there; it only executes on a hypothetical arch
    // with no qemu_exit backend.
    #[allow(unreachable_code)]
    loop {}
}

// Re-export types for convenience
pub use types::*;

// Embed Init Binary (stripped by build.rs, served from EMBEDDED_OUT_DIR).
// RV32 Nano (Phase 31) has no init ELF; x86_64 is now included (Phase 04).
#[cfg(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
))]
static INIT_ELF: &[u8] = include_bytes!(concat!(env!("EMBEDDED_OUT_DIR"), "/init"));

/// Kernel entry point called from HAL boot code
#[no_mangle]
pub extern "C" fn kmain(hartid: usize, dtb: usize) -> ! {
    #[cfg(target_arch = "riscv64")]
    task::smp::set_boot_physical_hart(hartid);
    #[cfg(not(target_arch = "riscv64"))]
    let _hartid = hartid;
    let dtb = boot::effective_dtb(dtb);
    cpu_features::detect(dtb);
    // Parse DTB for MMIO bases before any driver or paging init.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    crate::platform::init(dtb);
    // Set runtime PLIC base before the later RV64 PLIC initialization consumes it.
    #[cfg(target_arch = "riscv64")]
    crate::platform::with(|p| hal::common::plic::set_plic_base(p.plic_base));
    // 0. Initialize UART immediately for early logging
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    task::drivers::uart::init();
    #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
    crate::hal::uart_bcm_mini::init();
    #[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
    crate::hal::uart_pl011::init();
    #[cfg(target_arch = "arm")]
    crate::hal::uart_pl011::init();
    #[cfg(target_arch = "x86_64")]
    {
        let board = crate::board::selected();
        if !board.has_driver(cellos_boards::DriverId::Uart16550PortIo) {
            panic!("[board] selected x86 board does not enable the early UART");
        }
        let soc = crate::board::selected_x86_64_soc();
        crate::hal::uart_16550::configure(soc.com1.base, soc.com1.irq);
        crate::hal::uart_16550::init();
        crate::hal::uart_16550::puts(
            "[x86-gate] configured 16550 ready: TX + polled RX; IRQ pending MADT\n",
        );
        if crate::hal::uart_16550::poll_input().is_some() {
            crate::hal::uart_16550::puts("[x86-gate] COM1 polled RX observed\n");
        }
    }
    #[cfg(target_arch = "x86")]
    crate::hal::uart_16550::init();

    // Set HHDM base for LAPIC/IOAPIC MMIO access AND for phys_to_virt.
    // Limine maps RAM at HHDM_BASE+phys (no identity mapping of physical RAM).
    // This must be called before FrameAllocator::new_from_map.
    #[cfg(target_arch = "x86_64")]
    {
        let hhdm = crate::boot::limine::get_hhdm_offset().unwrap_or(0);
        crate::hal::apic::set_hhdm_base(hhdm);
        crate::memory::frame::set_phys_offset(hhdm as usize);
        // Propagate the HHDM offset to the HAL PML4 walker so walk_create /
        // walk_read can dereference physical PTE addresses via HHDM virtual ptrs.
        crate::hal::paging::set_hhdm_offset(hhdm as usize);
        // Initialise KASLR seed from HHDM entropy + RDTSC.
        crate::memory::kaslr::init_kaslr(hhdm);
    }

    // 1. Initialize HAL (Architecture specific) - Early Trap Setup
    // x86_64: LAPIC is deferred until after paging sets up the MMIO mapping
    // (LAPIC phys 0xFEE00000 isn't in Limine's HHDM for MMIO regions).
    #[cfg(target_arch = "x86_64")]
    {
        crate::hal::gdt::init();
        crate::hal::idt::init();
        crate::hal::cet::init_kernel_cet(); // LAYER2-CET-INIT
        crate::hal::pku::init(); // LAYER2-PKU-INIT (requires IBT, checked inside)
        crate::hal::syscall::init();
        // apic::init_lapic() deferred — needs MMIO mapped via custom PML4
    }
    #[cfg(not(target_arch = "x86_64"))]
    hal::ARCH.init();
    // Define puts helper — arch-specific character output.
    let puts = |s: &str| {
        for c in s.bytes() {
            #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
            {
                let _ = crate::hal::sbi::console_putchar(c);
            }
            #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
            {
                crate::hal::uart_bcm_mini::putchar(c);
            }
            #[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
            {
                crate::hal::uart_pl011::putchar(c);
            }
            #[cfg(target_arch = "arm")]
            {
                crate::hal::uart_pl011::putchar(c);
            }
            #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
            {
                crate::hal::uart_16550::putchar(c);
            }
        }
    };

    // Restore log_info helper
    let log_info = |s: &str| {
        puts("[INFO] ");
        puts(s);
        puts("\n");
    };

    // Stable banner — CI greps for this exact string.
    puts("[Cellos] kernel boot v");
    puts(env!("CARGO_PKG_VERSION"));
    puts("\n");
    puts("Kernel started (Hart: 0, DTB: ...)\n");
    #[cfg(target_arch = "riscv64")]
    if cpu_features::has_h_ext() {
        puts("[cpu] H-extension: detected\n");
    } else {
        puts("[cpu] H-extension: not present\n");
    }

    // Parse bootloader information
    let boot_info_result = boot::parse_bootloader_info();

    // Check if Limine failed, if so, use fallback (SimpleBootInfo)
    let boot_info: &dyn BootInfo = match &boot_info_result {
        Ok(info) => info,
        Err(_) => {
            log_info("Limine not found, using QEMU/OpenSBI fallback");
            // aarch64-virt sizes the kernel region from the linker end symbol —
            // EMBEDDED_OVERRIDE images make the binary size unbounded.
            boot::fallback_boot_info(dtb)
        }
    };
    // Log physical base — non-default value confirms KASLR is active.
    {
        puts("[boot] kernel_phys_base=0x");
        let mut base = boot_info.kernel_base();
        let digits = b"0123456789abcdef";
        let mut hex_buf = [b'0'; 16];
        for i in (0..16).rev() {
            hex_buf[i] = digits[base & 0xf];
            base >>= 4;
        }
        if let Ok(s) = core::str::from_utf8(&hex_buf) {
            puts(s);
        }
        puts("\n");
    }

    // Initialize kernel subsystems

    // 1. Memory Management
    // Get memory map from Boot Info (Converted to Cellos format)
    let mmap_entries = boot_info.memory_map();

    // Initialize frame allocator with the largest usable region
    let frame_allocator = memory::frame::FrameAllocator::new_from_map(mmap_entries);
    log::info!(
        "[boot] allocator range {:#x}..{:#x} ({} bytes)",
        frame_allocator.memory_start(),
        frame_allocator.memory_end(),
        frame_allocator.total_frames() * 4096
    );

    // Targets that already have usable memory attributes (or intentionally run
    // without an MMU) can publish the allocator immediately. AArch64 must keep
    // it local until paging is active because its Spinlock uses exclusive
    // atomics that require Normal memory attributes; RV64 follows the same
    // build-then-publish lifecycle.
    #[cfg(any(
        target_arch = "riscv32",
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "arm",
    ))]
    unsafe {
        core::ptr::write(
            &mut *memory::frame::FRAME_ALLOCATOR.lock(),
            Some(frame_allocator),
        );
    }
    #[cfg(any(
        target_arch = "riscv32",
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "arm",
    ))]
    log_info("Frame allocator initialized");

    // Limine does not guarantee HHDM mappings for firmware-owned memory. The
    // parser requests a mapping for each RSDP/SDT range before dereferencing it.
    // Child tables must stay inside ACPI reclaimable/NVS or firmware-reserved
    // memory-map entries. Legacy BIOS ACPI records may live in the bounded
    // windows supplied by the selected x86 platform profile. Some BIOSes,
    // including q35, report other SDTs as Reserved, so table checksums remain
    // the final trust gate.
    #[cfg(target_arch = "x86_64")]
    let acpi_info = {
        let rsdp = crate::boot::limine::get_rsdp_ptr().unwrap_or(0);
        let soc = crate::board::selected_x86_64_soc();
        let mut map_physical = |physical: usize, length: usize| {
            let end = physical.checked_add(length)?;
            if length == 0 {
                return None;
            }
            let legacy_firmware = soc.legacy_bios_window.contains(physical, length);
            let legacy_rsdp = physical == rsdp && soc.legacy_rsdp_window.contains(physical, length);
            let firmware_owned = mmap_entries.iter().any(|entry| {
                let Some(entry_end) = entry.base.checked_add(entry.length) else {
                    return false;
                };
                matches!(
                    entry.ty,
                    crate::boot::MemoryType::Reserved
                        | crate::boot::MemoryType::AcpiReclaimable
                        | crate::boot::MemoryType::AcpiNvs
                ) && physical >= entry.base
                    && end <= entry_end
            });
            if !legacy_firmware && !legacy_rsdp && !firmware_owned {
                log::warn!(
                    "[acpi] rejected non-firmware range {:#x}..{:#x}",
                    physical,
                    end
                );
                return None;
            }
            let mut allocator = memory::frame::FRAME_ALLOCATOR.lock();
            let mut allocate = || {
                allocator
                    .as_mut()
                    .and_then(|frames| frames.allocate_frame())
            };
            // SAFETY: the published allocator owns returned frames and the live
            // Limine page tables are reachable through the configured HHDM.
            let mapped = unsafe {
                crate::hal::paging::map_hhdm_firmware_range(physical, length, &mut allocate)
            };
            if mapped {
                Some(crate::memory::frame::phys_to_virt(physical))
            } else {
                None
            }
        };

        let info = crate::acpi::parse(rsdp, &mut map_physical);
        log::info!(
            "[acpi] gates: madt={} hpet={} mcfg={}",
            info.lapic_base != 0 && info.ioapic_base != 0,
            info.hpet_base != 0,
            info.ecam_base != 0
        );
        info
    };

    #[cfg(target_arch = "x86_64")]
    let x86_ioapic_base = if crate::board::selected().has_driver(cellos_boards::DriverId::IoApic) {
        acpi_info.ioapic_base
    } else {
        0
    };
    #[cfg(target_arch = "x86_64")]
    let x86_lapic_base = if x86_ioapic_base != 0 {
        acpi_info.lapic_base
    } else {
        0
    };
    #[cfg(target_arch = "x86_64")]
    let x86_hpet_base = if crate::board::selected().has_driver(cellos_boards::DriverId::Hpet) {
        acpi_info.hpet_base
    } else {
        0
    };
    #[cfg(target_arch = "x86_64")]
    let x86_ecam_base = if crate::board::selected().has_driver(cellos_boards::DriverId::PcieEcam) {
        acpi_info.ecam_base
    } else {
        0
    };
    #[cfg(target_arch = "x86_64")]
    let x86_ecam_len = if x86_ecam_base == 0 {
        0
    } else {
        crate::task::drivers::pcie_ecam::ecam_window_size(
            acpi_info.ecam_bus_start,
            acpi_info.ecam_bus_end,
        )
        .expect("validated ACPI MCFG range has a bounded ECAM window")
    };
    #[cfg(target_arch = "x86_64")]
    let x86_timer_ready = x86_lapic_base != 0 && x86_ioapic_base != 0 && x86_hpet_base != 0;

    // 3. Paging (Virtual Memory) Setup
    // x86_64 bring-up: Limine's PML4 already maps RAM via HHDM and the kernel
    // at 0xFFFFFFFF80000000. We skip building + activating our own page tables
    // until the full x86_64 port (Phase 09). init_kernel_paging uses physical
    // addresses as virtual pointers, which would fault under Limine's paging.
    #[cfg(not(any(
        target_arch = "riscv32",
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "arm",
    )))]
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
        #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
        TEST_SAFE_ROOT.store(root_table_phys, core::sync::atomic::Ordering::Release);
        unsafe {
            core::ptr::write(
                &mut *memory::frame::FRAME_ALLOCATOR.lock(),
                Some(frame_allocator),
            );
        }
        log_info("Paging activated");
        log_info("Frame allocator initialized");
        // Set sstatus.SUM=1 so S-mode (kernel) can access USER-mapped pages throughout
        // the kernel lifetime. VirtIO/peripheral MMIO is mapped USER=1 for Driver Cells
        // (U-mode). Without SUM=1 the kernel's tech-debt VirtIO drivers fault at early-boot
        // MMIO init. In Cellos's SAS+LBI model security comes from Rust type safety, not
        // hardware USER-bit separation for kernel-vs-cell — SUM=1 is safe and intentional.
        #[cfg(target_arch = "riscv64")]
        // SAFETY: csrs modifies sstatus.SUM (bit 18). Safe to set for kernel S-mode code.
        unsafe {
            core::arch::asm!("csrs sstatus, {sum}", sum = in(reg) 0x40000_usize, options(nostack));
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        // Set runtime ECAM base from validated ACPI before PCIe scan. Zero
        // keeps PCIe closed; there is no implicit q35 fallback.
        crate::task::drivers::pcie_ecam::set_ecam_base_x86(x86_ecam_base as usize);
        crate::task::drivers::pcie_ecam::set_ecam_bus_range_x86(
            acpi_info.ecam_bus_start,
            acpi_info.ecam_bus_end,
        );

        log_info("Initializing x86_64 paging (kernel PML4)...");
        let root_table_phys = {
            let mut locked_frame_allocator = memory::frame::FRAME_ALLOCATOR.lock();
            memory::paging::init_kernel_paging_x86(
                locked_frame_allocator
                    .as_mut()
                    .expect("Frame allocator not initialized"),
                x86_ioapic_base,
                x86_hpet_base,
                x86_lapic_base,
                x86_ecam_base,
                x86_ecam_len,
            )
            .expect("Failed to initialize x86_64 kernel PML4")
        };
        log_info("x86_64 paging initialized");
        log_info("Activating x86_64 paging (mov cr3)...");
        // SAFETY: init_kernel_paging_x86 copied higher-half entries from Limine's PML4
        // (preserving kernel text/data/HHDM) and identity-mapped MMIO, so the kernel
        // continues executing after this CR3 switch without a triple-fault.
        unsafe {
            memory::paging::activate_paging(root_table_phys);
        }
        // Immediate configured port-I/O probe after activate_paging. If 'Q'
        // appears, the CR3 switch returned to kmain with the UART mechanism live.
        crate::hal::uart_16550::putchar(b'Q');
        if x86_timer_ready {
            crate::hal::apic::set_lapic_phys(x86_lapic_base);
            crate::hal::apic::set_ioapic_phys(x86_ioapic_base);
            crate::hal::apic::set_irq_overrides(
                &acpi_info.irq_overrides,
                acpi_info.ioapic_gsi_base,
            );
            crate::hal::set_hpet_base(x86_hpet_base as usize);
            crate::hal::init_timers();
            log_info("x86_64 timers initialized from ACPI (HPET + LAPIC)");
        } else {
            log::error!("[x86-gate] timer CLOSED: validated MADT + HPET required");
        }

        // Tier 3b x86 VMM P01: enter root-of-virtualization on the BSP.
        // SVM first (TCG-testable); VMX only on genuine Intel (KVM/HW lane).
        // Failure is non-fatal — the kernel runs fine without virt; the
        // HypervisorCap gate simply stays closed (has_x86_virt() == false).
        match cpu_features::x86_virt_kind() {
            Some(cpu_features::X86Virt::Svm) => {
                let hsave_pa = {
                    let mut guard = memory::frame::FRAME_ALLOCATOR.lock();
                    guard
                        .as_mut()
                        .expect("Frame allocator not initialized")
                        .allocate_frame()
                };
                match hsave_pa {
                    // SAFETY: hsave frame is freshly allocated, exclusively
                    // owned, never freed (root operation lasts until reset),
                    // and never mapped into any guest NPT (P02 invariant).
                    Some(pa) => match unsafe { hal::svm::enable(pa as u64) } {
                        Ok(()) => {
                            cpu_features::latch_x86_root_active();
                            log_info("x86 virt: root operation active (SVM)");
                        }
                        Err(_) => log_info("x86 virt: SVM present but firmware-locked; cap closed"),
                    },
                    None => log_info("x86 virt: OOM allocating HSAVE frame; cap closed"),
                }
            }
            Some(cpu_features::X86Virt::Vmx) => {
                let vmxon_pa = {
                    let mut guard = memory::frame::FRAME_ALLOCATOR.lock();
                    guard
                        .as_mut()
                        .expect("Frame allocator not initialized")
                        .allocate_frame()
                };
                match vmxon_pa {
                    Some(pa) => {
                        let va = memory::frame::phys_to_virt(pa) as *mut u32;
                        // SAFETY: vmxon frame freshly allocated + exclusively
                        // owned + alive forever; va is its HHDM mapping.
                        match unsafe { hal::vmx::enter_root(pa as u64, va) } {
                            Ok(()) => {
                                cpu_features::latch_x86_root_active();
                                log_info("x86 virt: root operation active (VMX)");
                            }
                            Err(_) => log_info("x86 virt: VMX present but unavailable; cap closed"),
                        }
                    }
                    None => log_info("x86 virt: OOM allocating VMXON frame; cap closed"),
                }
            }
            None => log_info("x86 virt: not supported by CPU; cap closed"),
        }
    }
    // Bare physical: RV32 Nano (SATP=0), x86_32 (CR0.PG=0), AArch32 (MMU off).
    #[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
    {
        memory::paging::init_bare();
        log_info("Paging: bare physical");
    }

    // 4. Heap Allocator (Global) - MUST be after paging but before any allocations
    // 4 MiB = 1024 frames (reduced from 32 MiB / 8192 frames under Phase 01 memory footprint reduction).
    // Sized to hold kernel structures, scheduler state, and IPC buffers.
    const HEAP_FRAMES: usize = 1_024;
    let heap_start = {
        let mut allocator_guard = memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = allocator_guard
            .as_mut()
            .expect("Frame allocator not initialized");
        let start = allocator.allocate_frame().expect("OOM: Heap start");
        for _ in 1..HEAP_FRAMES {
            allocator.allocate_frame().expect("OOM: Heap continuation");
        }
        start
    };
    let heap_size = HEAP_FRAMES * 4096;
    // On x86_64, phys_to_virt adds HHDM offset (Limine maps RAM at HHDM+phys).
    // On RISC-V, phys_to_virt returns phys unchanged (identity-mapped before paging).
    let heap_virt = memory::frame::phys_to_virt(heap_start);
    unsafe {
        memory::heap::init_heap(heap_virt, heap_size);
    }
    log_info("Heap initialized");

    memory::rt_heap::init();
    log_info("RT heap initialized");

    // 5. Hardware Abstraction Layer (HAL) Initialization
    // GDT/IDT/SYSCALL already done at step 1. Initialize PLIC for RISC-V external IRQs.
    #[cfg(target_arch = "riscv64")]
    if crate::board::active().has_driver(cellos_boards::DriverId::PlicSifive) {
        if let Some((context, irqs, irq_count)) = crate::platform::riscv_plic_init_data() {
            crate::hal::common::plic::init(context, &irqs[..irq_count]);
        } else {
            log::warn!("[plic] no active RV64 context mapping; external IRQs stay disabled");
        }
    } else {
        log::info!("[plic] disabled by active board descriptor");
    }
    log_info("HAL initialized (PLIC enabled)");

    // 6. Logger & Drivers & FS
    task::drivers::uart::init(); // registers log backend on all arches
    #[cfg(all(
        target_arch = "aarch64",
        not(feature = "board-rpi3"),
        not(feature = "board-rpi4")
    ))]
    {
        let (start, end) = boot::fallback_dtb_ram_range();
        log::info!("[boot] DTB RAM range {:#x}..{:#x}", start, end);
    }
    #[cfg(target_arch = "riscv64")]
    task::drivers::uart::init_input();
    #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
    {
        // IRQ delivery must start only after the heap-backed queue exists.
        task::drivers::uart::init_input();
        crate::hal::uart_bcm_mini::enable_rx_interrupt();
    }
    // RV32 Nano / x86_64 bring-up: skip VirtIO MMIO probing (PCIe transport not yet ported).
    // x86_64 gets VirtIO via the PCI path in virtio_pci::init() below.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    task::drivers::init();
    // x86_64: load embedded kernel_fs.img into RAM so EarlyLoader can serve ELFs from it.
    // VirtIO devices on q35 are on PCIe — probed via virtio_pci::init() after ECAM scan.
    #[cfg(target_arch = "x86_64")]
    {
        task::drivers::ramdisk::init_driver();
        // COM1 TX remains available without ACPI. RX IRQ requires the validated
        // MADT/HPET gate because it is routed through IOAPIC/LAPIC.
        if x86_timer_ready {
            crate::hal::uart_16550::init_input_irq();
        } else {
            log::warn!("[x86-gate] configured UART IRQ CLOSED: interrupt/timer unavailable");
        }
        // Initialise the RX buffer that vi_handle_uart_irq() writes into.
        task::drivers::uart::init_input();
        log_info("x86_64: ramdisk + UART RX IRQ initialised");
    }

    // PCIe ECAM scan + NVMe + e1000 + VirtIO PCI init.
    // ARM64 virt uses VirtIO MMIO (not PCIe); accessing 0x3F000000 on QEMU
    // virt 7+ triggers a Synchronous External Abort — skip on aarch64.
    // x86_64: the complete validated ACPI MCFG window is identity-mapped;
    // the early kernel fallback scans its first admitted bus before Platform.
    #[cfg(any(target_arch = "riscv64", target_arch = "x86_64"))]
    {
        #[cfg(target_arch = "x86_64")]
        if x86_ecam_base != 0 {
            // Early first-admitted-bus fallback: seed kernel PCI identity and
            // activate VT-d before any DMA-capable Driver Cell can run. The
            // Platform Cell receives the full validated MCFG range via argv
            // later and performs authoritative multi-bus enumeration.
            task::drivers::pcie_ecam::init();
            task::drivers::iommu::init();
            task::drivers::iommu::activate_isolation();
        } else {
            log::error!("[x86-gate] PCIe CLOSED: validated MCFG required");
        }
        #[cfg(target_arch = "riscv64")]
        task::drivers::iommu::set_deferred_init_pending();
        // VirtIO PCI block init removed (G2 loader redesign phase 06). x86 block I/O
        // is served by the NVMe Driver Cell (F4); the kernel drives no block hardware.
        // activate_isolation() is now called inside iommu::try_deferred_init() once the
        // IOMMU device has been registered by the Platform Cell. The call below is a no-op
        // (IOMMU not yet initialized at this point in boot).
        #[cfg(target_arch = "riscv64")]
        task::drivers::iommu::activate_isolation();
    }

    // Attempt warm boot from snapshot before any cell initialization.
    // RV32 Nano / x86_64 skip: no VirtIO block in bring-up.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    if snapshot::try_restore() {
        // try_restore() called yield_cpu() and should not return in a successful
        // warm boot.  If we reach here, fall through to cold boot as a safety net.
        log::warn!("[boot] snapshot restore returned unexpectedly → cold boot");
    }

    // Cross-check the on-disk MBR against the compiled-in partition layout
    // (warn-only — surfaces image/kernel drift at boot instead of as silent
    // corruption later).
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    crate::loader::disk_layout::verify_mbr();

    // Probe the cell bootstrap table so SpawnFromPath works during init.
    // RV32 Nano / x86_64 bring-up: no disk.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    match crate::loader::early::EarlyLoader::probe() {
        Ok(()) => puts("[loader] cell bootstrap table loaded\n"),
        Err(_) => {
            puts("[loader] WARN: cell table not found — disk image may lack bootstrap section\n")
        }
    }

    // RV32 Nano: no FAT32 FS in bring-up.
    // x86_64 uses the ramdisk-backed embedded FS to serve cell ELFs via VIFS1.
    #[cfg(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    ))]
    fs::init();

    // Load + verify the signed operator policy (P5b) NOW: after VIFS1 is mounted,
    // before any cap-bearing cell spawns. Absent → dev-permissive (this G1 build);
    // invalid → fail-closed. Phase 04 folds policy::lookup into the spawn grant.
    #[cfg(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    ))]
    policy::load_from_vifs1();

    // Phase 20: hot-migration state-transfer self-test.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    crate::cell::state_stash::self_test();

    log_info("Kernel subsystems initialized successfully.");

    // 7. Initialize Scheduler
    log_info("Initializing scheduler...");
    task::init();
    log_info("Scheduler initialized");
    #[cfg(feature = "test-hooks")]
    if task::scheduler::death_subscription_quota_self_test() {
        log_info("death-subscriber quota self-test PASS");
    } else {
        log_info("death-subscriber quota self-test FAIL");
    }
    // This test-only branch must precede every unrelated boot suite: its
    // terminal is the sole evidence for opcode 214's SAS ownership path.
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "getrandom-sas-test"
    ))]
    {
        task::smp::start_secondaries();
        // Erase the diverging return type so the generic boot path remains
        // typechecked without a feature-specific unreachable-code warning.
        let exit = |success| -> () { crate::qemu_exit(success) };
        exit(task::user_copy_tests::run_getrandom_primary());
    }

    #[cfg(feature = "test-hooks")]
    if crate::resource_registry::pcie_bar_window_self_test() {
        log_info("[selftest] PCIE-BAR-WINDOW: PASS (bounded, aligned, overflow-safe)");
    } else {
        log_info("[selftest] PCIE-BAR-WINDOW: FAIL");
    }

    // The loader corpus exercises real spawn_gated denials and snapshots the
    // scheduler around every malformed image. Run it after task::init(), while
    // the scheduler is available but still empty and before secondary harts or
    // unrelated boot tasks can add noise. Assertions make any failure boot-fatal.
    #[cfg(feature = "test-hooks")]
    crate::loader::elf_tests::run_all();
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if task::path_selftest::self_test() {
        log_info("cwd-path self-test PASS");
    } else {
        log_info("cwd-path self-test FAIL");
    }
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if task::fstat_selftest::self_test() {
        log_info("fstat self-test PASS");
    } else {
        log_info("fstat self-test FAIL");
    }

    // 7a. Trust-model self-tests (thread identity inheritance + honest revoke).
    // Runs HERE — after the scheduler exists but BEFORE secondaries start — so the
    // synthetic thread it spawns cannot be raced onto another hart before teardown.
    #[cfg(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    ))]
    {
        if task::thread_cap_selftest::self_test() {
            log_info("thread-cap self-test PASS (thread-inherit + honest-revoke + spawn bound)");
        } else {
            log_info("thread-cap self-test FAIL");
        }
        if task::thread_quota_selftest::self_test() {
            log_info("thread-quota self-test PASS (charged, released, enforced)");
        } else {
            log_info("thread-quota self-test FAIL");
        }
        match task::thread_user_entry_selftest::self_test() {
            Some(true) => log_info("thread-user-entry self-test PASS (U-mode entry+arg+exit)"),
            Some(false) => log_info("thread-user-entry self-test FAIL"),
            None => log_info("thread-user-entry self-test SKIP (RV64 runtime gate only)"),
        }
        if task::completion_selftest::self_test() {
            log_info("completion-queue self-test PASS (reserve, land, bound, defer)");
        } else {
            log_info("completion-queue self-test FAIL");
        }
        if task::net_rx_selftest::self_test() {
            log_info("net-rx-reservation self-test PASS (fill, remember, release)");
        } else {
            log_info("net-rx-reservation self-test FAIL");
        }
        if task::ipc_pending_selftest::self_test() {
            log_info("ipc-pending self-test PASS (deferred delivery, bounds, quota)");
        } else {
            log_info("ipc-pending self-test FAIL");
        }
        task::cap_file_selftest::run();
        log_info("cap-file self-test PASS (read-only enforcement)");
        if task::ipc_guardrail_selftest::self_test() {
            log_info("ipc-guardrail self-test PASS (dead-peer wake, RecvScatter isolated)");
        } else {
            log_info("ipc-guardrail self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::vfs_lifecycle_selftest::self_test() {
            log_info("vfs-lifetime self-test PASS (exact lease, quarantine, owner watch)");
        } else {
            log_info("vfs-lifetime self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::stack::stack_probe_self_test() {
            log_info(
                "stack-probe self-test PASS (two guards, overflow target unmapped, watermark)",
            );
        } else {
            log_info("stack-probe self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::stack_sizing_policy_self_test() {
            log_info("stack-sizing policy self-test PASS (measured=16, unknown=64)");
        } else {
            log_info("stack-sizing policy self-test FAIL");
        }
    }

    // 7b. Bring secondary harts online (riscv64 only; no-op on other arches).
    // Must run AFTER task::init() so the heap and scheduler are live before
    // any secondary hart starts running kernel code.
    #[cfg(target_arch = "riscv64")]
    task::smp::start_secondaries();

    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    crate::memory::tlb_shootdown_selftest::run_primary();
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    crate::memory::address_space::address_space_tests::run_primary();

    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    crate::loader::atomic_publication_tests::run_governed_success_after_secondaries();
    #[cfg(all(target_arch = "riscv64", feature = "native-domains"))]
    memory::domain_supervisor_registry::activate();
    #[cfg(all(target_arch = "riscv64", feature = "native-domains"))]
    memory::domain_supervisor_registry::register_static_image()
        .expect("kernel static ranges must be disjoint");
    #[cfg(all(target_arch = "riscv64", feature = "native-domains"))]
    memory::domain_supervisor_registry::register(
        heap_virt,
        heap_virt
            .checked_add(heap_size)
            .expect("kernel heap range overflow"),
        memory::domain_supervisor_registry::SupervisorRangeKind::KernelHeap,
        memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
    )
    .expect("kernel heap registration must be unique");
    #[cfg(all(target_arch = "riscv64", feature = "native-domains"))]
    if let Some(guard) = memory::frame::FRAME_ALLOCATOR.lock().as_ref() {
        let (bm_start, bm_end) = guard.bitmap_range();
        memory::domain_supervisor_registry::register(
            bm_start,
            bm_end,
            memory::domain_supervisor_registry::SupervisorRangeKind::StaticWritable,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        )
        .expect("frame allocator bitmap registration must succeed");
    }
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    if !task::domain_switch_tests::run_primary() {
        log::error!("native-domain SAS scheduler fixture failed");
    }
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    task::context_handoff_selftest::run_primary();
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    task::retirement_selftest::run_primary();
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    {
        task::user_copy_tests::run_primary();
        task::ipc_wire_selftest::run_primary(task::smp::online_hart_count());
        crate::loader::domain_admission::run_selftest();
        task::domain_grant::run_selftest();
    }

    // 8. Spawn Embedded Init
    // RV32 Nano bring-up: no init binary — boot to idle loop.
    #[cfg(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    ))]
    {
        log_info("Spawning Embedded Init...");

        // Power-on self-test of the Ed25519 verify primitive (RFC 8032 TEST 1 +
        // tamper-negative) before it is trusted to authenticate the signed
        // operator policy (P5). Cheap (~one verify); a FAIL means the crypto path
        // is broken and signed policy must not be trusted.
        if crate::ed25519::self_test() {
            log_info("ed25519 verify self-test PASS (RFC 8032 + tamper)");
        } else {
            log_info("ed25519 verify self-test FAIL — signed policy unsafe");
        }
        // Power-on self-test of the signed-policy path: verify + parse a known
        // dev-signed blob, and confirm a tampered blob is rejected.
        if crate::policy::self_test() {
            log_info("policy verify+parse self-test PASS (signed blob + tamper)");
        } else {
            log_info("policy verify+parse self-test FAIL");
        }
        // Cell binary signing self-test: known-good vector verifies + flipped
        // payload is rejected. Runs before any spawn_from_path is called.
        if crate::signing::self_test() {
            log_info("cell signing self-test PASS");
        } else {
            log_info("cell signing self-test FAIL — cell signature gate unsafe");
        }
        #[cfg(feature = "test-hooks")]
        if crate::admission::self_test() {
            log_info("admission-core self-test PASS (fail-closed A/B floor model)");
        } else {
            log_info("admission-core self-test FAIL");
        }
        // P-TRUST: privileged path-caps are bounded by the spawn ceiling. Pure
        // CapSet logic (no scheduler), so it runs here alongside the crypto tests,
        // before any cap-bearing cell spawns.
        if crate::task::p_trust_selftest::self_test() {
            log_info("P-TRUST self-test PASS (privileged path-caps ceiling-bounded)");
        } else {
            log_info("P-TRUST self-test FAIL");
        }
        // Boot ceiling: the per-path table is per-path (not a union) and no boot
        // cell is over-tightened out of the cap it needs. Runs BEFORE the first
        // Root spawn below, so a bad row is reported before it breaks a cell.
        if crate::loader::boot_ceiling::self_test() {
            log_info("boot-ceiling self-test PASS (per-path table, no union collapse)");
        } else {
            log_info("boot-ceiling self-test FAIL — a boot cell may lose caps");
        }
        // MMIO allowlist: the production table must still authorize its known
        // windows and must keep denying the DWC2 USB window (USB host authority
        // awaits policy v3 with a signed byte). Runs on the REAL allowlist.
        if crate::resource_registry::self_test() {
            log_info("mmio-allowlist self-test PASS (known windows live, DWC2 denied)");
        } else {
            log_info("mmio-allowlist self-test FAIL — DWC2 window or allowlist shape changed");
        }
        // Manifest v2: v1-upcast/v2-parse + the tier-floor invariant. Pure logic,
        // no scheduler — runs alongside the other crypto/trust self-tests.
        if crate::task::manifest_v2_selftest::self_test() {
            log_info("Manifest-v2 self-test PASS (v1 upcast + v2 parse + tier-floor)");
        } else {
            log_info("Manifest-v2 self-test FAIL");
        }

        // Layer-2 hardware security self-tests (test-hooks feature only).
        // MTE (aarch64) and PKU (x86_64) — each prints PASS or SKIP.
        // Runs here: after all HW init, before scheduler + first cell spawn.
        #[cfg(feature = "test-hooks")]
        {
            #[cfg(target_arch = "aarch64")]
            crate::layer2_selftest::run_mte_selftest();
            #[cfg(target_arch = "x86_64")]
            crate::layer2_selftest::run_pku_selftest();
            // Tier 3b x86 VMM P03 M1: SVM world-switch smoke (port-out 'K' + HLT).
            // Only meaningful when SVM root operation was entered at boot.
            #[cfg(target_arch = "x86_64")]
            if cpu_features::has_x86_virt() {
                crate::hypervisor::svm_registry::x86_smoke();
            } else {
                log_info("X86-VMM-SMOKE: SKIP (no SVM/VMX root)");
            }
        }
        #[cfg(all(target_arch = "x86_64", feature = "x86-mmio-smoke"))]
        if cpu_features::has_x86_virt() {
            crate::hypervisor::svm_registry::x86_mmio_smoke();
        } else {
            log_info("X86-MMIO-SMOKE: SKIP (no SVM/VMX root)");
        }

        // Spawn the Platform Cell before init (PCIe ECAM scanner).
        // RISC-V uses the compile-time GPEX window (0x3000_0000).
        // x86_64 hands the validated runtime ACPI-MCFG base and bus range via argv.
        // Failure is non-fatal: kernel-side PCI_DEVICES stays empty; Driver Cells
        // that rely on sys_find_pcie_device will simply not find their device.
        #[cfg(target_arch = "riscv64")]
        match crate::loader::spawn_from_path(
            "/bin/platform",
            crate::loader::SpawnRequest::governed_boot(),
        ) {
            Ok(_) => log_info("Platform Cell spawned (PCIe ECAM scanner)"),
            Err(_) => log_info("Platform Cell absent — PCIe BARs will not be pre-registered"),
        }

        #[cfg(target_arch = "x86_64")]
        if x86_ecam_base != 0 {
            let argv_str = alloc::format!(
                "--ecam-base={:#x} --bus-start={} --bus-end={}",
                x86_ecam_base,
                acpi_info.ecam_bus_start,
                acpi_info.ecam_bus_end,
            );
            match crate::loader::spawn_from_path(
                "/bin/platform",
                crate::loader::SpawnRequest::governed_boot().with_argv(argv_str.into_bytes()),
            ) {
                Ok(_) => log_info("Platform Cell spawned (x86 PCIe ECAM scanner)"),
                Err(_) => log_info("Platform Cell absent — PCIe BARs will not be pre-registered"),
            }
        }

        // `aligned_elf::bytes` borrows an already aligned embedded image and only
        // materializes an aligned copy when the linker did not provide one. Do not
        // unconditionally duplicate init on the kernel heap at this boot boundary.
        #[cfg(feature = "test-hooks")]
        {
            log_info("ATOMIC_PUBLICATION_AP-15: arming trusted init");
            crate::loader::atomic_publication_tests::arm_trusted_success();
            log_info("ATOMIC_PUBLICATION_AP-15: armed for trusted init");
        }
        log_info("Publishing trusted init");
        match crate::loader::spawn_trusted_init(INIT_ELF) {
            Ok(tid) => {
                #[cfg(feature = "test-hooks")]
                crate::loader::atomic_publication_tests::finish_trusted_success(tid);
                log::info!(
                    "Successfully spawned init with complete root authority (tid={})",
                    tid
                );
                #[cfg(feature = "board-rpi3")]
                crate::hal::uart_bcm_mini::probe_put(b'V');
            }
            Err(_e) => {
                log_info("Failed to spawn init");
                // Failure occurs before init reaches any ready queue.
                #[cfg(feature = "board-rpi3")]
                crate::hal::uart_bcm_mini::probe_put(b'F');
                #[cfg(feature = "test-hooks")]
                panic!("atomic-publication trusted-init success contract did not publish init");
            }
        }
    }

    // Ring-3 smoke test: spawn a minimal U-mode task that logs and exits.
    // RISC-V only — task writes RISC-V machine code directly.
    // Expected serial output: "Hi from U-mode!\n" followed by task exit.
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    match task::user_hello::spawn() {
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
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    if task::stack_overflow_probe::spawn().is_err() {
        log_info("[stack-guard] deliberate overflow probe spawn FAIL");
    }

    log_info("Kernel initialization complete. Entering idle loop.");

    // 9. Start multitasking
    log_info("Starting scheduler...");

    // Quiet the shared console for interactive use. Kernel bring-up is done; the
    // remaining Info chatter is per-spawn noise ([loader] SpawnFromPath, Spawn:,
    // ELF LOAD) that floods the UART and buries the shell prompt. WARN/ERROR still
    // surface real problems. Raise back to Info when debugging the spawn path.

    // Enable interrupts before entering the idle loop.
    // RISC-V: set SPP=1 and SIE=1 in sstatus (0x102).
    // AArch64: clear DAIF.I bit to unmask IRQs.
    // x86_64: STI via ARCH.enable_interrupts().
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    unsafe {
        // SAFETY: csrs sstatus SPP|SIE from S-mode — standard interrupt enable.
        core::arch::asm!("csrs sstatus, {0}", in(reg) 0x102usize);
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("msr daifclr, #2", options(nomem, nostack));
    }
    // x86_64, x86_32, AArch32: use the Arch trait's enable_interrupts().
    #[cfg(any(target_arch = "x86_64", target_arch = "x86", target_arch = "arm"))]
    crate::hal::ARCH.enable_interrupts();

    log::set_max_level(log::LevelFilter::Warn);

    // Probe 'Q': fires ONLY if no IRQ preempted the code between daifclr and here.
    // Q fires  → BCM2835 IRQ is not actually delivered to CPU on QEMU (pending but silent).
    // Q absent → IRQ fired immediately after daifclr (expected); check if H/G appear.
    #[cfg(feature = "board-rpi3")]
    crate::hal::uart_bcm_mini::probe_put(b'Q');

    // board-rpi3: Print IRQ state snapshot immediately after enabling IRQs.
    // Format: "K<src_bit8><pend_bit1><src_nibble>" where src = CORE0_IRQ_SOURCE,
    // pend = IRQ_PENDING1 bit 1 (BCM2835 systimer C1 pending).
    // If G never fires but K shows pend=1 and src=0: QEMU does not route BCM2835→BCM2836.
    #[cfg(feature = "board-rpi3")]
    {
        let soc = hal_soc_bcm27xx::BCM2837;
        // SAFETY: both BCM controller apertures are identity-mapped before IRQs are enabled.
        let src_raw = unsafe {
            core::ptr::read_volatile((soc.mmio.local_controller_base + 0x60) as *const u32)
        };
        let pend =
            unsafe { core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x04) as *const u32) };
        let hex = |n: u32| -> u8 {
            if n < 10 {
                b'0' + n as u8
            } else {
                b'a' + n as u8 - 10
            }
        };
        crate::hal::uart_bcm_mini::probe_put(b'K');
        crate::hal::uart_bcm_mini::probe_put(if src_raw & soc.irq.local_gpu_mask != 0 {
            b'1'
        } else {
            b'0'
        });
        crate::hal::uart_bcm_mini::probe_put(if pend & (1 << soc.irq.system_timer_c1) != 0 {
            b'1'
        } else {
            b'0'
        });
        crate::hal::uart_bcm_mini::probe_put(hex(src_raw & 0xF));
    }

    // Probe 'L': fires once per idle loop iteration (only first 3 on board-rpi3 to avoid flood).
    // If 'L' never appears after 'K', the code never reaches the idle loop.
    #[cfg(feature = "board-rpi3")]
    {
        static IDLE_COUNT: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0);
        loop {
            let n = IDLE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if n < 4 {
                crate::hal::uart_bcm_mini::probe_put(b'L');
            }
            crate::task::yield_cpu();
            crate::hal::ARCH.wait_for_interrupt();
        }
    }
    #[cfg(not(feature = "board-rpi3"))]
    loop {
        crate::task::yield_cpu();
        crate::hal::ARCH.wait_for_interrupt();
    }
}

/// Emit an allocation-free RV64 fault snapshot before the panic path.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
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

/// Non-recoverable kernel panic handler.
///
/// `current_cell_id` is allocation attribution, not an execution-mode proof:
/// kernel code can hold scheduler or other kernel locks while servicing a
/// Cell. Therefore this handler never schedules or retires a Cell. Only the
/// architecture trap path, after proving an interrupted U-mode context, may
/// enter the deferred retirement funnel.
#[cfg(not(all(test, not(target_os = "none"))))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // True kernel panic: print diagnostics and halt.
    #[inline(always)]
    fn panic_putchar(c: u8) {
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        {
            let _ = crate::hal::sbi::console_putchar(c);
        }
        #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
        {
            crate::hal::uart_bcm_mini::putchar(c);
        }
        #[cfg(all(target_arch = "aarch64", not(feature = "board-rpi3")))]
        {
            crate::hal::uart_pl011::putchar(c);
        }
        #[cfg(target_arch = "arm")]
        {
            crate::hal::uart_pl011::putchar(c);
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        {
            crate::hal::uart_16550::putchar(c);
        }
    }
    let puts = |s: &str| {
        for c in s.bytes() {
            panic_putchar(c);
        }
    };
    puts("\n[KERNEL PANIC] ");
    puts("Critical failure.\n");
    use core::fmt::Write;
    struct PanicWriter;
    impl core::fmt::Write for PanicWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for c in s.bytes() {
                panic_putchar(c);
            }
            Ok(())
        }
    }
    let _ = writeln!(PanicWriter, "{}", info);

    // Reboot or spin: RISC-V uses SBI SRST; ARM64 / x86_64 spin.
    puts("[KERNEL PANIC] halting...\n");
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    crate::hal::sbi::system_reset(crate::hal::sbi::SBI_RESET_COLD_REBOOT, 0);
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
    // Fallback halt for all non-x86 arches (including riscv — unreachable after system_reset).
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    loop {
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
    }
}
