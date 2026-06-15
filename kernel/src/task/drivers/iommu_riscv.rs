//! RISC-V IOMMU PCIe driver — bare/passthrough mode (IOVA == PA).
//!
//! Discovers the RISC-V IOMMU PCIe device (class 0x08/0x06/0x00, QEMU 8.2+
//! vendor 0x1b36 device 0x0014), maps its BAR0 MMIO registers, and configures
//! DDTP.MODE=1 (bare passthrough). DMA proceeds without page-table overhead.
//!
//! Requires QEMU flag: `-device riscv-iommu-pci,bus=pcie.0`

use crate::task::drivers::pcie_ecam;

// PCI device identification (RISC-V IOMMU spec §3 + QEMU `hw/riscv/riscv-iommu.c`)
const CLASS:  u8 = 0x08;
const SUB:    u8 = 0x06;
const PROGIF: u8 = 0x00;

// Register offsets within BAR0 (RISC-V IOMMU spec v1.0 §3.1)
const REG_CAPS: usize = 0x00; // 64-bit capabilities
const REG_FCTL: usize = 0x08; // 32-bit feature control
const REG_DDTP: usize = 0x10; // 64-bit device-directory-table pointer
const REG_IPSR: usize = 0x38; // 32-bit interrupt-pending status

// DDTP.MODE field values (bits [3:0])
const DDTP_MODE_BARE: u64 = 1; // MODE=1 → IOVA == PA (bare passthrough)

#[inline]
unsafe fn read32(base: usize, off: usize) -> u32 {
    // SAFETY: caller guarantees `base` is a valid identity-mapped MMIO window.
    unsafe { core::ptr::read_volatile((base + off) as *const u32) }
}

#[inline]
unsafe fn write32(base: usize, off: usize, val: u32) {
    // SAFETY: caller guarantees `base` is a valid identity-mapped MMIO window.
    unsafe { core::ptr::write_volatile((base + off) as *mut u32, val) }
}

#[inline]
unsafe fn write64(base: usize, off: usize, val: u64) {
    // SAFETY: caller guarantees `base` is a valid identity-mapped MMIO window.
    unsafe { core::ptr::write_volatile((base + off) as *mut u64, val) }
}

/// Initialise the RISC-V IOMMU in bare/passthrough mode.
///
/// Called from `iommu::init()` on `riscv64` targets. Falls through if the
/// IOMMU PCIe device is absent (QEMU < 8.2 or missing `-device riscv-iommu-pci`).
pub fn init_riscv_iommu() {
    let dev = match pcie_ecam::find_class(CLASS, SUB, PROGIF) {
        Some(d) => d,
        None => {
            log::warn!(
                "[iommu] RISC-V IOMMU not found \
                 (requires QEMU ≥ 8.2 with -device riscv-iommu-pci,bus=pcie.0)"
            );
            return;
        }
    };

    let bar0 = dev.bars[0].base_addr() as usize;
    if bar0 == 0 {
        log::warn!("[iommu] RISC-V IOMMU BAR0 == 0 (firmware did not configure MMIO)");
        return;
    }

    // On RISC-V the MMIO space is identity-mapped (VA == PA) by init_kernel_paging.
    // SAFETY: bar0 is the identity-mapped MMIO base of the RISC-V IOMMU device.
    let _caps = unsafe { core::ptr::read_volatile((bar0 + REG_CAPS) as *const u64) };

    unsafe {
        // a. FCTL = 0: little-endian, no memory-mapped command FIFO.
        write32(bar0, REG_FCTL, 0);

        // b. DDTP = MODE=1 (bare passthrough). PPN field = 0 (unused in bare mode).
        write64(bar0, REG_DDTP, DDTP_MODE_BARE);

        // c. Clear any pending fault interrupts (write-1-to-clear).
        let ipsr = read32(bar0, REG_IPSR);
        if ipsr != 0 {
            write32(bar0, REG_IPSR, ipsr);
        }
    }

    super::iommu::set_active();
    log::info!(
        "[iommu] RISC-V IOMMU: bare passthrough enabled (vendor={:04x} dev={:04x})",
        dev.vendor_id, dev.device_id
    );
}
