// SPDX-License-Identifier: MPL-2.0
//! DMAR (VT-d) parser and its records.

use super::bytes::TableBytes;
use super::records::{AcpiTopology, BoundedVec};
use super::AcpiInfo;

/// Path depth kept per device scope. A deeper path keeps `path_len` as
/// declared, so a consumer can tell it was cut and refuse the device.
pub const SCOPE_PATH_MAX: usize = 4;
/// Device scopes kept per DRHD unit.
pub const DRHD_SCOPES_MAX: usize = 16;
/// Device scopes kept per RMRR.
pub const RMRR_SCOPES_MAX: usize = 8;

/// DMAR device scope entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeviceScope {
    /// 1 PCI endpoint, 2 PCI sub-hierarchy, 3 IOAPIC, 4 HPET, 5 ACPI device.
    pub kind: u8,
    pub enumeration_id: u8,
    pub start_bus: u8,
    /// Declared path length in (device, function) pairs.
    pub path_len: u8,
    pub path: [[u8; 2]; SCOPE_PATH_MAX],
}

impl DeviceScope {
    pub const ZERO: Self = Self {
        kind: 0,
        enumeration_id: 0,
        start_bus: 0,
        path_len: 0,
        path: [[0; 2]; SCOPE_PATH_MAX],
    };

    pub fn path_truncated(&self) -> bool {
        self.path_len as usize > SCOPE_PATH_MAX
    }
}

/// DMAR type-0 remapping hardware unit.
#[derive(Clone, Copy, Debug)]
pub struct DrhdEntry {
    /// Bit 0 = INCLUDE_PCI_ALL.
    pub flags: u8,
    pub segment: u16,
    pub register_base: u64,
    pub scopes: BoundedVec<DeviceScope, DRHD_SCOPES_MAX>,
}

impl DrhdEntry {
    pub const ZERO: Self = Self {
        flags: 0,
        segment: 0,
        register_base: 0,
        scopes: BoundedVec::new(DeviceScope::ZERO),
    };

