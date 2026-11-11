//! Kernel-side refusal of PCI functions whose DMA would bypass VT-d.
//!
//! A virtio device that does not offer `VIRTIO_F_ACCESS_PLATFORM` (feature bit
//! 33) uses guest-physical addresses directly: under QEMU it skips the vIOMMU.
//! QEMU's VMware PVSCSI model (`15ad:07c0`) reads request descriptors and
//! writes completion descriptors and sense by physical address too (phase-09
//! D5). The cells refuse such a device themselves where they can, but any cell
//! holding `PcieDriverCap` could claim it through 418/424 and DMA anywhere. So
//! while VT-d is active the kernel refuses to hand such a function out at all.
//! VMware's "Virtualize IOMMU" never activates VT-d (its unit reports an
//! all-ones `GCAP`, `iommu_x86`), so VMware guests keep their PVSCSI disk.

use super::config::Config;
use crate::dev::pcie_ecam::PciDevice;
use crate::sync::Spinlock;
use alloc::vec::Vec;

const VIRTIO_VENDOR: u16 = 0x1AF4;
/// VMware PVSCSI: always bypasses (QEMU's model; no feature to negotiate).
const PVSCSI: (u16, u16) = (0x15AD, 0x07C0);
const VIRTIO_BYPASS: &str = "virtio without VIRTIO_F_ACCESS_PLATFORM would bypass VT-d";
const PVSCSI_BYPASS: &str = "PVSCSI rings are accessed by physical address and would bypass VT-d";
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
/// Judged only while VT-d translates: PVSCSI always bypasses; a virtio function
/// bypasses unless it offers `ACCESS_PLATFORM`, and one without a usable common
/// configuration (legacy-only, or a BAR the scan did not size) counts as
/// bypassing: legacy virtio has no `ACCESS_PLATFORM`. Call only for a function
/// no task owns: the first virtio probe maps the common-config page
/// kernel-only and writes `device_feature_select`. A refusal is cached per BDF
/// (a device's offer is fixed for the boot) and logged once, so a driver
/// retrying discovery neither re-probes nor floods the console.
pub fn dma_bypasses_iommu(dev: &PciDevice) -> bool {
    if !crate::dev::iommu::is_active() {
        return false;
    }
    let bdf = dev.canonical_bdf();
    if REFUSED.lock().contains(&bdf) {
        return true;
    }
    let Some(reason) = bypass_reason(dev.vendor_id, dev.device_id, true, || probe_bypasses(dev))
    else {
        return false;
    };
    REFUSED.lock().push(bdf);
    log::warn!(
        "[pcie] refusing {:02x}:{:02x}.{} {:04x}:{:04x}: {}",
        bdf >> 8,
        (bdf >> 3) & 0x1F,
        bdf & 7,
        dev.vendor_id,
        dev.device_id,
        reason
    );
    true
}

/// Why a `(vendor_id, device_id)` function would bypass VT-d, if it would.
/// `virtio_bypasses` probes a virtio function's feature offer; it runs only
/// for a virtio function while VT-d is active.
fn bypass_reason(
    vendor_id: u16,
    device_id: u16,
    iommu_active: bool,
    virtio_bypasses: impl FnOnce() -> bool,
) -> Option<&'static str> {
    if !iommu_active {
        None
    } else if (vendor_id, device_id) == PVSCSI {
        Some(PVSCSI_BYPASS)
    } else if vendor_id == VIRTIO_VENDOR && virtio_bypasses() {
        Some(VIRTIO_BYPASS)
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pvscsi_and_bypassing_virtio_are_refused_only_under_vtd() {
        let count = core::cell::Cell::new(0);
        let probed = &count;
        let probe = |bypasses| {
            move || {
                probed.set(probed.get() + 1);
                bypasses
            }
        };
        // PVSCSI: refused under VT-d without a probe, offered without VT-d.
        assert_eq!(
            bypass_reason(0x15AD, 0x07C0, true, probe(false)),
            Some(PVSCSI_BYPASS)
        );
        assert_eq!(bypass_reason(0x15AD, 0x07C0, false, probe(true)), None);
        // vmxnet3 shares the vendor but translates its DMA.
        assert_eq!(bypass_reason(0x15AD, 0x07B0, true, probe(true)), None);
        assert_eq!(probed.get(), 0, "only virtio is probed");
        // virtio follows its feature offer.
        assert_eq!(
            bypass_reason(0x1AF4, 0x1041, true, probe(true)),
            Some(VIRTIO_BYPASS)
        );
        assert_eq!(bypass_reason(0x1AF4, 0x1041, true, probe(false)), None);
        assert_eq!(probed.get(), 2);
        assert_eq!(bypass_reason(0x1AF4, 0x1041, false, probe(true)), None);
        assert_eq!(probed.get(), 2, "no probe without VT-d");
        // Anything else is never judged.
        assert_eq!(bypass_reason(0x8086, 0x10D3, true, probe(true)), None);
        assert_eq!(probed.get(), 2);
    }
}
