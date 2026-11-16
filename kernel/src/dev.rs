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
pub mod block;
pub mod console_drv;
pub mod mmc;
pub mod ramdisk; // RAM Disk workaround for VirtIO hang
pub mod virtio_common;
// virtio_blk deleted (G2 loader redesign phase 06) — block is a Driver Cell now.
pub mod input_irq_ack; // Minimal VirtIO input IRQ ACK shim (event routing is in input Cell)
                       // virtio_net removed: VirtIO NIC is now the virtio-net Driver Cell (cells/drivers/virtio-net/).
pub mod gpio_irq; // GPIO edge IRQ → MMIO-owner IPC dispatch (AArch64 PL061)
pub mod iommu; // IOMMU common API — three-phase DMA isolation
pub mod iommu_pt; // IOMMU identity-mapping page tables (Sv39 / VT-d SLPT)
pub mod pcie_ecam; // PCIe ECAM config-space walker (bus 0)
pub mod virtio_rng; // Kernel entropy source (x86 RDSEED/RDRAND)
                    // virtio_pci deleted (G2 loader redesign phase 06) — x86 block is the NVMe Driver Cell.
pub mod driver_cell; // Driver Cell role registration and lifecycle teardown.
pub mod irq_wait;
pub mod ownership; // Per-device ownership records: MMIO, IRQ, DMA and route checks. // IRQ wait/pending tables for Driver Cell sys_wait_irq
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
                            // and binds its per-device block route (416); VFS routes that device's sector I/O
                            // to the route's provider. Kernel
                            // boot-time block reads (snapshot restore, verify_mbr, EarlyLoader::probe) degrade
                            // gracefully (block::read_sector → Err on the null device). MMC (SDHCI) still
                            // initialises for real boards.
    #[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
    {
        let board = crate::board::active();
        #[cfg(target_arch = "aarch64")]
        if let Some(gpio_base) =
            <hal::aarch64::backend::Board as hal::aarch64::backend::ArmResources>::BOARD_PINMUX_GPIO_BASE
        {
            crate::arch::dev::bcm_pinmux::apply(gpio_base, board.wiring);
        }
        if board.has_driver(cellos_boards::DriverId::SdhciArasan)
            || board.has_driver(cellos_boards::DriverId::SdhciDwCqe)
        {
            mmc::init_driver();
        }
        // Record the VirtIO MMIO slots enabled by the active board contract,
        // so Driver Cells can request only those. A slot alone in its page is
        // recorded as that page so its claim maps it for U-mode (a sub-page
        // window is never user-mapped, and RISC-V has no kernel mediation of
        // sub-page accesses).
        if board.has_driver(cellos_boards::DriverId::VirtioMmio) {
            for slot in virtio_common::virtio_slots() {
                let window = virtio_common::slot_window(
                    slot.base,
                    virtio_common::virtio_slots().map(|other| other.base),
                );
                ownership::register_mmio_device(
                    slot.base,
                    window,
                    ownership::DeviceKind::VirtioSlot,
                    u8::try_from(slot.irq).ok(),
                );
            }
        }
        if let Some((base, size)) = crate::resource_registry::dwc2_aperture() {
            // The board's USB one-shot line belongs to the aperture's owner.
            #[cfg(target_arch = "aarch64")]
            let line = <hal::aarch64::backend::Board as hal::aarch64::backend::ArmIrqController>::USB_ONE_SHOT_IRQ
                .and_then(|line| u8::try_from(line).ok());
            #[cfg(not(target_arch = "aarch64"))]
            let line = None;
            ownership::register_mmio_device(base, size, ownership::DeviceKind::Dwc2, line);
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
