//! Interrupt-remapping bring-up and I/O APIC routing through IRTEs (Intel
//! VT-d spec §§5.1, 6.5.2.7, 10.4.29).
//!
//! With `GCMD.CFI` clear a remapping unit blocks compatibility-format
//! interrupts, so the switch runs in an order where no source ever names an
//! entry that is not valid yet:
//! 1. queued invalidation is on (only the queue can invalidate the IEC);
//! 2. per unit: allocate the table, SIRTP, install an IRTE for every I/O APIC
//!    route the kernel has programmed, invalidate the IEC globally;
//! 3. mask every recorded I/O APIC route, so nothing is emitted in the old
//!    format (a level-triggered message blocked by the unit would leave the
//!    pin's Remote IRR set forever);
//! 4. per unit: set IRE and wait for IRES;
//! 5. rewrite every route in remappable format, unmasked;
//! 6. deliver every edge-triggered route's vector once by IPI.
//!
//! Step 6 closes the masked window: the I/O APIC discards an edge on a masked
//! pin, and a source that holds its line until serviced (a 16550 with unread
//! data) would then never edge again. Level-triggered pins re-assert on
//! unmask. Leaving edge pins unmasked would not keep them deliverable: from
//! IRE until an entry is rewritten the unit blocks its compatibility-format
//! message, and a remappable entry is only decoded once IRE is set.
//!
//! No MSI exists before this runs (pcie_msi starts after activation). Any
//! failure leaves the unit, or for step 5 every unit, with IRE clear and
//! compatibility-format interrupts; step 6 runs on that path too.

use super::context::canonical_bdf;
use super::gcmd::{disable, issue_gcmd, wait_status};
use super::ir::table::IrTable;
use super::ir::{
    remappable_ioapic_entry, Irte, DMAR_IRTA_REG, VTD_GCMD_IRE, VTD_GCMD_SIRTP, VTD_GSTS_IRES,
    VTD_GSTS_IRTPS,
};
use super::rmrr::scope_target;
use super::unit::VtdUnit;
use crate::acpi::DeviceScope;
use hal::ioapic::{self, Route};

/// DMAR device-scope type of an I/O APIC.
pub const SCOPE_IOAPIC: u8 = 3;
/// IRTA.S: 2^(S+1) = 512 entries (8 KiB) for MSI blocks and I/O APIC pins.
const IR_TABLE_SIZE_POWER: u8 = 8;

/// Requester id of the device a DMAR scope names.
pub fn scope_sid(scope: &DeviceScope) -> Option<u16> {
    let (bus, dev, func) = scope_target(scope)?;
    Some(canonical_bdf(bus, dev, func))
}

/// An I/O APIC as interrupt-remapping hardware sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoapicSource {
    /// MADT I/O APIC id (the DMAR scope's enumeration id).
    pub id: u8,
    /// Index of the unit whose DRHD scope lists it.
    pub unit: usize,
    /// Requester id its messages carry.
    pub sid: u16,
}

/// The source behind the I/O APIC with MADT id `id`, from DMAR scopes.
pub fn source_for_id(units: &[VtdUnit], id: u8) -> Option<IoapicSource> {
    units.iter().enumerate().find_map(|(unit, u)| {
        let scope = u
            .scopes
            .as_slice()
            .iter()
            .find(|s| s.kind == SCOPE_IOAPIC && s.enumeration_id == id)?;
        Some(IoapicSource {
            id,
            unit,
            sid: scope_sid(scope)?,
        })
    })
}

/// The source behind the I/O APIC whose MMIO base is `phys`.
fn source_for(units: &[VtdUnit], phys: u64) -> Option<IoapicSource> {
    let id = crate::acpi::with_topology(|topo| {
        topo.ioapics
            .as_slice()
            .iter()
            .find(|a| a.address == phys)
            .map(|a| a.id)
    })?;
    source_for_id(units, id)
}

/// Redirection entry for `route`: remappable through the unit that covers
/// the I/O APIC when that unit remaps, else compatibility format. `None` when
/// no IRTE could be installed or invalidated.
fn ioapic_entry(units: &mut [VtdUnit], route: &Route) -> Option<u64> {
    let source = source_for(units, route.ioapic_phys)?;
    let unit = &mut units[source.unit];
    let Some(table) = unit.ir_table.as_mut() else {
        return Some(ioapic::compat_entry(route));
    };
    let level = route.level_triggered();
    let irte = Irte::encode(route.vector, route.destination, source.sid, level);
    let slot = table.install_ioapic(source.id, route.pin, irte)?;
    if slot.written && !unit.invalidate_iec(Some(slot.index)) {
        return None;
    }
    Some(remappable_ioapic_entry(
        slot.index,
        route.vector,
        level,
        route.active_low(),
    ))
}

