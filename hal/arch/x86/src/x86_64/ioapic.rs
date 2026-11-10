//! I/O APIC routing across every unit the MADT declares.
//!
//! Units are registered once on the BSP (`add_unit`) before any route is
//! programmed. Each unit owns GSIs `[gsi_base, gsi_base + pins)`. Routes use
//! physical destination mode with an 8-bit APIC ID; destinations above 255
//! need x2APIC-format interrupt remapping and are refused.
//!
//! Every programmed route is recorded so the kernel can re-emit it once
//! interrupt remapping turns on: compatibility-format entries are blocked by a
//! remapping unit (`GCMD.CFI` stays clear), so each route must be rewritten in
//! remappable format through the kernel's [`Remapper`].

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

pub const MAX_UNITS: usize = 16;
/// Routes remembered for re-emission. A route that cannot be recorded is
/// refused: it could not follow a later switch to remappable format.
pub const MAX_ROUTES: usize = 64;

static UNIT_COUNT: AtomicUsize = AtomicUsize::new(0);
static UNIT_PHYS: [AtomicU64; MAX_UNITS] = [const { AtomicU64::new(0) }; MAX_UNITS];
static UNIT_GSI_BASE: [AtomicU32; MAX_UNITS] = [const { AtomicU32::new(0) }; MAX_UNITS];
static UNIT_PINS: [AtomicU32; MAX_UNITS] = [const { AtomicU32::new(0) }; MAX_UNITS];
/// Packed [`Route`] records (see `pack`); 0 is a free slot.
static ROUTES: [AtomicU64; MAX_ROUTES] = [const { AtomicU64::new(0) }; MAX_ROUTES];
/// `fn(&Route) -> Option<u64>` stored as an address; 0 = compatibility format.
static REMAPPER: AtomicUsize = AtomicUsize::new(0);

/// One programmed redirection, with everything needed to re-emit its entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// MMIO base of the owning I/O APIC (identifies it to ACPI MADT/DMAR).
    pub ioapic_phys: u64,
    pub pin: u8,
    pub vector: u8,
    /// Physical APIC ID of the target CPU.
    pub destination: u8,
    /// MPS INTI flags: polarity bits 0-1, trigger bits 2-3.
    pub flags: u16,
}

impl Route {
    pub fn active_low(&self) -> bool {
        self.flags & 0b11 == 0b11
    }

    pub fn level_triggered(&self) -> bool {
        (self.flags >> 2) & 0b11 == 0b11
    }
}

/// Builds the redirection entry for a route while interrupt remapping is on;
/// `None` refuses the route (no interrupt-remapping entry could be installed).
pub type Remapper = fn(&Route) -> Option<u64>;

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

