//! Builds the `QueryPcieFunction` (434) record from the scan and config space.

use super::config::{
    Config, CAP_ID_MSI, CAP_ID_MSIX, CFG_BAR0, CFG_HEADER_TYPE, CFG_REVISION, CFG_SUBSYSTEM_ID,
    CFG_SUBSYSTEM_VENDOR,
};
use super::layout::MsixLayout;
use crate::dev::pcie_ecam::{Bar, PciDevice};
use api::pcie_function::{
    PcieBar, PcieCap, ViPcieFunctionV1, BAR_FLAG_MSIX_CARVED, BAR_FLAG_PREFETCHABLE,
    BAR_KIND_ABSENT, BAR_KIND_IO, BAR_KIND_MEM32, BAR_KIND_MEM64, FLAG_DMA_REMAPPED,
    FLAG_MSIX_PRESENT, FLAG_MSI_FORBIDDEN_BY_FIRMWARE, FLAG_MSI_PRESENT, PCIE_CAP_BYTES,
    PCIE_FUNCTION_CAPS_MAX,
};

/// Vendor-specific capability; its own length byte bounds the copy.
const CAP_ID_VENDOR: u8 = 0x09;

/// Size of memory BAR `index` from the scan; 0 for absent, I/O or upper halves.
pub fn bar_len(dev: &PciDevice, index: u8) -> u64 {
    match dev.bars.get(index as usize) {
        Some(Bar::Memory32 { addr, size }) if *addr != 0 => *size as u64,
        Some(Bar::Memory64 { addr, size }) if *addr != 0 => *size,
        _ => 0,
    }
}

/// MSI-X capability offset and decoded layout, if the function has one.
pub fn msix_layout(cfg: &Config) -> Option<(u8, MsixLayout)> {
    let at = cfg.find_capability(CAP_ID_MSIX)?;
    let base = at as usize;
    let layout = MsixLayout::from_registers(
        cfg.read16(base + 2)?,
        cfg.read32(base + 4)?,
        cfg.read32(base + 8)?,
    );
    Some((at, layout))
}

/// MSI-X is carved (and offered) only for a layout that passes the page rule.
pub fn carved_layout(dev: &PciDevice, cfg: &Config) -> Option<(u8, MsixLayout)> {
    msix_layout(cfg).filter(|(_, layout)| layout.carve_safe(|i| bar_len(dev, i)))
}

fn bar_entry(dev: &PciDevice, cfg: &Config, index: usize) -> PcieBar {
    let raw = cfg.read32(CFG_BAR0 + index * 4).unwrap_or(0);
    let (kind, base, len) = match dev.bars[index] {
        Bar::Memory32 { addr, size } if addr != 0 => (BAR_KIND_MEM32, addr as u64, size as u64),
        Bar::Memory64 { addr, size } if addr != 0 => (BAR_KIND_MEM64, addr, size),
        // I/O BARs are not sized by the scan; the base is reported, the length is 0.
        Bar::Io => (BAR_KIND_IO, (raw & !3) as u64, 0),
        _ => return PcieBar::default(),
    };
    let prefetchable = kind != BAR_KIND_IO && raw & (1 << 3) != 0;
    PcieBar {
        base,
        len,
        kind,
        bar_flags: if prefetchable {
            BAR_FLAG_PREFETCHABLE
        } else {
            0
        },
    }
}

fn cap_entry(cfg: &Config, id: u8, at: u8) -> PcieCap {
    let room = (0x100 - at as usize).min(PCIE_CAP_BYTES);
    let len = if id == CAP_ID_VENDOR {
        (cfg.read8(at as usize + 2).unwrap_or(0) as usize).min(room)
    } else {
        room
    };
    let mut bytes = [0u8; PCIE_CAP_BYTES];
    for (i, byte) in bytes[..len].iter_mut().enumerate() {
        *byte = cfg.read8(at as usize + i).unwrap_or(0);
    }
    PcieCap {
        id,
        offset: at,
        len: len as u8,
        bytes,
    }
}

/// The v1 record for `dev`, or `None` when the function no longer answers.
pub fn build(dev: &PciDevice, firmware_forbids_msi: bool) -> Option<ViPcieFunctionV1> {
    let bdf = dev.canonical_bdf();
    let cfg = Config::new(bdf);
    let ids = cfg.read32(0)?;
    if ids & 0xFFFF == 0xFFFF {
        return None;
    }
    let type0 = cfg.read8(CFG_HEADER_TYPE)? & 0x7F == 0;
    let mut rec = ViPcieFunctionV1 {
        bdf,
        vendor_id: dev.vendor_id,
        device_id: dev.device_id,
        class: dev.class,
        subclass: dev.subclass,
        prog_if: dev.prog_if,
        revision: cfg.read8(CFG_REVISION)?,
        ..ViPcieFunctionV1::default()
    };
    if type0 {
        rec.subsystem_vendor_id = cfg.read16(CFG_SUBSYSTEM_VENDOR)?;
        rec.subsystem_id = cfg.read16(CFG_SUBSYSTEM_ID)?;
        for index in 0..rec.bars.len() {
            rec.bars[index] = bar_entry(dev, &cfg, index);
        }
    }
    if firmware_forbids_msi {
        rec.flags |= FLAG_MSI_FORBIDDEN_BY_FIRMWARE;
    }
    // Set while the kernel's VT-d translation is active. Until phase 12 programs
    // every DRHD, a function under an unprogrammed unit also gets the bit: that
    // errs toward requiring ACCESS_PLATFORM and is not proof of confinement.
    if crate::dev::iommu::is_active() {
        rec.flags |= FLAG_DMA_REMAPPED;
    }
    if let Some((_, layout)) = msix_layout(&cfg) {
        rec.flags |= FLAG_MSIX_PRESENT;
        rec.msix.table_size = layout.table_size;
        rec.msix.table_bar = layout.table_bar;
        rec.msix.pba_bar = layout.pba_bar;
        rec.msix.table_offset = layout.table_offset;
        rec.msix.pba_offset = layout.pba_offset;
    }
    if let Some((_, layout)) = carved_layout(dev, &cfg) {
        for (index, bar) in rec.bars.iter_mut().enumerate() {
            if layout.holds(index as u8) && bar.kind != BAR_KIND_ABSENT {
                bar.bar_flags |= BAR_FLAG_MSIX_CARVED;
            }
        }
    }
    let caps = cfg.capabilities();
    if let Some(&(_, at)) = caps.iter().find(|(id, _)| *id == CAP_ID_MSI) {
        rec.flags |= FLAG_MSI_PRESENT;
        rec.msi_control = cfg.read16(at as usize + 2)?;
    }
    for (slot, &(id, at)) in caps.iter().take(PCIE_FUNCTION_CAPS_MAX).enumerate() {
        rec.caps[slot] = cap_entry(&cfg, id, at);
        rec.cap_count += 1;
    }
    Some(rec)
}