/// The [`ioapic::Remapper`] installed while remapping is on.
fn remap_route(route: &Route) -> Option<u64> {
    ioapic_entry(&mut super::UNITS.lock(), route)
}

impl VtdUnit {
    /// Allocate an interrupt remapping table and latch it with SIRTP.
    fn load_ir_table(&self) -> Option<IrTable> {
        let table = IrTable::new(IR_TABLE_SIZE_POWER, self.ecap);
        // xAPIC format (EIME=0); see the `ir` module note.
        let irta = (table.phys & !0xFFF) | table.size_power as u64;
        // SAFETY: the register page is identity-mapped.
        unsafe { core::ptr::write_volatile((self.base + DMAR_IRTA_REG) as *mut u64, irta) };
        issue_gcmd(self.base, VTD_GCMD_SIRTP);
        wait_status(self.base, VTD_GSTS_IRTPS, true, "IRTPS").then_some(table)
    }

    /// Step 2 for this unit: a table holding every recorded route of the
    /// I/O APICs it covers, with the IEC invalidated.
    fn prepare_ir(&self, index: usize, sources: &[(u64, IoapicSource)]) -> Option<IrTable> {
        let mut table = self.load_ir_table()?;
        let mut installed = true;
        ioapic::for_each_route(|route| {
            let Some((_, source)) = sources.iter().find(|(p, _)| *p == route.ioapic_phys) else {
                return;
            };
            if source.unit == index {
                let level = route.level_triggered();
                let irte = Irte::encode(route.vector, route.destination, source.sid, level);
                installed &= table.install_ioapic(source.id, route.pin, irte).is_some();
            }
        });
        // SIRTP itself requires a global IEC invalidation before IRE.
        (installed && self.invalidate_iec(None)).then_some(table)
    }
}

/// Every registered I/O APIC with the unit and requester id behind it.
/// `None` when one is not listed in any DMAR scope: its compatibility-format
/// messages would be blocked by whichever unit it sits behind.
fn ioapic_sources(units: &[VtdUnit]) -> Option<alloc::vec::Vec<(u64, IoapicSource)>> {
    let mut sources = alloc::vec::Vec::new();
    for index in 0..ioapic::unit_count() {
        let phys = ioapic::unit_phys(index)?;
        let Some(source) = source_for(units, phys) else {
            log::warn!(
                "[vtd] I/O APIC {:#x} is in no DMAR scope; interrupt remapping off",
                phys
            );
            return None;
        };
        sources.push((phys, source));
    }
    Some(sources)
}

/// Steps 3-6 of the switch as hardware actions, split from the register work
/// so [`switch`]'s order is host-testable.
trait Switch {
    /// Step 3: mask every recorded I/O APIC route.
    fn mask_routes(&mut self);
    /// Step 4: set IRE on every prepared unit; `true` when at least one remaps.
    fn enable_remapping(&mut self) -> bool;
    /// Step 5: rewrite every route in remappable format, unmasked.
    fn emit_remappable(&mut self) -> bool;
    /// Clear IRE everywhere and rewrite every route in compatibility format,
    /// unmasked.
    fn fall_back(&mut self);
    /// Step 6: deliver each edge-triggered route's vector once.
    fn resignal_edges(&mut self);
}

/// Run steps 3-6; `true` when interrupt remapping is on. The re-signal comes
/// after the last write that unmasks a route, whichever format it ends in,
/// so every edge discarded while masked is replaced by one that is delivered.
fn switch(hw: &mut impl Switch) -> bool {
    hw.mask_routes();
    let remapped = hw.enable_remapping() && hw.emit_remappable();
    if !remapped {
        hw.fall_back();
    }
    hw.resignal_edges();
    remapped
}

/// [`Switch`] over the real units, with the tables `prepare_ir` built.
struct Hardware<'a> {
    units: &'a mut [VtdUnit],
    tables: alloc::vec::Vec<Option<IrTable>>,
}

