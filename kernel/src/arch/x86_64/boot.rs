// SPDX-License-Identifier: MPL-2.0
//! `boot`: the x86_64 phases of `kmain` and its hooks in the neutral tail.
//!
//! [`early`] runs up to and including paging, the timers and root operation;
//! [`platform_init`] runs after the heap, up to and including VT-d. The
//! remaining functions are called at fixed points of the neutral tail and keep
//! the place the x86_64 code had there.

pub mod cpu_features;
pub mod firmware;
pub mod limine;

use crate::boot::{
    BootInfoSource, BootStorage, ClockReady, ConsoleReady, IommuReady, IrqReady, PciReady,
    PlatformReady,
};
use crate::log_info;
use crate::memory;

/// Write one byte to the early console (the configured 16550, polled); the
/// panic handler writes through it too.
#[inline(always)]
pub(crate) fn putc(c: u8) {
    crate::hal::uart_16550::putchar(c);
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
    acpi_info: crate::acpi::AcpiInfo,
    ecam_base: u64,
    timer_ready: bool,
}

/// Console, HHDM, descriptor tables, boot memory, ACPI and profile binding,
/// kernel PML4, LAPIC/IOAPIC, timers and VMX/SVM root operation.
pub(crate) fn early(_hartid: usize, dtb: usize) -> EarlyPlatform {
    let dtb = crate::boot::effective_dtb(dtb);
    crate::cpu_features::detect(dtb);
    // 0. Initialize UART immediately for early logging
    {
        // No profile is bound before ACPI discovery; every catalog profile
        // shares these facts (const-asserted in hal_soc_x86), so the console
        // and COM wiring cannot depend on which profile is bound later.
        let early = &hal_soc_x86::X86_EARLY_FACTS;
        crate::hal::uart_16550::configure(early.com1.base, early.com1.irq);
        crate::hal::uart_16550::init();
        // Register every declared port: the console takes slot 0 and each extra
        // port must answer a 16550 probe before it is offered, so a declared
        // port this machine does not have produces a refusal rather than a
        // cell writing into empty port space.
        {
            use crate::hal::uart_16550::{SerialPortSpec, MAX_PORTS};
            let mut specs = [SerialPortSpec { base: 0, irq: 0 }; MAX_PORTS];
            let declared = early.serial_ports.len().min(MAX_PORTS);
            for (slot, port) in early.serial_ports.iter().take(declared).enumerate() {
                specs[slot] = SerialPortSpec {
                    base: port.base,
                    irq: port.irq,
                };
            }
            crate::hal::uart_16550::configure_all(&specs[..declared]);
        }
        // Emitted only after `init()`: `putchar` asserts a configured port. The
        // bound profile is named by `board::bind_x86_profile` after ACPI.
        crate::hal::uart_16550::puts(
            "[x86-gate] configured 16550 ready: TX + polled RX; IRQ pending MADT\n",
        );
        if crate::hal::uart_16550::poll_input().is_some() {
            crate::hal::uart_16550::puts("[x86-gate] COM1 polled RX observed\n");
        }
        // Register the log backend now, as RV64 does above: the console is
        // configured and the boot stub installed GS (Spinlock needs it), and
        // neither the logger nor its console lock allocates. Registering it at
        // step 6 dropped every `log` record of the boot path before the heap
        // (allocator ranges, firmware-range rejections, ACPI table findings).
        crate::dev::uart::init();
    }

    // Set HHDM base for LAPIC/IOAPIC MMIO access AND for phys_to_virt.
    // Limine maps RAM at HHDM_BASE+phys (no identity mapping of physical RAM).
    // This must be called before FrameAllocator::new_from_map.
    {
        let hhdm = crate::boot::limine::get_hhdm_offset().unwrap_or(0);
        crate::hal::apic::set_hhdm_base(hhdm);
        crate::memory::frame::set_phys_offset(hhdm as usize);
        // Propagate the HHDM offset to the HAL page-table walkers so they can
        // dereference physical PTE addresses via HHDM virtual pointers.
        crate::hal::paging::set_hhdm_offset(hhdm as usize);
        // Initialise KASLR seed from HHDM entropy + RDTSC.
        crate::memory::kaslr::init_kaslr(hhdm);
    }

    // 1. Initialize HAL (Architecture specific) - Early Trap Setup
    // x86_64: LAPIC is deferred until after paging sets up the MMIO mapping
    // (LAPIC phys 0xFEE00000 isn't in Limine's HHDM for MMIO regions).
    {
        // SSE/FXSR first: every IDT entry runs FXSAVE/LDMXCSR, so a fault
        // taken during gdt/idt init (including the IDT fixture's probes) would
        // #UD in the entry path and triple-fault without CR4.OSFXSR.
        crate::hal::init_sse();
        // Unmap the guard pages around every NMI/#DF/#MC IST stack, and the one
        // below the BSP boot stack, before the TSS and IST gates are loaded.
        // The live root is Limine's; the kernel PML4, AP roots and domain roots
        // later copy its higher-half entries, so they share these cleared PTEs.
        if let Err(error) = unsafe { crate::hal::x86_64::ist::install_guards() } {
            panic!("x86: IST/boot-stack guard pages unavailable: {error:?}");
        }
        crate::hal::gdt::init();
        crate::hal::idt::init();
        crate::hal::cet::init_kernel_cet(); // LAYER2-CET-INIT
        crate::hal::pku::init(); // LAYER2-PKU-INIT (requires IBT, checked inside)
        crate::hal::syscall::init();
        // Opt-in SMEP/SMAP/UMIP profile: a requested protection the CPU
        // lacks refuses boot rather than running unprotected.
        assert!(
            crate::hal::x86_64::protect::init_cpu(),
            "x86: requested SMEP/SMAP/UMIP profile unsupported by CPU"
        );
        crate::hal::x86_64::publish_boot_cpu_policy();
        // apic::init_lapic() deferred — needs MMIO mapped via custom PML4
    }

    crate::boot_banner();
    let (boot_info, frame_allocator) = crate::boot_memory(dtb);
    let mmap_entries = boot_info.get().memory_map();

    // x86_64 publishes the allocator immediately: Limine's paging already gives
    // its Spinlock usable memory attributes.
    unsafe {
        core::ptr::write(
            &mut *memory::frame::FRAME_ALLOCATOR.lock(),
            Some(frame_allocator),
        );
    }
    log_info("Frame allocator initialized");

    // Limine does not guarantee HHDM mappings for firmware-owned memory. The
    // ACPI and SMBIOS parsers request a mapping for each RSDP/SDT/SMBIOS range
    // before dereferencing it. Those ranges must stay inside ACPI
    // reclaimable/NVS or firmware-reserved memory-map entries. Legacy BIOS
    // records may live in the bounded windows of `X86_EARLY_FACTS`, which every
    // catalog profile shares. Some BIOSes, including q35, report other SDTs as
    // Reserved, so checksums remain the final trust gate.
    let (acpi_info, smbios_system) = {
        let rsdp = limine::get_rsdp_ptr().unwrap_or(0);
        let early = &hal_soc_x86::X86_EARLY_FACTS;
        let mut map_physical = |physical: usize, length: usize| {
            let end = physical.checked_add(length)?;
            if length == 0 {
                return None;
            }
            let legacy_firmware = early.legacy_bios_window.contains(physical, length);
            let legacy_rsdp =
                physical == rsdp && early.legacy_rsdp_window.contains(physical, length);
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
                    "[firmware] rejected non-firmware range {:#x}..{:#x}",
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
            "[acpi] gates: madt={} hpet={} mcfg={} dmar={} fadt={} spcr={}",
            info.lapic_base != 0 && info.ioapic_base != 0,
            info.hpet_base != 0,
            info.ecam_base != 0,
            info.dmar_base != 0,
            info.fadt.is_some(),
            info.spcr.is_some()
        );
        crate::acpi::with_topology(|topo| {
            log::info!(
                "[acpi] topology: cpus={} ioapics={} nmis={} mcfg={} drhd={} rmrr={} ir={} x2apic_opt_out={}",
                topo.cpus.len(),
                topo.ioapics.len(),
                topo.lapic_nmis.len(),
                topo.mcfg.len(),
                topo.drhds.len(),
                topo.rmrrs.len(),
                topo.dmar_intr_remap(),
                topo.dmar_x2apic_opt_out()
            )
        });
        // Power facts (`\_S5_`, reset register, GPE blocks) need the same
        // firmware-range mapper; the G1 private lane keeps its own contract.
        #[cfg(not(feature = "g1-cpu-accounting"))]
        crate::arch::acpi::sci::init(&info, rsdp, &mut map_physical);
        // SMBIOS entry points and tables get the same firmware-range admission
        // as ACPI tables: the legacy BIOS window or firmware-owned memory.
        let (smbios_32, smbios_64) = limine::get_smbios_entry_points();
        let smbios = crate::smbios::discover(smbios_32, smbios_64, &mut map_physical);
        (info, smbios)
    };
    let platform_identity = crate::board::PlatformIdentity {
        acpi_oem: acpi_info.root_oem,
        smbios: smbios_system,
        hypervisor: crate::arch::boot::cpu_features::hypervisor_present(),
    };
    crate::board::report_identity(&platform_identity);
    // Bind the profile once, before the first `has_driver` gate. The operator
    // names it on the Limine command line; detected facts may only reject it.
    // The facts are kept with the profile for the machine fingerprint (433).
    {
        let cmdline = limine::get_executable_cmdline(crate::board::CMDLINE_MAX + 1).unwrap_or(&[]);
        match crate::board::choose_x86_profile(cmdline, &platform_identity) {
            Ok((board, source)) => crate::board::bind_x86_profile(
                board,
                crate::board::X86BootFacts {
                    identity: platform_identity,
                    source,
                    dmar_present: acpi_info.dmar_present,
                },
            ),
            Err(error) => panic!("[board] {}", error),
        }
        let console_opt = crate::board::console_option(cmdline).unwrap_or(None);
        let _ = crate::arch::dev::console_select::select_and_apply_console(console_opt, &acpi_info);
    }

    let x86_ioapic_base = if crate::board::active().has_driver(cellos_boards::DriverId::IoApic) {
        acpi_info.ioapic_base
    } else {
        0
    };
    let x86_lapic_base = if x86_ioapic_base != 0 {
        acpi_info.lapic_base
    } else {
        0
    };
    let x86_hpet_base = if crate::board::active().has_driver(cellos_boards::DriverId::Hpet) {
        acpi_info.hpet_base
    } else {
        0
    };
    let x86_ecam_base = if crate::board::active().has_driver(cellos_boards::DriverId::PcieEcam) {
        acpi_info.ecam_base
    } else {
        0
    };
    let x86_ecam_len = if x86_ecam_base == 0 {
        0
    } else {
        crate::dev::pcie_ecam::ecam_window_size(acpi_info.ecam_bus_start, acpi_info.ecam_bus_end)
            .expect("validated ACPI MCFG range has a bounded ECAM window")
    };
    // The timer gate opens only after `init_timers` finds a clocksource
    // (phase 02 of plan 261103-2218); HPET is one candidate, not a requirement.
    let x86_interrupts_ready = x86_lapic_base != 0 && x86_ioapic_base != 0;
    #[cfg_attr(feature = "g1-cpu-accounting", allow(unused_assignments))]
    // G1 assigns or panics on every path
    let mut x86_timer_ready = false;

    // 3. Paging (Virtual Memory) Setup
    {
        // Set runtime ECAM base from validated ACPI before PCIe scan. Zero
        // keeps PCIe closed; there is no implicit q35 fallback.
        crate::dev::pcie_ecam::set_ecam_base_x86(x86_ecam_base as usize);
        crate::dev::pcie_ecam::set_ecam_bus_range_x86(
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
        // The G1 private lane keeps its HPET-only contract.
        #[cfg(feature = "g1-cpu-accounting")]
        let x86_timer_candidates = x86_interrupts_ready && x86_hpet_base != 0;
        #[cfg(not(feature = "g1-cpu-accounting"))]
        let x86_timer_candidates = x86_interrupts_ready;
        if x86_timer_candidates {
            crate::hal::apic::set_lapic_phys(x86_lapic_base);
            crate::hal::apic::set_irq_overrides(
                &acpi_info.irq_overrides,
                &acpi_info.irq_override_flags,
            );
            register_x86_ioapics();
            crate::hal::set_hpet_base(x86_hpet_base as usize);
            #[cfg(not(feature = "g1-cpu-accounting"))]
            {
                offer_x86_clock_candidates(&acpi_info);
                match crate::hal::init_timers() {
                    Some(setup) => {
                        x86_timer_ready = true;
                        log::info!(
                            "[x86-gate] timer: clock={} clockevent={:?} apic={}",
                            setup.source.name(),
                            setup.clockevent,
                            if setup.x2apic { "x2apic" } else { "xapic" }
                        );
                    }
                    None => log::error!(
                        "[x86-gate] timer CLOSED: no clocksource (TSC, kvmclock, HPET or PM timer)"
                    ),
                }
            }
            #[cfg(feature = "g1-cpu-accounting")]
            {
                let vendor = core::arch::x86_64::__cpuid(0);
                assert!(
                    vendor.ebx == u32::from_le_bytes(*b"Genu")
                        && vendor.edx == u32::from_le_bytes(*b"ineI")
                        && vendor.ecx == u32::from_le_bytes(*b"ntel"),
                    "G1 private lane requires GenuineIntel CPUID"
                );
                let calibration = crate::hal::init_timers_observed()
                    .expect("G1 requires actual HPET/LAPIC timer initialization");
                let clock = crate::hal::hpet::checked_sample()
                    .expect("G1 requires a checked shared 64-bit HPET");
                let initial = calibration
                    .ticks_per_ms
                    .checked_mul(10)
                    .and_then(|count| u32::try_from(count).ok())
                    .expect("G1 rejects overflowed LAPIC calibration");
                assert!(
                    calibration.hpet_used
                        && initial >= 10_000
                        && initial == calibration.initial_count
                        && calibration.lvt & (1 << 16) == 0
                        && (calibration.lvt >> 17) & 3 == 1,
                    "G1 rejects LAPIC fallback, stopped or masked timer"
                );
                // The G1 lane's boot receipt: emitted raw and allocation-free,
                // starting its own line, without a log-level tag.
                struct G1BootWriter;
                impl core::fmt::Write for G1BootWriter {
                    fn write_str(&mut self, text: &str) -> core::fmt::Result {
                        for byte in text.bytes() {
                            crate::hal::uart_16550::putchar(byte);
                        }
                        Ok(())
                    }
                }
                core::fmt::write(&mut G1BootWriter, format_args!(
                    "\nG1-CLOCK-BOOT raw={} period_fs={} base={} caps={} config={} ticks_per_ms={} initial={} lvt={}\n",
                    clock.raw_counter, clock.period_fs, clock.base, clock.capabilities,
                    clock.configuration, calibration.ticks_per_ms,
                    calibration.initial_count, calibration.lvt,
                )).expect("G1 boot receipt formatting");
                x86_timer_ready = true;
                log_info("x86_64 timers initialized from ACPI (HPET + LAPIC)");
            }
        } else {
            // Fail-closed decision: it must reach the console.
            log::error!("[x86-gate] timer CLOSED: validated MADT required");
            #[cfg(feature = "g1-cpu-accounting")]
            panic!("G1 private measurement requires validated MADT and HPET");
        }

        // Tier 3 x86 VMM P01: enter root-of-virtualization on the BSP.
        // SVM first (TCG-testable); VMX only on genuine Intel (KVM/HW lane).
        // Failure is non-fatal — the kernel runs fine without virt; the
        // HypervisorCap gate simply stays closed (has_x86_virt() == false).
        // The two backends differ in how a refusal surfaces, and that decides
        // what may be attempted under a hypervisor:
        //   * VMXON **faults** (#GP) when the host grants the bits but refuses
        //     nested operation, so VMX is never attempted under a hypervisor —
        //     an optional capability must not take the boot down.
        //   * SVM's enable path only writes EFER.SVME and VM_HSAVE_PA, both
        //     gated on the CPUID bit, and reports failure instead of faulting.
        //     QEMU advertises CPUID.1:ECX[31] on every builtin CPU model while
        //     still emulating SVM root operation under TCG, so gating SVM on
        //     the hypervisor bit would close the capability on the only lane
        //     that can exercise it (the x86 hypervisor smoke lane).
        let _under_hypervisor = crate::arch::boot::cpu_features::hypervisor_present();
        // Root operation is per-CPU: the same helper runs on each AP before it
        // goes online (crate::sched::smp::x86_secondary_main).
        if crate::arch::boot::cpu_features::x86_virt_kind()
            == Some(crate::arch::boot::cpu_features::X86Virt::Vmx)
        {
            // VMXON faults (#GP) rather than reporting failure when a
            // precondition is unmet; logged first, a fault stays attributable.
            log::info!(
                "[kernel] x86 virt: VMX preconditions CPUID={} FEATURE_CONTROL={:#x} VMX_BASIC_revid={:#x}",
                hal::vmx::supported(),
                hal::vmx::feature_control(),
                hal::vmx::vmxon_revision()
            );
        }
        match crate::arch::boot::cpu_features::enter_x86_root_on_current_cpu() {
            Ok(kind) => {
                crate::arch::boot::cpu_features::latch_x86_root_active();
                log_info(match kind {
                    crate::arch::boot::cpu_features::X86Virt::Svm => {
                        "x86 virt: root operation active (SVM)"
                    }
                    crate::arch::boot::cpu_features::X86Virt::Vmx => {
                        "x86 virt: root operation active (VMX)"
                    }
                });
            }
            Err(reason) => {
                puts("[INFO] x86 virt: ");
                puts(reason);
                puts("; cap closed\n");
            }
        }
    }

    EarlyPlatform {
        boot_info,
        acpi_info,
        ecam_base: x86_ecam_base,
        timer_ready: x86_timer_ready,
    }
}

/// Log backend, snapshot scratch, ramdisk, PS/2, UART RX IRQ, then the early
/// PCIe scan and VT-d before any DMA-capable driver.
pub(crate) fn platform_init(early: EarlyPlatform) -> PlatformReady {
    let EarlyPlatform {
        boot_info,
        acpi_info,
        ecam_base: x86_ecam_base,
        timer_ready: x86_timer_ready,
    } = early;
    // 5. Hardware Abstraction Layer (HAL) Initialization
    // GDT/IDT/SYSCALL already done at step 1.
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
    // x86_64: load embedded kernel_fs.img into RAM so EarlyLoader can serve ELFs from it.
    // VirtIO devices on q35 are on PCIe — probed via virtio_pci::init() after ECAM scan.
    {
        crate::dev::ramdisk::init_driver();
        crate::arch::dev::ps2_keyboard::init();
        // COM1 TX remains available without ACPI. RX IRQ requires the timer
        // gate (validated MADT plus a clocksource) and an I/O APIC that serves
        // the UART's GSI, because it is routed through IOAPIC/LAPIC.
        if !x86_timer_ready {
            log::warn!("[x86-gate] configured UART IRQ CLOSED: interrupt/timer unavailable");
        } else if !crate::hal::uart_16550::init_input_irq() {
            log::warn!("[x86-gate] configured UART IRQ CLOSED: no I/O APIC serves its GSI");
        }
        // Initialise the RX buffer that vi_handle_uart_irq() writes into.
        crate::dev::uart::init_input();
        // Phase 06: name every declared serial port and whether the machine
        // actually answered the 16550 probe. The console is slot 0; the extras
        // are polled, so an absent one is a refusal, never a silent write.
        {
            let soc = crate::arch::board::soc();
            for (index, port) in soc.serial_ports.iter().enumerate() {
                match crate::hal::uart_16550::port_base_at(index) {
                    Some(base) => log::info!(
                        "[x86-gate] serial port {} base={:#x} irq={} usable",
                        index,
                        base,
                        port.irq
                    ),
                    None => log::warn!(
                        "[x86-gate] serial port {} base={:#x} irq={} absent (16550 probe failed)",
                        index,
                        port.base,
                        port.irq
                    ),
                }
            }
            if soc.serial_ports.len() > crate::hal::uart_16550::MAX_PORTS {
                log::warn!(
                    "[x86-gate] {} declared serial ports exceed the {} this kernel carries",
                    soc.serial_ports.len(),
                    crate::hal::uart_16550::MAX_PORTS
                );
            }
        }
        log_info("x86_64: ramdisk + UART RX IRQ initialised");
    }

    // PCIe ECAM scan + NVMe + e1000 + VirtIO PCI init.
    // x86_64: the complete validated ACPI MCFG window is identity-mapped;
    // the early kernel fallback scans its first admitted bus before Platform.
    let iommu = if x86_ecam_base != 0 {
        // Early first-admitted-bus fallback: seed kernel PCI identity and
        // activate VT-d before any DMA-capable Driver Cell can run. The
        // Platform Cell receives the full validated MCFG range via argv
        // later and performs authoritative multi-bus enumeration.
        crate::dev::pcie_ecam::init();
        let probed = crate::dev::iommu::init(crate::dev::iommu::DmaIsolationInput {
            vtd_base: acpi_info.dmar_base,
            vtd_units: acpi_info.dmar_units,
            vtd_include_pci_all: acpi_info.dmar_include_pci_all,
        });
        let isolation = crate::dev::iommu::activate_isolation(probed);
        // FADT IAPC_BOOT_ARCH bit 3: firmware says MSI must not be enabled.
        crate::arch::dev::pcie_msi::init(
            acpi_info
                .fadt
                .as_ref()
                .is_some_and(|fadt| fadt.iapc_boot_arch & (1 << 3) != 0),
        );
        IommuReady::Activated { isolation }
    } else {
        log::error!("[x86-gate] PCIe CLOSED: validated MCFG required");
        IommuReady::Absent {
            reason: "PCIe closed: no validated MCFG, so no PCI DMA device",
        }
    };
    // VirtIO PCI block init removed (G2 loader redesign phase 06). x86 block I/O
    // is served by the NVMe Driver Cell (F4); the kernel drives no block hardware.

    PlatformReady {
        console: ConsoleReady { boot_info },
        irq: IrqReady,
        clock: ClockReady,
        pci: (x86_ecam_base != 0).then_some(PciReady::Ecam {
            base: x86_ecam_base,
            bus_start: acpi_info.ecam_bus_start,
            bus_end: acpi_info.ecam_bus_end,
        }),
        iommu,
        storage: BootStorage::None,
    }
}

/// Map and register every MADT I/O APIC with the HAL routing table.
///
/// Runs after the kernel PML4 is active: `init_kernel_paging_x86` identity-maps
/// the legacy (last-entry) unit, and every other unit is mapped here. APIC
/// MMIO therefore switches to identity addressing (`set_hhdm_base(0)`).
fn register_x86_ioapics() {
    crate::hal::apic::set_hhdm_base(0);
    let (declared, registered) = crate::acpi::with_topology(|topo| {
        let mut registered = 0;
        for unit in topo.ioapics.as_slice() {
            memory::paging::map_mmio_x86(unit.address as usize, 0x1000);
            if crate::hal::ioapic::add_unit(unit.address, unit.gsi_base) {
                registered += 1;
            }
        }
        (topo.ioapics.len(), registered)
    });
    log::info!(
        "[x86-gate] ioapics registered={} declared={}",
        registered,
        declared
    );
}

/// Hand the HAL the clocksource candidates `init_timers` may choose besides
/// HPET and the TSC:
/// - the FADT PM timer, when it is a port-I/O block on a non-hardware-reduced
///   platform;
/// - one zeroed frame for kvmclock, when KVM advertises it.
///
/// The frame is never freed: once registered, the host writes into it until
/// reset.
#[cfg(not(feature = "g1-cpu-accounting"))]
fn offer_x86_clock_candidates(acpi_info: &crate::acpi::AcpiInfo) {
    if let Some(fadt) = acpi_info.fadt.filter(|fadt| !fadt.hw_reduced()) {
        let timer = fadt.pm_timer;
        match u16::try_from(timer.address) {
            Ok(port) if timer.space_id == crate::acpi::Gas::SYSTEM_IO && port != 0 => {
                crate::hal::set_pm_timer(port, if fadt.pm_timer_32bit() { 32 } else { 24 });
            }
            _ if timer.is_present() => log::warn!(
                "[x86-gate] PM timer at space {} address {:#x} not used (port I/O only)",
                timer.space_id,
                timer.address
            ),
            _ => {}
        }
    }
    if crate::hal::clock::kvmclock::available() {
        let frame = memory::frame::FRAME_ALLOCATOR
            .lock()
            .as_mut()
            .and_then(|frames| frames.allocate_frame());
        if let Some(phys) = frame {
            let virt = memory::frame::phys_to_virt(phys);
            // SAFETY: a freshly allocated, HHDM-mapped frame owned by this call.
            unsafe { core::ptr::write_bytes(virt as *mut u8, 0, 4096) };
            crate::hal::clock::set_pvclock_area(virt, phys as u64);
        }
    }
}

// ── Hooks in the neutral tail of `kmain` ─────────────────────────────────────
// Each runs where the x86_64 code it holds ran before the split; an empty hook
// marks a point where only another architecture has code.

/// After the signed policy is loaded.
pub(crate) fn fixtures_after_policy() {}

/// Right after `task::init`, before every unrelated boot suite.
pub(crate) fn fixtures_after_task_init() {}

/// After the loader corpus, before the trust-model self-tests.
pub(crate) fn fixtures_after_loader_corpus() {}

/// Right after the secondaries start.
pub(crate) fn fixtures_after_secondaries() {}

/// Board MMIO the domain supervisor registry keeps shared with the kernel:
/// none on x86_64.
#[cfg(feature = "native-domains")]
pub(crate) fn supervisor_device_spans() -> impl Iterator<Item = (usize, usize)> {
    core::iter::empty()
}

/// The browser-engine profile's Tier 2 admission posture (QuickJS only).
/// `true` when it decided and logged the posture.
#[cfg(all(
    feature = "native-domains",
    any(feature = "policy-required", feature = "production-relay-image")
))]
pub(crate) fn browser_engine_admission() -> bool {
    #[cfg(feature = "browser-engine-profile")]
    {
        if crate::loader::domain_admission::browser_engine_policy_permitted()
            && crate::loader::domain_admission::enable_for_boot()
        {
            log_info("Tier 2 admission: ENABLED (browser engine profile, QuickJS only)");
        } else {
            log_info(
                "Tier 2 admission: DISABLED (browser engine policy or x86 substrate gate) \
                 — domain-class artifacts are denied",
            );
        }
        true
    }
    #[cfg(not(feature = "browser-engine-profile"))]
    false
}

