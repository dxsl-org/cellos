//! Driver interfaces and registry.
//!
//! This module manages the lifecycle and registration of kernel drivers.
//! It serves as the central point for:
//! 1. Hardware Abstraction (HAL) implementations (e.g., VirtIO)
//! 2. Driver discovery and initialization
//! 3. Driver naming and ID resolution

// Export the registry for driver management
pub mod registry;

// HAL implementations
pub mod virtio_hal;

// Serial Driver
pub mod uart;

// Drivers
pub mod console_drv;
pub mod fb_console;
pub mod font;
pub mod input_map;
pub mod block;
pub mod mmc;
pub mod ramdisk; // RAM Disk workaround for VirtIO hang
pub mod virtio_common;
pub mod virtio_blk;
pub mod virtio_gpu;
pub mod virtio_input;
pub mod virtio_net;
pub mod virtio_sound; // VirtIO sound (virtio-snd) output — backs the AudioPlay syscall
pub mod gpio_irq;     // GPIO edge IRQ → MMIO-owner IPC dispatch (AArch64 PL061)
pub mod virtio_rng;
pub mod pcie_ecam;    // PCIe ECAM config-space walker (bus 0)
pub mod iommu_pt;     // IOMMU identity-mapping page tables (Sv39 / VT-d SLPT)
pub mod iommu;        // IOMMU common API — three-phase DMA isolation
pub mod iommu_riscv;  // RISC-V IOMMU — 1-level DDT + Sv39 second-stage
pub mod iommu_x86;    // Intel VT-d — TT=TRANSLATED + Sv39 SLPT
pub mod nic;          // NIC selector (VirtIO; PCIe NICs are Driver Cells)
pub mod virtio_pci;   // VirtIO PCI transport for x86_64 q35 (transitional BLK/NET)
pub mod driver_cell;  // Driver Cell registration statics (BLOCK_DRIVER_CELL / NIC_DRIVER_CELL)
// blk_nvme and nic_e1000 have been migrated to Driver Cells:
//   cells/drivers/nvme/   ← NVMe PCIe block driver
//   cells/drivers/e1000/  ← Intel e1000 PCIe NIC driver

/// Initialize drivers subsystem
///
/// Use: Sets up the driver registry and initializes statically linked drivers.
pub fn init() {
    registry::init();

    // Init specific drivers
    virtio_input::init_driver();
    console_drv::init();
    ramdisk::init_driver(); // RAM disk for embedded FAT32 (kernel self-hosted FS)
    // Disable global interrupts during VirtIO init to prevent IRQ deadlocks.
    // VirtIO block raises an IRQ on init; if the PLIC is enabled and the trap
    // handler tries to re-acquire a Spinlock held by this thread, it will spin
    // forever.  We re-enable SIE after all drivers are initialised.
    virtio_blk::init_driver(); // VirtIO block — GPU probe hang fixed via mem::forget
    mmc::init_driver();        // MMC/SD — no-op on QEMU (VirtIO wins); probes SDHCI on real board
    virtio_net::init_driver(); // VirtIO NIC — backs the net service cell
    virtio_gpu::init_driver();
    virtio_sound::init_driver(); // VirtIO sound — backs the AudioPlay syscall
    // VirtIO RNG init deferred: full MMIO probe hangs on RISC-V when probing
    // already-claimed slots (block/net). The no-op stub is sufficient until a
    // safe probe strategy is implemented (skip slots claimed by other drivers).

    // PCIe ECAM scan (pcie_ecam::init() + IOMMU init) is called from main.rs
    // separately on PCIe arches. NVMe and e1000 are now Driver Cells — no
    // kernel-side init_driver() calls needed for those devices.
}
