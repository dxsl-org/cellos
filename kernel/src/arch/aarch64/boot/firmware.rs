// SPDX-License-Identifier: MPL-2.0
//! aarch64 firmware tree: the boot memory map by the board backend's
//! [`DtbPolicy`], the SMP facts and the generic ECAM bridge it describes.

use crate::boot::{
    dtb_memory, fallback_memory_entry, MemoryMapEntry, MemoryType, SimpleBootInfo, DTB_BOOT_INFO,
    DTB_MEMORY_MAP,
};

use core::sync::atomic::{AtomicUsize, Ordering};

use hal::aarch64::backend::{ArmPlatform, Board, DtbPolicy};

// AArch64: the board backend's descriptor (`board::default_aarch64_board`).
const AARCH64_BOARD: &cellos_boards::BoardDescriptor = crate::board::default_aarch64_board();

// Raspberry Pi (`DtbPolicy::Conservative` / `Required`): kernel at 0x80000,
// RAM below the peripheral aperture. VideoCore firmware loads kernel8.img at
// 0x80000 and owns the current GPU split above the firmware DTB's ARM-visible
// memory range. The static map is the board descriptor's two fallback spans.
static FALLBACK_MEMORY_MAP: [MemoryMapEntry; 2] = [
    fallback_memory_entry(AARCH64_BOARD, 0),
    fallback_memory_entry(AARCH64_BOARD, 1),
];

pub static FALLBACK_BOOT_INFO: SimpleBootInfo = SimpleBootInfo {
    memory_map: &FALLBACK_MEMORY_MAP,
    kernel_phys_base: AARCH64_BOARD.boot.kernel_load_base,
    hhdm_offset: 0x0,
};

// The RPi3 map used to take its kernel base from the first fallback span; the
// descriptors keep both at the firmware load address.
const _: () = assert!(
    AARCH64_BOARD.fallback_memory[0].base == AARCH64_BOARD.boot.kernel_load_base
        || matches!(<Board as ArmPlatform>::DTB_POLICY, DtbPolicy::Optional)
);

// QEMU ARM virt (`DtbPolicy::Optional`; 256MB at 0x4000_0000, kernel loaded at
// 0x4008_0000): MMIO below 0x4000_0000 is mapped by init_kernel_paging; RAM
// regions only here.
//
// The kernel span is measured at RUNTIME from the linker's `__stack_top`, not
// hardcoded: EMBEDDED_OVERRIDE images make the binary size unbounded (the
// hypervisor build embeds Alpine and reaches 60 MB). The previous fixed 32 MB
// boundary let the frame allocator re-issue everything past it as free RAM —
// the kernel then overwrote the tail of its own embedded VIFS1 image and died
// in a recursive same-EL sync-abort storm (PC pinned at vt_sync_spx) the
// moment the corrupted FAT was walked.
static mut RUNTIME_MEMORY_MAP: [MemoryMapEntry; 2] = [
    MemoryMapEntry {
        base: AARCH64_BOARD.fallback_memory[0].base as usize,
        length: 0, // patched by optional_dtb_boot_info()
        ty: MemoryType::Kernel,
    },
    MemoryMapEntry {
        base: 0,
        length: 0, // patched by optional_dtb_boot_info()
        ty: MemoryType::Usable,
    },
];

static mut FALLBACK_BOOT_INFO_RUNTIME: SimpleBootInfo = SimpleBootInfo {
    memory_map: &[],
    kernel_phys_base: AARCH64_BOARD.boot.kernel_load_base,
    hhdm_offset: 0x0,
};

static FALLBACK_DTB_RAM_START: AtomicUsize = AtomicUsize::new(0);

static FALLBACK_DTB_RAM_END: AtomicUsize = AtomicUsize::new(0);

/// RAM range of the firmware DTB's first memory region (`DtbPolicy::Optional`).
pub fn fallback_dtb_ram_range() -> (usize, usize) {
    (
        FALLBACK_DTB_RAM_START.load(Ordering::Relaxed),
        FALLBACK_DTB_RAM_END.load(Ordering::Relaxed),
    )
}

