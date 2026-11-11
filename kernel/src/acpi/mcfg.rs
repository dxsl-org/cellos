// SPDX-License-Identifier: MPL-2.0
//! MCFG (PCIe ECAM) parser.

use super::bytes::TableBytes;
use super::records::{AcpiTopology, McfgAlloc};
use super::AcpiInfo;

// ---------------------------------------------------------------------------
// MCFG parser
// ---------------------------------------------------------------------------

/// Parse MCFG (Memory Mapped Configuration Space) table.
///
/// Selects the first valid allocation for segment 0. MCFG stores a segment's
/// bus-0-relative ECAM base; the admitted window starts at `base + bus_start×1 MiB`.
/// MCFG body layout (after 36-byte SDT header):
///   - 8 bytes reserved
///   - then N × 16-byte allocation entries:
///     base_address (u64), segment (u16), bus_start (u8), bus_end (u8), _reserved (u32)
pub(super) fn parse_mcfg(virt: usize, length: usize, info: &mut AcpiInfo, topo: &mut AcpiTopology) {
    // First allocation entry starts at offset 44 (36-byte header + 8-byte reserved).
    if length < 44 + 16 {
        log::warn!(
            "[acpi] MCFG too short ({} bytes) for any allocation entry",
            length
        );
        return;
    }

    let entry_count = (length - 44) / 16;
    // SAFETY: the caller validated and mapped `[virt, virt + length)`.
    let table = unsafe { TableBytes::new(virt, length) };
    for index in 0..entry_count {
        let at = 44 + index * 16;
        topo.mcfg.push(McfgAlloc {
            base: table.u64(at).unwrap_or(0),
            segment: table.u16(at + 8).unwrap_or(0),
            bus_start: table.u8(at + 10).unwrap_or(0),
            bus_end: table.u8(at + 11).unwrap_or(0),
        });
    }
    for index in 0..entry_count {
        let entry = virt + 44 + index * 16;
        // SAFETY: every field is within this validated 16-byte allocation entry.
        let base_addr = unsafe { core::ptr::read_unaligned(entry as *const u64) };
        let segment = unsafe { core::ptr::read_unaligned((entry + 8) as *const u16) };
        let bus_start = unsafe { core::ptr::read_volatile((entry + 10) as *const u8) };
        let bus_end = unsafe { core::ptr::read_volatile((entry + 11) as *const u8) };
        let window_len = crate::dev::pcie_ecam::ecam_window_size(bus_start, bus_end);
        let window_base = base_addr.checked_add((bus_start as u64) << 20);
        if segment == 0
            && base_addr != 0
            && base_addr & 0xF_FFFF == 0
            && window_base
                .zip(window_len)
                .and_then(|(base, len)| base.checked_add(len as u64))
                .is_some()
        {
            let window_base = window_base.expect("validated MCFG window base");
            info.ecam_base = window_base;
            info.ecam_bus_start = bus_start;
            info.ecam_bus_end = bus_end;
            log::info!(
                "[acpi] MCFG: segment 0 bus {}-{} ECAM window base = {:#x}",
                bus_start,
                bus_end,
                window_base
            );
            return;
        }
    }
    log::warn!("[acpi] MCFG has no valid segment 0 allocation — PCIe gate closed");
}
