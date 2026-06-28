//! VirtIO PCI transport for x86_64 QEMU q35 — BLK only.
//!
//! QEMU q35 exposes VirtIO devices via PCIe (vendor 0x1AF4) rather than the
//! MMIO slots used by the ARM64/RISC-V `virt` machine.  This module scans the
//! ECAM bus-0 snapshot built by `pcie_ecam::init()`, locates VirtIO BLK
//! devices, maps their BAR1 MMIO region, and hands the mapped `VirtIOHeader`
//! pointer to `MmioTransport`.  VirtIO NET was removed (P08): handled by the
//! virtio-net Driver Cell on RISC-V/ARM64, e1000 Driver Cell on x86_64.
//!
//! ## VirtIO PCI device layout (QEMU default — transitional)
//!
//! Transitional VirtIO devices expose the legacy MMIO register file through a
//! Memory BAR (BAR1 on q35).  The register layout at that BAR is identical to
//! the MMIO transport (`VirtIOHeader`), so `MmioTransport::new()` works without
//! any modification to the upstream driver code.
//!
//! BAR0 is an I/O-space BAR (skipped; `pcie_ecam` decodes it as `Bar::Io`).
//! BAR1 is a 32-bit MMIO BAR containing `VirtIOHeader`.
//!
//! Modern VirtIO PCI (device IDs 0x1040+) uses a different capability-based
//! layout; that path is deferred (comment in `init` explains the gate).
//!
//! ## Call ordering
//!
//! `pcie_ecam::init()` → `virtio_pci::init()`
//! Must run after the ECAM scan populates `PCI_DEVICES`.

use crate::task::drivers::pcie_ecam::{self, Bar};
use crate::task::drivers::virtio_hal::VirtioHal;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::transport::{DeviceType, Transport};

// ── VirtIO PCI vendor / device IDs ───────────────────────────────────────────

const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// Transitional VirtIO block (legacy BAR1 MMIO header layout).
const VIRTIO_PCI_BLK_LEGACY:  u16 = 0x1001;
/// Modern VirtIO block (capability-based; deferred).
const VIRTIO_PCI_BLK_MODERN:  u16 = 0x1042;
// VirtIO PCI NET (0x1000 legacy / 0x1041 modern) removed: VirtIO net is now the
// virtio-net Driver Cell (cells/drivers/virtio-net/).  x86_64 uses e1000 Driver Cell.

// BAR size to map: 4 KiB covers the VirtIOHeader (0x100 bytes) plus queue
// notify registers with plenty of headroom.  QEMU reports the real size during
// the ECAM scan; we use it directly from the decoded `Bar`.
// Used only in the x86_64 `map_mmio_x86` call; suppressed on other arches.
#[allow(dead_code)]
const FALLBACK_BAR_MAP_SIZE: usize = 4096;

// ── VirtIO PCI class (mass-storage 0x01, subclass 0x00, prog-if 0x00) ────────
// VirtIO network class is 0x02/0x00/0x00 — but we query by vendor ID instead,
// which is reliable across legacy/modern and avoids false positives.

// ── Init ──────────────────────────────────────────────────────────────────────

/// Probe PCIe bus 0 for VirtIO BLK devices (vendor 0x1AF4) and initialise them.
///
/// VirtIO NET is handled by the virtio-net Driver Cell; this function initialises
/// BLK only.  x86_64 uses e1000 Driver Cell for networking.
///
/// On x86_64, every BAR is identity-mapped via `map_mmio_x86` before the
/// `VirtIOHeader` pointer is passed to `MmioTransport::new`.
///
/// # Limitations
/// - Modern VirtIO PCI (device ID 0x1042) requires walking VirtIO PCI capability
///   structures; that path is deferred.
/// - MSI-X is not wired; VirtIO runs in polled mode.
pub fn init() {
    let devices = pcie_ecam::devices();

    let mut blk_done = false;

    for dev in &devices {
        if dev.vendor_id != VIRTIO_VENDOR_ID {
            continue;
        }

        log::info!(
            "[virtio_pci] found vendor={:#06x} device={:#06x} bdf={:02x}:{:02x}.{}",
            dev.vendor_id,
            dev.device_id,
            dev.bdf.0,
            dev.bdf.1,
            dev.bdf.2,
        );

        match dev.device_id {
            VIRTIO_PCI_BLK_LEGACY => {
                if blk_done {
                    log::info!("[virtio_pci] BLK already initialised, skipping");
                    continue;
                }
                if let Some(bar_mmio) = find_mmio_bar(dev) {
                    if init_blk(bar_mmio) {
                        blk_done = true;
                    }
                } else {
                    log::warn!(
                        "[virtio_pci] BLK device {:04x}:{:04x} has no usable MMIO BAR — skipped",
                        dev.vendor_id, dev.device_id
                    );
                }
            }
            VIRTIO_PCI_BLK_MODERN => {
                // Modern VirtIO PCI requires walking vendor capability structures
                // (VIRTIO_PCI_CAP_COMMON_CFG / NOTIFY / ISR) to locate the correct
                // BAR regions.  Deferred: `virtio-drivers` `PciTransport` requires
                // its own `PciRoot` abstraction.
                log::warn!(
                    "[virtio_pci] Modern VirtIO BLK PCI device {:#06x} detected — init deferred \
                     (modern capability walk not yet implemented)",
                    dev.device_id
                );
            }
            other => {
                log::info!(
                    "[virtio_pci] Unhandled VirtIO device ID {:#06x} — skipped",
                    other
                );
            }
        }
    }

    if !blk_done {
        log::info!("[virtio_pci] No VirtIO PCI BLK device found on PCIe bus 0");
    }
}

