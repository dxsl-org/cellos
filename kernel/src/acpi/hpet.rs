// SPDX-License-Identifier: MPL-2.0
//! HPET table parser.

use super::AcpiInfo;

// ---------------------------------------------------------------------------
// HPET parser
// ---------------------------------------------------------------------------

/// Parse HPET table for event timer block MMIO address.
///
/// HPET table layout (after 36-byte SDT header):
///   offset 36: event_timer_block_id (u32) — hardware rev, comparators, etc.
///   offset 40: base_address (GAS, 12 bytes)
///       GAS[0]:  address_space_id (u8) — 0 = MMIO
///       GAS[4..12]: address (u64)
///   offset 52: hpet_number (u8)
///   offset 53: minimum_tick (u16)
///   offset 55: page_protection (u8)
pub(super) fn parse_hpet(virt: usize, length: usize, info: &mut AcpiInfo) {
    // GAS starts at offset 40; address field is at GAS offset 4 → table offset 44.
    if length < 56 {
        log::warn!("[acpi] HPET table too short ({} bytes)", length);
        return;
    }

    // GAS address_space_id at offset 40.
    // SAFETY: offset 40 is within the validated HPET body.
    let addr_space = unsafe { core::ptr::read_volatile((virt + 40) as *const u8) };
    if addr_space != 0 {
        // Non-zero address space means I/O ports or PCI config — not simple MMIO.
        log::warn!(
            "[acpi] HPET GAS address_space_id={} (not MMIO) — timer gate closed",
            addr_space
        );
        return;
    }

    // GAS address at GAS offset +4 → table offset 44.
    // SAFETY: offset 44 is within the validated HPET body.
    let hpet_addr = unsafe { core::ptr::read_unaligned((virt + 44) as *const u64) };
    if super::mmio_base_admissible(hpet_addr) {
        info.hpet_base = hpet_addr;
        log::info!("[acpi] HPET: event timer block = {:#x}", hpet_addr);
    } else {
        log::warn!(
            "[acpi] HPET: GAS address {:#x} is zero, unaligned or beyond 52 bits — timer gate closed",
            hpet_addr
        );
    }
}
