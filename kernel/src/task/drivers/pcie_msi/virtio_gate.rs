//! Kernel-side refusal of virtio functions whose DMA would bypass VT-d.
//!
//! A virtio device that does not offer `VIRTIO_F_ACCESS_PLATFORM` (feature bit
//! 33) uses guest-physical addresses directly: under QEMU it skips the vIOMMU.
//! The virtio cells refuse such a device themselves, but any cell holding
//! `PcieDriverCap` could claim it through 418/424 and DMA anywhere. So while
//! VT-d is active the kernel refuses to hand such a function out at all.

use super::config::Config;
use crate::sync::Spinlock;
use crate::task::drivers::pcie_ecam::PciDevice;
use alloc::vec::Vec;

const VIRTIO_VENDOR: u16 = 0x1AF4;
const CAP_ID_VENDOR: u8 = 0x09;
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
/// Common configuration: `device_feature_select` at 0, `device_feature` at 4.
const DEVICE_FEATURE_SELECT: usize = 0x00;
const DEVICE_FEATURE: usize = 0x04;
/// `VIRTIO_F_ACCESS_PLATFORM` is bit 1 of feature word 1.
const ACCESS_PLATFORM_HIGH_BIT: u32 = 1 << 1;
const CMD_MEM_SPACE: u16 = 1 << 1;

/// Location of the common configuration structure: `(bar, offset)`.
fn common_cfg(cfg: &Config) -> Option<(u8, u32)> {
    cfg.capabilities().into_iter().find_map(|(id, at)| {
        let base = at as usize;
        if id != CAP_ID_VENDOR || cfg.read8(base + 3)? != VIRTIO_PCI_CAP_COMMON_CFG {
            return None;
        }
        Some((cfg.read8(base + 4)?, cfg.read32(base + 8)?))
    })
}

/// Whether handing `dev` to a driver would let its DMA bypass an active VT-d.
///
/// Only virtio functions are judged, and only while VT-d translates. A virtio
/// function without a usable common configuration (legacy-only, or a BAR the
/// scan did not size) counts as bypassing: legacy virtio has no
/// `ACCESS_PLATFORM`. Call only for a function no task owns: the first probe
/// maps the common-config page kernel-only and writes `device_feature_select`.
/// A refusal is cached per BDF (a device's offer is fixed for the boot) and
/// logged once, so a driver retrying discovery neither re-probes nor floods the
/// console.
pub fn dma_bypasses_iommu(dev: &PciDevice) -> bool {
    if dev.vendor_id != VIRTIO_VENDOR || !crate::task::drivers::iommu::is_active() {
        return false;
    }
    let bdf = dev.canonical_bdf();
    if REFUSED.lock().contains(&bdf) {
        return true;
    }
    if !probe_bypasses(dev) {
        return false;
    }
    REFUSED.lock().push(bdf);
    log::warn!(
        "[pcie] refusing {:02x}:{:02x}.{} {:04x}:{:04x}: virtio without VIRTIO_F_ACCESS_PLATFORM would bypass VT-d",
        bdf >> 8,
        (bdf >> 3) & 0x1F,
        bdf & 7,
        dev.vendor_id,
        dev.device_id
    );
    true
}

static REFUSED: Spinlock<Vec<u32>> = Spinlock::new(Vec::new());

fn probe_bypasses(dev: &PciDevice) -> bool {
    let cfg = Config::new(dev.canonical_bdf());
    let Some((bar, offset)) = common_cfg(&cfg) else {
        return true;
    };
    let base = dev.bars.get(bar as usize).map_or(0, |b| b.base_addr());
    let len = super::record::bar_len(dev, bar);
    if base == 0 || (offset as u64).saturating_add(8) > len {
        return true;
    }
    // Memory decoding only; bus mastering stays as it is (off for an unowned function).
    if let Some(command) = cfg.read16(0x04) {
        let _ = cfg.write16(0x04, command | CMD_MEM_SPACE);
    }
    let common = (base + offset as u64) as usize;
    crate::memory::paging::map_mmio_x86(common, 8);
    // SAFETY: `common` lies inside the function's memory BAR (checked above)
    // and was just identity-mapped kernel-only.
    let high = unsafe {
        core::ptr::write_volatile((common + DEVICE_FEATURE_SELECT) as *mut u32, 1);
        core::ptr::read_volatile((common + DEVICE_FEATURE) as *const u32)
    };
    high == u32::MAX || high & ACCESS_PLATFORM_HIGH_BIT == 0
}
