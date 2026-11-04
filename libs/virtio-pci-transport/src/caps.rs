// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Virtio structure capabilities (virtio 1.2 §4.1.4) decoded from the
//! capability bytes the kernel copies into the 434 record.
//!
//! Each vendor capability (`id` 0x09) starts with the standard header, so
//! `bytes` holds `cap_vndr`, `cap_next`, `cap_len`, `cfg_type` (3), `bar` (4),
//! `id`, padding, `offset` (8, u32 LE), `length` (12, u32 LE) and, for the
//! notify structure only, `notify_off_multiplier` (16, u32 LE). The kernel
//! bounds `len` by the capability's own `cap_len`, so `len` is the structure
//! size the device advertised (capped at 20).

use crate::error::TransportError;
use crate::regs::COMMON_CFG_LEN;
use api::pcie_function::{
    PcieCap, ViPcieFunctionV1, BAR_KIND_MEM32, BAR_KIND_MEM64, PCIE_FUNCTION_BARS,
    PCIE_FUNCTION_CAPS_MAX,
};

/// PCI capability ID of a vendor-specific capability.
pub const PCI_CAP_ID_VNDR: u8 = 0x09;

/// `sizeof(struct virtio_pci_cap)`.
const VIRTIO_CAP_LEN: u8 = 16;
/// `sizeof(struct virtio_pci_notify_cap)`.
const NOTIFY_CAP_LEN: u8 = 20;

/// Virtio structure types this transport uses (`cfg_type` 1–4). Type 5
/// (PCI configuration access) and vendor types are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CfgType {
    /// `VIRTIO_PCI_CAP_COMMON_CFG`.
    Common = 1,
    /// `VIRTIO_PCI_CAP_NOTIFY_CFG`.
    Notify = 2,
    /// `VIRTIO_PCI_CAP_ISR_CFG`.
    Isr = 3,
    /// `VIRTIO_PCI_CAP_DEVICE_CFG`.
    Device = 4,
}

impl CfgType {
    fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::Common),
            2 => Some(Self::Notify),
            3 => Some(Self::Isr),
            4 => Some(Self::Device),
            _ => None,
        }
    }

    /// Bytes the transport accesses inside the structure; a shorter region
    /// would put register accesses outside what the device decodes.
    fn min_len(self) -> u32 {
        match self {
            Self::Common => COMMON_CFG_LEN as u32,
            Self::Notify => 2,
            Self::Isr => 1,
            Self::Device => 0,
        }
    }
}

/// One virtio structure: `length` bytes at `offset` inside BAR `bar`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapRegion {
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

/// The structures a modern virtio PCI function exposes, each validated to lie
/// inside an allocated memory BAR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtioCaps {
    pub common: CapRegion,
    pub notify: CapRegion,
    /// Bytes between consecutive `queue_notify_off` units; 0 means every
    /// queue shares one notify address. Always even.
    pub notify_off_multiplier: u32,
    pub isr: CapRegion,
    /// Optional: some device types (e.g. entropy) have no device config.
    pub device: Option<CapRegion>,
}

fn le32(cap: &PcieCap, at: usize) -> u32 {
    u32::from_le_bytes([
        cap.bytes[at],
        cap.bytes[at + 1],
        cap.bytes[at + 2],
        cap.bytes[at + 3],
    ])
}

/// Decodes a capability the transport understands. The spec has drivers skip
/// structures they cannot use rather than fail: truncated capabilities and
/// reserved BAR indices (6..=255) are therefore ignored, not errors.
fn understood(cap: &PcieCap) -> Option<(CfgType, CapRegion, u32)> {
    if cap.id != PCI_CAP_ID_VNDR || cap.len < VIRTIO_CAP_LEN {
        return None;
    }
    let kind = CfgType::from_raw(cap.bytes[3])?;
    let bar = cap.bytes[4];
    if bar as usize >= PCIE_FUNCTION_BARS {
        return None;
    }
    let multiplier = match kind {
        CfgType::Notify if cap.len < NOTIFY_CAP_LEN => return None,
        CfgType::Notify => le32(cap, 16),
        _ => 0,
    };
    let region = CapRegion {
        bar,
        offset: le32(cap, 8),
        length: le32(cap, 12),
    };
    Some((kind, region, multiplier))
}

fn validate(
    record: &ViPcieFunctionV1,
    kind: CfgType,
    region: CapRegion,
) -> Result<CapRegion, TransportError> {
    let bar = &record.bars[region.bar as usize];
    if bar.kind != BAR_KIND_MEM32 && bar.kind != BAR_KIND_MEM64 {
        return Err(TransportError::NotMemoryBar(region.bar));
    }
    if bar.base == 0 {
        return Err(TransportError::BarNotAllocated(region.bar));
    }
    if u64::from(region.offset) + u64::from(region.length) > bar.len {
        return Err(TransportError::RegionOutOfBar(kind));
    }
    if region.length < kind.min_len() {
        return Err(TransportError::RegionTooSmall(kind));
    }
    Ok(region)
}

/// Finds and validates the common, notify, ISR and (optional) device
/// structures of `record`.
///
/// The first understood capability of each type wins, as the spec directs;
/// a later duplicate is never consulted, even when the first fails validation.
///
/// # Errors
/// [`TransportError::LegacyOnly`] with no understood virtio capability,
/// [`TransportError::MissingCap`] when common/notify/ISR is absent,
/// [`TransportError::InvalidNotifyOffMultiplier`] for an odd multiplier, and
/// the BAR errors of region validation (`NotMemoryBar`, `BarNotAllocated`,
/// `RegionOutOfBar`, `RegionTooSmall`).
pub fn parse_caps(record: &ViPcieFunctionV1) -> Result<VirtioCaps, TransportError> {
    let count = (record.cap_count as usize).min(PCIE_FUNCTION_CAPS_MAX);
    let mut found: [Option<(CapRegion, u32)>; 4] = [None; 4];
    for (kind, region, multiplier) in record.caps[..count].iter().filter_map(understood) {
        found[kind as usize - 1].get_or_insert((region, multiplier));
    }
    if found.iter().all(Option::is_none) {
        return Err(TransportError::LegacyOnly);
    }
    let take = |kind: CfgType| found[kind as usize - 1];
    let required = |kind: CfgType| -> Result<CapRegion, TransportError> {
        let (region, _) = take(kind).ok_or(TransportError::MissingCap(kind))?;
        validate(record, kind, region)
    };
    let common = required(CfgType::Common)?;
    let notify = required(CfgType::Notify)?;
    let isr = required(CfgType::Isr)?;
    let notify_off_multiplier = take(CfgType::Notify).map_or(0, |(_, m)| m);
    if notify_off_multiplier % 2 != 0 {
        return Err(TransportError::InvalidNotifyOffMultiplier(
            notify_off_multiplier,
        ));
    }
    let device = match take(CfgType::Device) {
        Some((region, _)) => Some(validate(record, CfgType::Device, region)?),
        None => None,
    };
    Ok(VirtioCaps {
        common,
        notify,
        notify_off_multiplier,
        isr,
        device,
    })
}