impl Switch for Hardware<'_> {
    fn mask_routes(&mut self) {
        ioapic::mask_routes();
    }

    fn enable_remapping(&mut self) -> bool {
        let mut any = false;
        for (u, table) in self.units.iter_mut().zip(self.tables.iter_mut()) {
            let Some(table) = table.take() else { continue };
            issue_gcmd(u.base, VTD_GCMD_IRE);
            if wait_status(u.base, VTD_GSTS_IRES, true, "IRES") {
                u.ir_table = Some(table);
                any = true;
            } else {
                disable(u.base, VTD_GCMD_IRE, "IRES");
            }
        }
        any
    }

    fn emit_remappable(&mut self) -> bool {
        let units = &mut *self.units;
        if !ioapic::reprogram(|route| ioapic_entry(units, route)) {
            return false;
        }
        ioapic::set_remapper(Some(remap_route));
        true
    }

    fn fall_back(&mut self) {
        for u in self.units.iter_mut().filter(|u| u.ir_table.is_some()) {
            disable(u.base, VTD_GCMD_IRE, "IRES");
            u.ir_table = None;
        }
        let restored = ioapic::reprogram(|route| Some(ioapic::compat_entry(route)));
        log::warn!(
            "[vtd] interrupt remapping not enabled; I/O APIC routes restored={}",
            restored
        );
    }

    fn resignal_edges(&mut self) {
        let sent = ioapic::resignal_edge_routes();
        log::info!(
            "[vtd] re-signalled {} edge-triggered I/O APIC route(s)",
            sent
        );
    }
}

/// Enable interrupt remapping on every capable unit and move the I/O APIC
/// routes onto it. `true` when at least one unit remaps.
pub fn activate(units: &mut [VtdUnit]) -> bool {
    let capable = |u: &VtdUnit| {
        u.ir_supported
            && u.qi.lock().is_some()
            && u.active.load(core::sync::atomic::Ordering::Acquire)
    };
    if !units.iter().any(capable) {
        return false;
    }
    let Some(sources) = ioapic_sources(units) else {
        return false;
    };
    let tables: alloc::vec::Vec<Option<IrTable>> = units
        .iter()
        .enumerate()
        .map(|(index, u)| capable(u).then(|| u.prepare_ir(index, &sources)).flatten())
        .collect();
    if tables.iter().all(Option::is_none) {
        return false;
    }

    let mut hw = Hardware { units, tables };
    if !switch(&mut hw) {
        return false;
    }
    for u in hw.units.iter().filter(|u| u.ir_table.is_some()) {
        log::info!("[vtd] unit {:#x} interrupt remapping enabled", u.base);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{switch, Switch};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        Mask,
        Enable,
        Emit,
        FallBack,
        Resignal,
    }

    struct Recorder {
        enable_ok: bool,
        emit_ok: bool,
        steps: alloc::vec::Vec<Step>,
    }

    impl Switch for Recorder {
        fn mask_routes(&mut self) {
            self.steps.push(Step::Mask);
        }
        fn enable_remapping(&mut self) -> bool {
            self.steps.push(Step::Enable);
            self.enable_ok
        }
        fn emit_remappable(&mut self) -> bool {
            self.steps.push(Step::Emit);
            self.emit_ok
        }
        fn fall_back(&mut self) {
            self.steps.push(Step::FallBack);
        }
        fn resignal_edges(&mut self) {
            self.steps.push(Step::Resignal);
        }
    }

    fn run(enable_ok: bool, emit_ok: bool) -> (bool, alloc::vec::Vec<Step>) {
        let mut hw = Recorder {
            enable_ok,
            emit_ok,
            steps: alloc::vec::Vec::new(),
        };
        (switch(&mut hw), hw.steps)
    }

    #[test]
    fn edges_are_resignalled_after_the_remappable_rewrite() {
        use Step::*;
        assert_eq!(
            run(true, true),
            (true, alloc::vec![Mask, Enable, Emit, Resignal])
        );
    }

    #[test]
    fn every_fallback_restores_then_resignals() {
        use Step::*;
        assert_eq!(
            run(false, true),
            (false, alloc::vec![Mask, Enable, FallBack, Resignal])
        );
        assert_eq!(
            run(true, false),
            (false, alloc::vec![Mask, Enable, Emit, FallBack, Resignal])
        );
    }
}
