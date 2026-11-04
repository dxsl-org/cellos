//! I/O APIC routing across every unit the MADT declares.
//!
//! Units are registered once on the BSP (`add_unit`) before any route is
//! programmed. Each unit owns GSIs `[gsi_base, gsi_base + pins)`. Routes use
//! physical destination mode with an 8-bit APIC ID; destinations above 255
//! need interrupt remapping (plan 261103-2218 phase 12) and are refused.

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub const MAX_UNITS: usize = 16;

static UNIT_COUNT: AtomicUsize = AtomicUsize::new(0);
static UNIT_PHYS: [AtomicU64; MAX_UNITS] = [const { AtomicU64::new(0) }; MAX_UNITS];
static UNIT_GSI_BASE: [AtomicU32; MAX_UNITS] = [const { AtomicU32::new(0) }; MAX_UNITS];
static UNIT_PINS: [AtomicU32; MAX_UNITS] = [const { AtomicU32::new(0) }; MAX_UNITS];

fn window(phys: u64) -> usize {
    super::apic::mmio_virt(phys as usize)
}

fn read(phys: u64, index: u32) -> u32 {
    let base = window(phys);
    // SAFETY: the unit's page is identity-mapped before `add_unit`.
    unsafe {
        core::ptr::write_volatile(base as *mut u32, index);
        core::ptr::read_volatile((base + 0x10) as *const u32)
    }
}

fn write(phys: u64, index: u32, value: u32) {
    let base = window(phys);
    // SAFETY: the unit's page is identity-mapped before `add_unit`.
    unsafe {
        core::ptr::write_volatile(base as *mut u32, index);
        core::ptr::write_volatile((base + 0x10) as *mut u32, value);
    }
}

/// Register an I/O APIC whose MMIO page is already mapped. Reads its
/// redirection-entry count from IOAPICVER. `false` when full or unusable.
pub fn add_unit(phys: u64, gsi_base: u32) -> bool {
    let index = UNIT_COUNT.load(Ordering::Acquire);
    if phys == 0 || index >= MAX_UNITS {
        return false;
    }
    let version = read(phys, 1);
    if version == u32::MAX {
        return false;
    }
    let pins = ((version >> 16) & 0xFF) + 1;
    UNIT_PHYS[index].store(phys, Ordering::Relaxed);
    UNIT_GSI_BASE[index].store(gsi_base, Ordering::Relaxed);
    UNIT_PINS[index].store(pins, Ordering::Relaxed);
    UNIT_COUNT.store(index + 1, Ordering::Release);
    true
}

pub fn unit_count() -> usize {
    UNIT_COUNT.load(Ordering::Acquire)
}

/// `(unit index, pin)` serving `gsi` among `(gsi_base, pins)` units.
pub(super) fn locate(units: &[(u32, u32)], gsi: u32) -> Option<(usize, u32)> {
    units.iter().enumerate().find_map(|(index, &(base, pins))| {
        let pin = gsi.checked_sub(base)?;
        (pin < pins).then_some((index, pin))
    })
}

/// Low dword of a redirection entry for `vector` with MPS INTI `flags`
/// (polarity bits 0-1, trigger bits 2-3; `00` = bus default, which is
/// active-high edge for ISA). Unmasked, fixed delivery, physical mode.
pub(super) fn rte_low(vector: u8, flags: u16) -> u32 {
    let active_low = flags & 0b11 == 0b11;
    let level = (flags >> 2) & 0b11 == 0b11;
    vector as u32 | (active_low as u32) << 13 | (level as u32) << 15
}

/// Route `gsi` to `vector` on the CPU with APIC ID `destination`.
pub fn route_gsi(gsi: u32, vector: u8, destination: u32, flags: u16) -> bool {
    if destination > 0xFF {
        return false;
    }
    let count = unit_count();
    let mut units = [(0u32, 0u32); MAX_UNITS];
    for (index, slot) in units.iter_mut().enumerate().take(count) {
        *slot = (
            UNIT_GSI_BASE[index].load(Ordering::Relaxed),
            UNIT_PINS[index].load(Ordering::Relaxed),
        );
    }
    let Some((unit, pin)) = locate(&units[..count], gsi) else {
        return false;
    };
    let phys = UNIT_PHYS[unit].load(Ordering::Relaxed);
    // High dword first: the entry is still masked or unchanged until the low
    // dword (with the vector) is written.
    write(phys, 0x10 + pin * 2 + 1, destination << 24);
    write(phys, 0x10 + pin * 2, rte_low(vector, flags));
    true
}

#[cfg(test)]
mod tests {
    use super::{locate, rte_low};

    #[test]
    fn gsis_resolve_to_the_owning_unit_and_pin() {
        let units = [(0, 24), (24, 24)];
        assert_eq!(locate(&units, 4), Some((0, 4)));
        assert_eq!(locate(&units, 24), Some((1, 0)));
        assert_eq!(locate(&units, 47), Some((1, 23)));
        assert_eq!(locate(&units, 48), None);
    }

    #[test]
    fn inti_flags_select_polarity_and_trigger() {
        assert_eq!(rte_low(0x24, 0), 0x24); // ISA default: high, edge
        assert_eq!(rte_low(0x30, 0b1111), 0x30 | 1 << 13 | 1 << 15); // low, level
        assert_eq!(rte_low(0x30, 0b0101), 0x30); // explicit high, edge
    }
}