/// Program redirection entry `pin` of the unit at `phys`. High dword first:
/// the vector and format-independent low fields land last, and the
/// intermediate entry (new high, old low) still names a valid target.
fn write_entry(phys: u64, pin: u8, entry: u64) {
    let index = 0x10 + pin as u32 * 2;
    write(phys, index + 1, (entry >> 32) as u32);
    write(phys, index, entry as u32);
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

/// MMIO base of registered unit `index`.
pub fn unit_phys(index: usize) -> Option<u64> {
    (index < unit_count()).then(|| UNIT_PHYS[index].load(Ordering::Relaxed))
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

/// Compatibility-format entry for `route`: destination in bits 63:56.
pub fn compat_entry(route: &Route) -> u64 {
    (route.destination as u64) << 56 | rte_low(route.vector, route.flags) as u64
}

/// Record layout: valid 63, unit 59:56, pin 55:48, vector 47:40,
/// destination 39:32, flags 15:0.
fn pack(unit: usize, route: &Route) -> u64 {
    1 << 63
        | (unit as u64 & 0xF) << 56
        | (route.pin as u64) << 48
        | (route.vector as u64) << 40
        | (route.destination as u64) << 32
        | route.flags as u64
}

fn unpack(record: u64) -> Option<(usize, Route)> {
    if record >> 63 == 0 {
        return None;
    }
    let unit = ((record >> 56) & 0xF) as usize;
    Some((
        unit,
        Route {
            ioapic_phys: unit_phys(unit)?,
            pin: (record >> 48) as u8,
            vector: (record >> 40) as u8,
            destination: (record >> 32) as u8,
            flags: record as u16,
        },
    ))
}

/// Slot holding `(unit, pin)`, else the first free slot.
fn route_slot(unit: usize, pin: u8) -> Option<&'static AtomicU64> {
    let same = |record: u64| unpack(record).is_some_and(|(u, r)| u == unit && r.pin == pin);
    ROUTES
        .iter()
        .find(|slot| same(slot.load(Ordering::Acquire)))
        .or_else(|| ROUTES.iter().find(|slot| slot.load(Ordering::Acquire) == 0))
}

fn remapper() -> Option<Remapper> {
    let raw = REMAPPER.load(Ordering::Acquire);
    // SAFETY: a non-zero value was stored from a `Remapper` by `set_remapper`.
    (raw != 0).then(|| unsafe { core::mem::transmute::<usize, Remapper>(raw) })
}

/// Entry for `route` in the current format; `None` when the remapper refuses.
fn entry_for(route: &Route) -> Option<u64> {
    match remapper() {
        Some(remap) => remap(route),
        None => Some(compat_entry(route)),
    }
}

/// Route `gsi` to `vector` on the CPU with APIC ID `destination`.
///
/// An edge-triggered route's handler must tolerate an invocation with nothing
/// pending (it polls its device's status): [`resignal_edge_routes`] delivers
/// the vector once after a format switch that could have dropped an edge.
pub fn route_gsi(gsi: u32, vector: u8, destination: u32, flags: u16) -> bool {
    let Ok(destination) = u8::try_from(destination) else {
        return false;
    };
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
    let route = Route {
        ioapic_phys: UNIT_PHYS[unit].load(Ordering::Relaxed),
        pin: pin as u8,
        vector,
        destination,
        flags,
    };
    let Some(slot) = route_slot(unit, route.pin) else {
        return false;
    };
    let Some(entry) = entry_for(&route) else {
        return false;
    };
    slot.store(pack(unit, &route), Ordering::Release);
    write_entry(route.ioapic_phys, route.pin, entry);
    true
}

/// Visit every recorded route.
pub fn for_each_route(mut visit: impl FnMut(&Route)) {
    for slot in ROUTES.iter() {
        if let Some((_, route)) = unpack(slot.load(Ordering::Acquire)) {
            visit(&route);
        }
    }
}

/// Rewrite every recorded route with the entry `build` returns. Every entry is
/// built before any is written, so a refusal leaves all entries unchanged and
/// returns `false`.
pub fn reprogram(mut build: impl FnMut(&Route) -> Option<u64>) -> bool {
    let mut entries = [None; MAX_ROUTES];
    for (slot, entry) in ROUTES.iter().zip(entries.iter_mut()) {
        if let Some((_, route)) = unpack(slot.load(Ordering::Acquire)) {
            let Some(value) = build(&route) else {
                return false;
            };
            *entry = Some((route, value));
        }
    }
    for (route, entry) in entries.into_iter().flatten() {
        write_entry(route.ioapic_phys, route.pin, entry);
    }
    true
}

/// Select how later routes are encoded: `Some` while interrupt remapping is
/// on, `None` for compatibility format. Existing routes are not touched; the
/// caller re-emits them with [`reprogram`].
pub fn set_remapper(remapper: Option<Remapper>) {
    REMAPPER.store(remapper.map_or(0, |f| f as usize), Ordering::Release);
}

/// Mask every recorded route (bit 16 of the low dword), leaving the rest of
/// each entry intact, ahead of a format switch. A level-triggered source stays
/// pending at the I/O APIC until [`reprogram`] unmasks it. An edge signalled
/// on a masked pin is discarded by the I/O APIC (QEMU's model keeps it, real
/// parts do not), and a source whose line stays asserted until serviced, such
/// as a 16550 with unread data, never edges again; the caller follows the
/// unmask with [`resignal_edge_routes`].
pub fn mask_routes() {
    for_each_route(|route| {
        let index = 0x10 + route.pin as u32 * 2;
        let low = read(route.ioapic_phys, index);
        write(route.ioapic_phys, index, low | 1 << 16);
    });
}

/// `(destination APIC ID, vector)` to re-signal for `route`: edge-triggered
/// routes only. A level-triggered pin still asserted is delivered by the I/O
/// APIC itself on unmask, so it needs none.
fn resignal_target(route: &Route) -> Option<(u32, u8)> {
    (!route.level_triggered()).then_some((route.destination as u32, route.vector))
}

/// Deliver every edge-triggered route's vector once, as a fixed IPI to its
/// destination, after [`mask_routes`] and the unmasking [`reprogram`]. The
/// handler then services whatever the device raised while its pin was masked;
/// an edge-triggered handler tolerates finding nothing (see [`route_gsi`]).
/// Returns how many were sent.
pub fn resignal_edge_routes() -> usize {
    let mut sent = 0;
    for_each_route(|route| {
        if let Some((destination, vector)) = resignal_target(route) {
            sent += super::apic::send_fixed_ipi(destination, vector) as usize;
        }
    });
    sent
}

#[cfg(test)]
mod tests {
    use super::{compat_entry, locate, resignal_target, rte_low, Route};

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

    #[test]
    fn compat_entry_places_the_destination_in_the_high_byte() {
        let route = Route {
            ioapic_phys: 0xFEC0_0000,
            pin: 9,
            vector: 0x2E,
            destination: 3,
            flags: 0b1111,
        };
        assert!(route.active_low() && route.level_triggered());
        assert_eq!(compat_entry(&route), 3u64 << 56 | 0x2E | 1 << 13 | 1 << 15);
    }

    #[test]
    fn only_edge_routes_are_resignalled() {
        let route = |flags| Route {
            ioapic_phys: 0xFEC0_0000,
            pin: 4,
            vector: 0x24,
            destination: 2,
            flags,
        };
        assert_eq!(resignal_target(&route(0)), Some((2, 0x24))); // ISA default: edge
        assert_eq!(resignal_target(&route(0b0111)), Some((2, 0x24))); // explicit edge, low
        assert_eq!(resignal_target(&route(0b1111)), None); // level, low
        assert_eq!(resignal_target(&route(0b1101)), None); // level, high
    }
}
