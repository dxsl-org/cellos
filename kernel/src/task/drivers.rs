//! Kernel-side driver shims, capability registry, and routing tables.
//!
//! Only kernel-resident drivers that satisfy the Boundary Law remain here
//! (early-boot console, VirtIO block + MMC as G2-pending fallbacks, IOMMU,
//! IRQ wait tables).  All other device drivers live in `cells/drivers/`.

// Export the registry for driver management
pub mod registry;

// HAL implementations
pub mod virtio_hal;

// Serial Driver
pub mod uart;

// Drivers
pub mod console_drv;
pub mod block;
pub mod mmc;
pub mod ramdisk; // RAM Disk workaround for VirtIO hang
pub mod virtio_common;
pub mod virtio_blk;
pub mod input_irq_ack; // Minimal VirtIO input IRQ ACK shim (event routing is in input Cell)
// virtio_net removed: VirtIO NIC is now the virtio-net Driver Cell (cells/drivers/virtio-net/).
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
pub mod irq_wait;     // IRQ wait/pending tables for Driver Cell sys_wait_irq
// blk_nvme and nic_e1000 have been migrated to Driver Cells:
//   cells/drivers/nvme/   ← NVMe PCIe block driver
//   cells/drivers/e1000/  ← Intel e1000 PCIe NIC driver

/// Initialize drivers subsystem
///
/// Use: Sets up the driver registry and initializes statically linked drivers.
pub fn init() {
    registry::init();

    // Init specific drivers
    input_irq_ack::init_driver(); // ACK-only shim; event routing is in input service Cell
    console_drv::init();
    ramdisk::init_driver(); // RAM disk for embedded FAT32 (kernel self-hosted FS)
    // Disable global interrupts during VirtIO init to prevent IRQ deadlocks.
    // VirtIO block raises an IRQ on init; if the PLIC is enabled and the trap
    // handler tries to re-acquire a Spinlock held by this thread, it will spin
    // forever.  We re-enable SIE after all drivers are initialised.
    virtio_blk::init_driver(); // VirtIO block — GPU probe hang fixed via mem::forget
    mmc::init_driver();        // MMC/SD — no-op on QEMU (VirtIO wins); probes SDHCI on real board
    // Pre-populate PCIE_BARS with VirtIO MMIO slot addresses so virtio-net Driver Cell
    // can claim them via sys_request_mmio (PcieDriverCap path). Must run before
    // virtio_net::init_driver so the BAR table is ready before net IPC begins.
    for slot in virtio_common::virtio_slots() {
        crate::resource_registry::register_pcie_bar(slot.base, 0x200);
    }
    // VirtIO NIC is now served by the virtio-net Driver Cell (P06 complete).
    // VirtIO RNG init deferred: full MMIO probe hangs on RISC-V when probing
    // already-claimed slots (block/net). The no-op stub is sufficient until a
    // safe probe strategy is implemented (skip slots claimed by other drivers).

    // PCIe ECAM scan (pcie_ecam::init() + IOMMU init) is called from main.rs
    // separately on PCIe arches. NVMe and e1000 are now Driver Cells — no
    // kernel-side init_driver() calls needed for those devices.
}