/// Firmware SMP facts, read once from the device tree during boot.
///
/// The kernel starts secondary cores through firmware (`PSCI_CPU_ON` over the
/// conduit the tree declares, or the tree's spin-table release words) and must
/// not guess either fact later: the tree is only reliably readable while boot
/// code is still running, and an SMC on a machine whose firmware never asked
/// for one is an undefined instruction.
pub mod firmware_smp {
    use crate::boot::firmware_cpus;
    use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};

    /// The instruction the firmware expects for PSCI calls (`/psci/method`).
    ///
    /// The hal's own type: this module only reports what the tree said, and a
    /// second enum with the same two variants would be a second source of truth
    /// for the caller that has to pick an instruction.
    pub use hal::aarch64::psci::Conduit;

    const UNREAD: u8 = 0;
    const NO_PSCI: u8 = 1;
    const SMC: u8 = 2;
    const HVC: u8 = 3;

    static CONDUIT: AtomicU8 = AtomicU8::new(UNREAD);
    static CPUS: AtomicUsize = AtomicUsize::new(0);
    /// Spin-table `cpu-release-addr` of each `/cpus` node, in tree order (0 = none).
    static CPU_RELEASE_ADDRS: [AtomicU64; 32] = [const { AtomicU64::new(0) }; 32];

    /// Read `/psci` and the `/cpus` population — each CPU's `MPIDR_EL1`
    /// affinity and spin-table release address included — from the firmware
    /// tree ([`firmware_cpus`] does the parsing).
    pub fn note(tree: &fdt::Fdt) {
        let conduit = match firmware_cpus::psci_method(tree) {
            Some("smc") => SMC,
            Some("hvc") => HVC,
            _ => NO_PSCI,
        };
        CONDUIT.store(conduit, Ordering::Relaxed);
        CPUS.store(firmware_cpus::cpus(tree).count(), Ordering::Relaxed);

        let ids = firmware_cpus::cpus(tree).map(|cpu| cpu.affinity);
        if let Err(error) = crate::sched::smp::CPU_MPIDRS.describe(ids) {
            log::warn!(
                "[boot] /cpus affinities unusable ({:?}) — CPU index = MPIDR Aff0",
                error
            );
        }

        for (slot, cpu) in CPU_RELEASE_ADDRS.iter().zip(firmware_cpus::cpus(tree)) {
            slot.store(cpu.release_addr.unwrap_or(0), Ordering::Relaxed);
        }
    }

    /// The PSCI conduit, or `None` when the tree declares no PSCI.
    pub fn psci_conduit() -> Option<Conduit> {
        match CONDUIT.load(Ordering::Relaxed) {
            SMC => Some(Conduit::Smc),
            HVC => Some(Conduit::Hvc),
            _ => None,
        }
    }

    /// CPUs the firmware tree describes (0 when the tree was never read).
    pub fn cpu_count() -> usize {
        CPUS.load(Ordering::Relaxed)
    }

    /// Was a firmware tree read at all? (`/psci` only means anything if one was.)
    pub fn tree_available() -> bool {
        CONDUIT.load(Ordering::Relaxed) != UNREAD
    }

    /// The spin-table release address of the `hart_id`-th `/cpus` node, if the tree gives one.
    pub fn cpu_release_addr(hart_id: usize) -> Option<u64> {
        CPU_RELEASE_ADDRS
            .get(hart_id)
            .map(|slot| slot.load(Ordering::Relaxed))
            .filter(|&addr| addr != 0)
    }
}

/// Bus-0 configuration space (32 devices × 8 functions × 4 KiB), the part of the
/// ECAM window `pcie_ecam::init` scans on AArch64.
const ECAM_BUS0_BYTES: usize = 1 << 20;

/// Record the firmware tree's generic ECAM host bridge for the PCIe scan, but
/// only when its bus-0 window lies inside a kernel MMIO identity map: an
/// unmapped window (e.g. QEMU virt's high ECAM above 256 GiB) would fault on
/// the first config read.
fn note_pcie_ecam(tree: &fdt::Fdt) {
    use hal::aarch64::backend::ArmResources;

    let Some(ecam) = tree
        .find_compatible(&["pci-host-ecam-generic"])
        .and_then(|node| node.reg())
        .and_then(|mut reg| reg.next())
    else {
        return;
    };
    let base = ecam.starting_address as usize;
    let mapped = base.checked_add(ECAM_BUS0_BYTES).is_some_and(|end| {
        <Board as ArmResources>::KERNEL_MMIO_MAPS
            .iter()
            .any(|map| map.base <= base && end <= map.end)
    });
    if mapped {
        crate::dev::pcie_ecam::record_ecam_base_aarch64(base);
    } else {
        log::warn!(
            "[boot] ECAM at {:#x} is outside the kernel MMIO maps; not scanned",
            base
        );
    }
}

/// AArch64 boot memory map, by the board backend's [`DtbPolicy`].
pub fn fallback_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    match <Board as ArmPlatform>::DTB_POLICY {
        DtbPolicy::Optional => optional_dtb_boot_info(dtb),
        DtbPolicy::Conservative => conservative_dtb_boot_info(dtb),
        DtbPolicy::Required => required_dtb_boot_info(dtb),
    }
}