    pub fn include_pci_all(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// DMAR type-1 reserved memory region `[base, limit]` (limit inclusive).
#[derive(Clone, Copy, Debug)]
pub struct RmrrEntry {
    pub segment: u16,
    pub base: u64,
    pub limit: u64,
    pub scopes: BoundedVec<DeviceScope, RMRR_SCOPES_MAX>,
}

impl RmrrEntry {
    pub const ZERO: Self = Self {
        segment: 0,
        base: 0,
        limit: 0,
        scopes: BoundedVec::new(DeviceScope::ZERO),
    };
}

/// Parse DMAR (DMA Remapping Reporting Table).
///
/// Layout after the 36-byte SDT header: host address width (u8) at 36, flags
/// (u8) at 37 (bit 0 INTR_REMAP, bit 1 X2APIC_OPT_OUT), records from 48.
/// Each record is `type u16, length u16, payload`.
///   - Type 0 DRHD: flags u8 @4 (bit 0 INCLUDE_PCI_ALL), segment u16 @6,
///     register_base u64 @8, device scopes from @16.
///   - Type 1 RMRR: segment u16 @6, base u64 @8, limit u64 @16, scopes from @24.
///   - Other types (ATSR, RHSA, ANDD, SATC): skipped by their own length.
///
/// Every DRHD and RMRR goes to `topo`. The legacy `dmar_*` selection does not
/// change: the first INCLUDE_PCI_ALL unit on segment 0 wins, otherwise the
/// first segment-0 unit, and units on other segments are counted only. A
/// record whose length is below 4 or runs past the table, or a DRHD shorter
/// than its fixed fields, stops the walk and keeps what was already
/// validated. A short RMRR is skipped, because the base decision never
/// depended on RMRRs.
pub(super) fn parse_dmar(virt: usize, length: usize, info: &mut AcpiInfo, topo: &mut AcpiTopology) {
    if length < 48 {
        log::warn!("[acpi] DMAR too short ({} bytes) for any unit", length);
        return;
    }
    // SAFETY: the caller validated and mapped `[virt, virt + length)`.
    let table = unsafe { TableBytes::new(virt, length) };
    topo.dmar_host_address_width = table.u8(36).unwrap_or(0);
    topo.dmar_flags = table.u8(37).unwrap_or(0);

    let mut units: u8 = 0;
    let mut selected: Option<(u64, bool)> = None;
    let mut offset = 48usize;

    while let (Some(kind), Some(len)) = (table.u16(offset), table.u16(offset + 2)) {
        let len = len as usize;
        if len < 4 || offset + len > length {
            log::warn!(
                "[acpi] DMAR record type {} has invalid length {} — stopping the walk",
                kind,
                len
            );
            break;
        }
        let end = offset + len;
        match kind {
            0 => {
                if len < 16 {
                    log::warn!("[acpi] DMAR DRHD shorter than its fixed fields — stopping");
                    break;
                }
                let mut unit = DrhdEntry::ZERO;
                unit.flags = table.u8(offset + 4).unwrap_or(0);
                unit.segment = table.u16(offset + 6).unwrap_or(0);
                unit.register_base = table.u64(offset + 8).unwrap_or(0);
                read_scopes(&table, offset + 16, end, &mut unit.scopes);
                units = units.saturating_add(1);

                if unit.segment == 0 && unit.register_base != 0 {
                    let include_all = unit.include_pci_all();
                    let better = match selected {
                        None => true,
                        // A scope-limited unit never displaces an all-inclusive one.
                        Some((_, true)) => false,
                        Some(_) => include_all,
                    };
                    if better {
                        selected = Some((unit.register_base, include_all));
                    }
                }
                topo.drhds.push(unit);
            }
            1 if len >= 24 => {
                let mut region = RmrrEntry::ZERO;
                region.segment = table.u16(offset + 6).unwrap_or(0);
                region.base = table.u64(offset + 8).unwrap_or(0);
                region.limit = table.u64(offset + 16).unwrap_or(0);
                read_scopes(&table, offset + 24, end, &mut region.scopes);
                topo.rmrrs.push(region);
            }
            1 => log::warn!("[acpi] DMAR RMRR shorter than its fixed fields — skipped"),
            _ => {}
        }
        offset = end;
    }

    info.dmar_units = units;
    match selected {
        Some((base, include_all)) => {
            info.dmar_base = base;
            info.dmar_include_pci_all = include_all;
            log::info!(
                "[acpi] DMAR: units={} selected base={:#x} include_pci_all={}",
                units,
                base,
                include_all
            );
        }
        None => {
            log::warn!(
                "[acpi] DMAR: units={} but no usable segment-0 DRHD — VT-d gate closed",
                units
            );
        }
    }
}

/// Read device scopes in `[start, end)`. A scope whose length is below 6 or
/// runs past `end` stops the walk for this record only.
fn read_scopes<const N: usize>(
    table: &TableBytes,
    start: usize,
    end: usize,
    out: &mut BoundedVec<DeviceScope, N>,
) {
    let mut at = start;
    while at + 2 <= end {
        let (Some(kind), Some(len)) = (table.u8(at), table.u8(at + 1)) else {
            break;
        };
        let len = len as usize;
        if len < 6 || at + len > end {
            log::warn!(
                "[acpi] DMAR device scope with bad length {} — stopping",
                len
            );
            break;
        }
        let pairs = (len - 6) / 2;
        let mut scope = DeviceScope {
            kind,
            enumeration_id: table.u8(at + 4).unwrap_or(0),
            start_bus: table.u8(at + 5).unwrap_or(0),
            path_len: u8::try_from(pairs).unwrap_or(u8::MAX),
            ..DeviceScope::ZERO
        };
        for (i, slot) in scope.path.iter_mut().enumerate().take(pairs) {
            *slot = [
                table.u8(at + 6 + i * 2).unwrap_or(0),
                table.u8(at + 7 + i * 2).unwrap_or(0),
            ];
        }
        if !out.push(scope) {
            log::warn!(
                "[acpi] DMAR record has more than {} device scopes — extra dropped",
                N
            );
        }
        at += len;
    }
}