/// After the native-domain SAS scheduler fixture.
pub(crate) fn fixtures_domain() {
    // Phase 15 domain grant lifecycle: the same kernel-side fixtures the RV64
    // and AArch64 lanes run drive the production `handle_syscall` entry points
    // with real `TaskAddressSpace::Domain` tasks (`S22-X86-GRANT-*`).
    #[cfg(all(cellos_domain_grants, feature = "test-hooks"))]
    {
        crate::task::domain_grant::run_selftest();
        crate::task::grant_gate_selftest::run_primary();
    }
}

/// After the IOMMU page-table fixture: the all-hart park hook (riscv64).
pub(crate) fn fixtures_park() {}

/// Layer-2 hardware security self-test (PKU) and the VMM smoke tests, after
/// all hardware init and before the first cell spawn.
pub(crate) fn fixtures_hardware() {
    #[cfg(feature = "test-hooks")]
    {
        crate::arch::layer2_selftest::run_pku_selftest();
        // Tier 3 x86 VMM P03 M1: SVM world-switch smoke (port-out 'K' + HLT).
        // Only meaningful when SVM root operation was entered at boot.
        if crate::cpu_features::has_x86_virt() {
            crate::arch::hypervisor::svm_registry::x86_smoke();
        } else {
            log_info("X86-VMM-SMOKE: SKIP (no SVM/VMX root)");
        }
    }
    #[cfg(feature = "x86-mmio-smoke")]
    if crate::cpu_features::has_x86_virt() {
        crate::arch::hypervisor::svm_registry::x86_mmio_smoke();
    } else {
        log_info("X86-MMIO-SMOKE: SKIP (no SVM/VMX root)");
    }
}

