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
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
mod bcm_pinmux;
pub mod block;
pub mod console_drv;
#[cfg(target_arch = "riscv64")]
pub mod irq_dispatch;
pub mod mmc;
pub mod ramdisk; // RAM Disk workaround for VirtIO hang
pub mod virtio_common;
// virtio_blk deleted (G2 loader redesign phase 06) — block is a Driver Cell now.
pub mod input_irq_ack; // Minimal VirtIO input IRQ ACK shim (event routing is in input Cell)
                       // virtio_net removed: VirtIO NIC is now the virtio-net Driver Cell (cells/drivers/virtio-net/).
pub mod gpio_irq; // GPIO edge IRQ → MMIO-owner IPC dispatch (AArch64 PL061)
pub mod iommu; // IOMMU common API — three-phase DMA isolation
pub mod iommu_pt; // IOMMU identity-mapping page tables (Sv39 / VT-d SLPT)
#[cfg(target_arch = "riscv64")]
pub mod iommu_riscv; // RISC-V IOMMU — 3-level DDT + Sv39 first-stage
#[cfg(target_arch = "riscv64")]
mod iommu_riscv_cmd;
#[cfg(target_arch = "x86_64")]
pub mod iommu_x86; // Intel VT-d — TT=TRANSLATED + Sv39 SLPT
pub mod nic;
pub mod pcie_ecam; // PCIe ECAM config-space walker (bus 0)
pub mod virtio_rng; // NIC selector (VirtIO; PCIe NICs are Driver Cells)
                    // virtio_pci deleted (G2 loader redesign phase 06) — x86 block is the NVMe Driver Cell.
pub mod driver_cell; // Driver Cell role registration and lifecycle teardown.
pub mod irq_wait; // IRQ wait/pending tables for Driver Cell sys_wait_irq
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
                            // G2 loader redesign (phases 05/06): the kernel drives NO block hardware. The
                            // virtio-blk Driver Cell (/bin/block) owns the VirtIO block device (MMIO + DMA)
                            // and registers service::BLOCK_DRIVER; VFS routes all sector I/O to it. Kernel
                            // boot-time block reads (snapshot restore, verify_mbr, EarlyLoader::probe) degrade
                            // gracefully (block::read_sector → Err on the null device). MMC (SDHCI) still
                            // initialises for real boards.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    {
        let board = crate::board::active();
        #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
        bcm_pinmux::apply(hal_soc_bcm27xx::BCM2837.mmio.gpio_base, board.wiring);
        if board.has_driver(cellos_boards::DriverId::SdhciArasan)
            || board.has_driver(cellos_boards::DriverId::SdhciDwCqe)
        {
            mmc::init_driver();
        }
        // Pre-populate PCIE_BARS with VirtIO MMIO slot addresses so Driver Cells
        // can request only slots enabled by the active board contract.
        if board.has_driver(cellos_boards::DriverId::VirtioMmio) {
            for slot in virtio_common::virtio_slots() {
                crate::resource_registry::register_pcie_bar(slot.base, 0x200);
            }
        }
    }
    // VirtIO NIC is now served by the virtio-net Driver Cell (P06 complete).
    // VirtIO RNG init deferred: full MMIO probe hangs on RISC-V when probing
    // already-claimed slots (block/net). The no-op stub is sufficient until a
    // safe probe strategy is implemented (skip slots claimed by other drivers).

    // PCIe ECAM scan (pcie_ecam::init() + IOMMU init) is called from main.rs
    // separately on PCIe arches. NVMe and e1000 are now Driver Cells — no
    // kernel-side init_driver() calls needed for those devices.
}