/// `DtbPolicy::Optional` (QEMU virt): the descriptor's spans, bounded by the
/// DTB's first memory region, with the kernel region sized at runtime.
fn optional_dtb_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    use core::ptr::{addr_of, addr_of_mut};
    extern "C" {
        /// Highest kernel address (end of .bss.stack) — linker-aarch64.ld.
        static __stack_top: u8;
    }
    const RAM_BASE: usize = AARCH64_BOARD.fallback_memory[0].base as usize;
    #[cfg(feature = "qemu-virt-1g")]
    const FALLBACK_RAM_END: usize = 0x8000_0000;
    #[cfg(not(feature = "qemu-virt-1g"))]
    const FALLBACK_RAM_END: usize =
        (AARCH64_BOARD.fallback_memory[1].base + AARCH64_BOARD.fallback_memory[1].size) as usize;
    const ALIGN_2M: usize = 0x20_0000;
    let dtb_ram = if dtb != 0 {
        // SAFETY: QEMU/firmware passes a valid FDT pointer in x0; Fdt validates
        // its header before any node traversal.
        unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }
            .ok()
            .and_then(|tree| {
                // The tree is only reliably readable here — record what the
                // firmware says about PSCI and the CPU population now.
                firmware_smp::note(&tree);
                note_pcie_ecam(&tree);
                tree.memory().regions().find_map(|region| {
                    let start = region.starting_address as usize;
                    region
                        .size
                        .and_then(|size| start.checked_add(size))
                        .map(|end| (start, end))
                })
            })
    } else {
        None
    };
    if let Some((start, end)) = dtb_ram {
        FALLBACK_DTB_RAM_START.store(start, Ordering::Relaxed);
        FALLBACK_DTB_RAM_END.store(end, Ordering::Relaxed);
    }
    let ram_end = match dtb_ram {
        Some((start, end)) if start <= RAM_BASE && end > RAM_BASE => end,
        _ => FALLBACK_RAM_END,
    };
    // SAFETY: single-hart early boot; the statics are written exactly once here
    // before any other reader exists, then only shared immutably.
    unsafe {
        let end = addr_of!(__stack_top) as usize;
        let kernel_len = if end > RAM_BASE {
            (end - RAM_BASE + ALIGN_2M - 1) & !(ALIGN_2M - 1)
        } else {
            0x200_0000
        };
        *addr_of_mut!(RUNTIME_MEMORY_MAP) = [
            MemoryMapEntry {
                base: RAM_BASE,
                length: kernel_len,
                ty: MemoryType::Kernel,
            },
            MemoryMapEntry {
                base: RAM_BASE + kernel_len,
                length: ram_end.saturating_sub(RAM_BASE + kernel_len),
                ty: MemoryType::Usable,
            },
        ];
        (*addr_of_mut!(FALLBACK_BOOT_INFO_RUNTIME)).memory_map =
            core::slice::from_raw_parts(addr_of!(RUNTIME_MEMORY_MAP) as *const MemoryMapEntry, 2);
        &*addr_of!(FALLBACK_BOOT_INFO_RUNTIME)
    }
}

/// `DtbPolicy::Conservative` (RPi3): build the allocator map from the
/// firmware-adjusted DTB memory node.
///
/// VideoCore owns RAM above the ARM-visible range and allocates framebuffers
/// there. The static board map remains a conservative recovery fallback only;
/// a valid firmware DTB is authoritative for the current GPU memory split.
fn conservative_dtb_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    use core::ptr::{addr_of, addr_of_mut};
    extern "C" {
        static __stack_top: u8;
    }

    if dtb != 0 {
        // SAFETY: VideoCore/U-Boot supplies a mapped FDT pointer; the parser
        // validates its header before traversing memory nodes.
        if let Ok(tree) = unsafe { fdt::Fdt::from_ptr(dtb as *const u8) } {
            let kernel_base = FALLBACK_BOOT_INFO.kernel_phys_base as usize;
            let kernel_end = addr_of!(__stack_top) as usize;
            // SAFETY: early single-hart boot publishes these statics once,
            // before the allocator or any secondary execution context exists.
            let result = unsafe {
                dtb_memory::build(
                    &tree,
                    kernel_base,
                    kernel_end,
                    &mut *addr_of_mut!(DTB_MEMORY_MAP),
                )
            };
            match result {
                Ok(count) => unsafe {
                    (*addr_of_mut!(DTB_BOOT_INFO)).memory_map = core::slice::from_raw_parts(
                        addr_of!(DTB_MEMORY_MAP) as *const MemoryMapEntry,
                        count,
                    );
                    (*addr_of_mut!(DTB_BOOT_INFO)).kernel_phys_base = kernel_base as u64;
                    return &*addr_of!(DTB_BOOT_INFO);
                },
                Err(error) => log::warn!("[boot] RPi3 DTB memory map rejected: {:?}", error),
            }
        } else {
            log::warn!("[boot] invalid RPi3 DTB memory map; using conservative fallback");
        }
    } else {
        log::warn!("[boot] no RPi3 DTB memory map; using conservative fallback");
    }

    // Fallback path: no usable firmware DTB. The board descriptor's static spans are audited but
    // assume a small image — `kernel` is a fixed 16 MiB from 0x80000 and `usable` starts at
    // 0x1080000 — which is wrong for any payload that carries real data. A 43.6 MB payload (a real
    // checkpoint inside VIFS1) panicked at compositor setup because the frame allocator's first
    // frames landed inside the image, and the cell overwrote the kernel's own code and ramdisk.
    // Size the kernel region from the linker end symbol instead, exactly as the QEMU-virt path does;
    // the descriptor's spans stay the *ceiling* for usable RAM.
    {
        const ALIGN_2M: usize = 0x20_0000;
        let ram_base = FALLBACK_BOOT_INFO.kernel_phys_base as usize;
        let descriptor_usable = AARCH64_BOARD.fallback_memory[1];
        let ram_end = (descriptor_usable.base + descriptor_usable.size) as usize;
        // SAFETY: single-hart early boot; the statics are written once here, before any other
        // reader exists, then only shared immutably.
        unsafe {
            let end = addr_of!(__stack_top) as usize;
            let kernel_len = if end > ram_base {
                (end - ram_base + ALIGN_2M - 1) & !(ALIGN_2M - 1)
            } else {
                descriptor_usable.base as usize - ram_base
            };
            let map = &mut *addr_of_mut!(DTB_MEMORY_MAP);
            map[0] = MemoryMapEntry {
                base: ram_base,
                length: kernel_len,
                ty: MemoryType::Kernel,
            };
            map[1] = MemoryMapEntry {
                base: ram_base + kernel_len,
                length: ram_end.saturating_sub(ram_base + kernel_len),
                ty: MemoryType::Usable,
            };
            (*addr_of_mut!(DTB_BOOT_INFO)).memory_map =
                core::slice::from_raw_parts(addr_of!(DTB_MEMORY_MAP) as *const MemoryMapEntry, 2);
            (*addr_of_mut!(DTB_BOOT_INFO)).kernel_phys_base = ram_base as u64;
            &*addr_of!(DTB_BOOT_INFO)
        }
    }
}