// ── BAR selection ─────────────────────────────────────────────────────────────

/// Find the first usable MMIO BAR base address for a VirtIO PCI device.
///
/// Transitional VirtIO PCI:
///   BAR0 = I/O space (skipped; `pcie_ecam` decodes these as `Bar::Io`)
///   BAR1 = 32-bit MMIO containing `VirtIOHeader`
///
/// Returns the physical base address of the MMIO BAR, or `None` if no MMIO
/// BAR is present (I/O-only device or firmware left BAR unassigned).
fn find_mmio_bar(dev: &pcie_ecam::PciDevice) -> Option<usize> {
    for bar in &dev.bars {
        let addr = bar.base_addr();
        if addr == 0 {
            continue;
        }
        match bar {
            Bar::Memory32 { .. } | Bar::Memory64 { .. } => return Some(addr as usize),
            Bar::Io | Bar::None => continue,
        }
    }
    None
}

// ── Device init helpers ───────────────────────────────────────────────────────

/// Attempt to initialise a VirtIO block device from a PCI BAR1 MMIO address.
///
/// Maps the MMIO window (x86_64 only), constructs an `MmioTransport` from the
/// `VirtIOHeader` at `bar_phys`, verifies device type, and stores the device
/// in `virtio_blk::BLOCK_DEVICE`.
///
/// Returns `true` on success.
fn init_blk(bar_phys: usize) -> bool {
    use crate::task::drivers::virtio_blk;
    use virtio_drivers::device::blk::VirtIOBlk;

    // Already initialised by the MMIO path on this arch.
    if virtio_blk::is_present() {
        log::info!("[virtio_pci] BLK BLOCK_DEVICE already set — skipping PCI init");
        return false;
    }

    // On x86_64, PCIe BARs are not in the pre-mapped identity region;
    // we must explicitly map each BAR page before accessing it.
    #[cfg(target_arch = "x86_64")]
    crate::memory::paging::map_mmio_x86(bar_phys, FALLBACK_BAR_MAP_SIZE);

    // SAFETY: bar_phys is now identity-mapped (VA == PA after map_mmio_x86).
    // The pointer is non-null and 4-byte aligned (BAR allocation guarantees
    // natural alignment).  MmioTransport::new validates magic/version fields
    // before any further register access.
    let header = match core::ptr::NonNull::new(bar_phys as *mut VirtIOHeader) {
        Some(h) => h,
        None => {
            log::warn!("[virtio_pci] BLK BAR physical address is zero");
            return false;
        }
    };

    // SAFETY: header points to an identity-mapped VirtIOHeader for a
    // transitional VirtIO PCI device; magic/version are validated inside.
    let transport = match unsafe { MmioTransport::new(header) } {
        Ok(t) => t,
        Err(e) => {
            log::warn!("[virtio_pci] BLK MmioTransport::new failed at {:#x}: {:?}", bar_phys, e);
            return false;
        }
    };

    if transport.device_type() != DeviceType::Block {
        log::warn!(
            "[virtio_pci] BLK BAR at {:#x} reported device type {:?}, expected Block",
            bar_phys,
            transport.device_type()
        );
        // Forget: do not reset a device we do not own.
        core::mem::forget(transport);
        return false;
    }

    match VirtIOBlk::<VirtioHal, MmioTransport>::new(transport) {
        Ok(blk) => {
            virtio_blk::store_pci_device(blk);
            log::info!("[virtio_pci] BLK initialised from PCI BAR at {:#x}", bar_phys);
            true
        }
        Err(e) => {
            log::warn!("[virtio_pci] BLK VirtIOBlk::new failed: {:?}", e);
            false
        }
    }
}

// init_net() removed: VirtIO NET is now the virtio-net Driver Cell (P06 complete).
// x86_64 networking is handled by the e1000 Driver Cell.