/// Map and register the Limine linear framebuffer and start the framebuffer
/// console on it.
pub(crate) fn framebuffer_console(console: &ConsoleReady) {
    let boot_info = console.boot_info.get();
    if let Some(fb) = boot_info.framebuffer() {
        let hhdm = boot_info.hhdm_offset();
        let phys_addr = if fb.addr >= hhdm && hhdm > 0 {
            fb.addr - hhdm
        } else {
            fb.addr
        };
        let width = fb.width as u16;
        let height = fb.height as u16;
        let pitch = fb.pitch as usize;
        let size = pitch.wrapping_mul(height as usize);
        let pages = size.div_ceil(4096);
        let owner = types::CellId(0);
        match crate::memory::paging::map_display_framebuffer_user(phys_addr, pages) {
            Ok(_) => {
                let _ = crate::resource_registry::reserve_display_framebuffer(
                    owner, phys_addr, size, width, height, pitch,
                );
                let _ = crate::resource_registry::activate_display_framebuffer(
                    owner, phys_addr, size, width, height, pitch,
                );
                log_info("[boot] x86_64 Limine linear framebuffer mapped and registered");
                crate::arch::dev::fb_console::init(fb.addr, width as usize, height as usize, pitch);
                unsafe {
                    let ptr = fb.addr as *mut u32;
                    let stride = pitch / 4;
                    let total = stride * (height as usize);
                    // Dark slate background (0xFF1E1E2E)
                    for i in 0..total {
                        *ptr.add(i) = 0xFF1E1E2E;
                    }
                    // Top banner bar (40px height, Cyan 0xFF00A8E8)
                    for y in 0..40usize {
                        for x in 0..(width as usize) {
                            *ptr.add(y * stride + x) = 0xFF00A8E8;
                        }
                    }
                }
            }
            Err(_) => {
                log_info("[boot] failed to map linear framebuffer");
            }
        }
    }
}

/// Board diagnostic probe byte (aarch64 boards only).
pub(crate) fn diag_probe(_byte: u8) {}

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

/// Board probes between the interrupt enable and the idle loop (aarch64).
pub(crate) fn idle_probes() {}