/// `DtbPolicy::Required` (RPi4, Apple t8103): a valid firmware DTB must be
/// present; the memory map is the descriptor's static fallback.
///
/// The tree is still the only source of the CPU topology — the `/cpus`
/// affinities (t8103's P-cores are a second cluster) and the spin-table
/// release words both boards start their secondaries through — so it is read
/// here, while it is intact, exactly as the `Optional` path does.
fn required_dtb_boot_info(dtb: usize) -> &'static SimpleBootInfo {
    if dtb == 0 {
        panic!(
            "[boot] {} {} requires a firmware DTB",
            AARCH64_BOARD.vendor, AARCH64_BOARD.model
        );
    }
    // SAFETY: the firmware (VideoCore, m1n1) supplies the DTB pointer; the
    // parser validates its header before any node traversal.
    let Ok(tree) = (unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }) else {
        panic!(
            "[boot] {} {} firmware DTB is invalid",
            AARCH64_BOARD.vendor, AARCH64_BOARD.model
        );
    };
    firmware_smp::note(&tree);
    // Records only a generic ECAM bridge inside a kernel MMIO map; neither
    // board's PCIe root complex is one, so this is a no-op on their trees.
    note_pcie_ecam(&tree);
    &FALLBACK_BOOT_INFO
}

/// Compact the firmware tree's `FDT_NOP` holes ([`crate::boot::fdt_nops`]),
/// then let the board backend read what it needs from it
/// (`ArmPlatform::note_firmware_tree`). Runs right after
/// [`crate::boot::effective_dtb`] picked the tree.
pub(crate) fn note_firmware_tree(dtb: usize) {
    if dtb != 0 {
        // SAFETY: firmware passes a mapped FDT pointer in x0; the header is
        // read before `totalsize` is trusted.
        let header = unsafe { core::slice::from_raw_parts(dtb as *const u8, 8) };
        if let Some(size) = crate::boot::fdt_nops::total_size(header) {
            // SAFETY: the firmware hands the kernel its tree's `totalsize`
            // bytes; nothing else references them on the single boot hart.
            let blob = unsafe { core::slice::from_raw_parts_mut(dtb as *mut u8, size) };
            match crate::boot::fdt_nops::strip(blob) {
                Ok(0) => {}
                Ok(removed) => log::info!("[boot] DTB: {removed} bytes of FDT_NOP compacted"),
                Err(error) => log::warn!("[boot] DTB left as is: {error:?}"),
            }
        }
        // SAFETY: firmware passes a mapped FDT pointer in x0; `Fdt` validates
        // the header before any node traversal.
        if let Ok(tree) = unsafe { fdt::Fdt::from_ptr(dtb as *const u8) } {
            <Board as ArmPlatform>::note_firmware_tree(&tree);
        }
    }
}
