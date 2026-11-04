// SPDX-License-Identifier: MPL-2.0
//! Minimal no_std ACPI table parser for x86_64 kernel.
//!
//! Parses RSDP → XSDT/RSDT → MADT (APIC), MCFG (PCIe ECAM), HPET, DMAR, FADT
//! and SPCR.
//!
//! Design invariants:
//! - NEVER panics; every parse error leaves the affected hardware gate closed.
//! - The caller maps each physical range before this parser dereferences it.
//! - Only x86_64 needs this; other arches use DTB via `platform::init`.
//! - Parsing records facts only. Consumers decide policy, and a fact that is
//!   missing or truncated keeps that consumer's gate closed.

mod bytes;
mod dmar;
mod fadt;
mod hpet;
mod madt;
mod mcfg;
mod records;
mod sdt;
mod spcr;
#[cfg(test)]
mod tests;

pub use dmar::{DeviceScope, DrhdEntry, RmrrEntry, SCOPE_PATH_MAX};
pub use fadt::FadtInfo;
pub use records::{AcpiTopology, BoundedVec, CpuEntry, Gas, IoApicEntry, LapicNmi, McfgAlloc};
pub use spcr::SpcrInfo;

use crate::sync::Spinlock;

/// Parsed addresses from ACPI tables.
///
/// A zero base means the corresponding firmware table was not validated. This
/// is deliberately fail-closed: q35 addresses are not portable defaults. The
/// larger per-CPU / per-unit lists live in [`AcpiTopology`] (see
/// [`with_topology`]) so this value stays cheap to copy on the boot stack.
#[derive(Clone, Copy, Debug)]
pub struct AcpiInfo {
    /// Local APIC MMIO base (MADT Local APIC Address field or type-5 override).
    pub lapic_base: u64,
    /// I/O APIC MMIO base of the **last** MADT type-1 entry. Every I/O APIC is
    /// listed in [`AcpiTopology::ioapics`].
    pub ioapic_base: u64,
    /// I/O APIC Global System Interrupt base (MADT type-1 gsi_base).
    pub ioapic_gsi_base: u32,
    /// HPET event timer block address (HPET table GAS address field).
    pub hpet_base: u64,
    /// PCIe ECAM config space base (MCFG allocation[0].base_address).
    pub ecam_base: u64,
    /// PCIe ECAM bus start (MCFG allocation[0].bus_start).
    pub ecam_bus_start: u8,
    /// PCIe ECAM bus end (MCFG allocation[0].bus_end).
    pub ecam_bus_end: u8,
    /// ISA IRQ → GSI override table. Index = ISA IRQ (0–15); value = GSI.
    /// Entries not overridden by MADT type-2 keep identity mapping (IRQ N → GSI N).
    pub irq_overrides: [u32; 16],
    /// MPS INTI flags of each override (polarity bits 0-1, trigger bits 2-3);
    /// 0 = "conforms to the bus" (ISA: active-high, edge).
    pub irq_override_flags: [u16; 16],
    /// Intel VT-d DRHD register base discovered from ACPI DMAR. Zero means no
    /// DMAR table (or no usable unit) was validated, so no remapper address may
    /// be assumed — the caller must decide fail-closed rather than guess.
    pub dmar_base: u64,
    /// DRHD unit count the DMAR table declared (0 = no DMAR table).
    pub dmar_units: u8,
    /// The selected DRHD carries INCLUDE_PCI_ALL, so it covers PCI devices that
    /// declare no device scope of their own.
    pub dmar_include_pci_all: bool,
    /// A DMAR table passed its SDT checksum, whether or not it declared a usable
    /// unit. Absence never implies anything about DMA policy.
    pub dmar_present: bool,
    /// OEM identity from the validated XSDT (ACPI 2.0+) or RSDT header; `None`
    /// when no root table validated.
    pub root_oem: Option<AcpiOem>,
    /// Validated FADT facts, if the table was present and long enough.
    pub fadt: Option<FadtInfo>,
    /// Validated SPCR facts, if the firmware publishes a console port.
    pub spcr: Option<SpcrInfo>,
}

/// The OEM fields of an ACPI system description table header.
///
/// These name the firmware vendor and build, not the machine: identical
/// firmware on different machines reports identical values. The bytes are
/// firmware-controlled and carry no authority; they are kept verbatim
/// (space-padded ASCII by specification, but not checked) for identity
/// comparison and escaped logging.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcpiOem {
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
}

impl Default for AcpiInfo {
    fn default() -> Self {
        let mut overrides = [0u32; 16];
        for (i, v) in overrides.iter_mut().enumerate() {
            *v = i as u32;
        }
        Self {
            lapic_base: 0,
            ioapic_base: 0,
            ioapic_gsi_base: 0,
            hpet_base: 0,
            ecam_base: 0,
            ecam_bus_start: 0,
            ecam_bus_end: 0,
            irq_overrides: overrides,
            irq_override_flags: [0; 16],
            dmar_base: 0,
            dmar_units: 0,
            dmar_include_pci_all: false,
            dmar_present: false,
            root_oem: None,
            fadt: None,
            spcr: None,
        }
    }
}

/// Written once by [`parse`] on the BSP, before any other CPU starts and
/// before any reader runs. Leaf lock: nothing else is acquired while held,
/// except the caller's mapper during [`parse`] itself.
static TOPOLOGY: Spinlock<AcpiTopology> = Spinlock::new(AcpiTopology::EMPTY);

/// Run `f` over the topology recorded by the last [`parse`].
pub fn with_topology<R>(f: impl FnOnce(&AcpiTopology) -> R) -> R {
    f(&TOPOLOGY.lock())
}

/// Parse ACPI tables starting from `rsdp_phys`.
///
/// `map_physical` must make `[phys, phys + len)` readable and return its virtual
/// base. Returning `None` closes the affected hardware gates.
///
/// Returns `AcpiInfo` with parsed values; any field that cannot be validated
/// remains zero so callers cannot accidentally touch q35-specific MMIO. The
/// lists go to the topology static, replacing whatever an earlier call left.
///
/// # Panics
/// Never — all errors produce a log warning and fall through to defaults.
pub fn parse(
    rsdp_phys: usize,
    mut map_physical: impl FnMut(usize, usize) -> Option<usize>,
) -> AcpiInfo {
    let mut info = AcpiInfo::default();
    let mut topo = TOPOLOGY.lock();
    *topo = AcpiTopology::EMPTY;
    parse_into(rsdp_phys, &mut map_physical, &mut info, &mut topo);
    info
}

/// [`parse`] into caller-owned storage (host tests use a boxed topology).
fn parse_into(
    rsdp_phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
    topo: &mut AcpiTopology,
) {
    sdt::walk(rsdp_phys, map_physical, info, topo);
    for (table, kept, dropped) in topo.truncations() {
        if dropped != 0 {
            log::warn!(
                "[acpi] {} truncated: kept {}, dropped {}",
                table,
                kept,
                dropped
            );
        }
    }
}
