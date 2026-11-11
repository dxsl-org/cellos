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
// The G1 private lane does not call `sci::init` (its x86 contract is pinned),
// so the discovery half of these modules is unused there.
#[cfg(target_arch = "x86_64")]
#[cfg_attr(feature = "g1-cpu-accounting", allow(dead_code))]
pub mod power;
#[cfg(target_arch = "x86_64")]
#[cfg_attr(feature = "g1-cpu-accounting", allow(dead_code))]
pub mod sci;
mod spcr;
#[cfg(test)]
mod tests;

pub use dmar::{
    DeviceScope, DrhdEntry, RmrrEntry, DRHD_SCOPES_MAX, RMRR_SCOPES_MAX, SCOPE_PATH_MAX,
};
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
static TOPOLOGY: Spinlock<AcpiTopology> =
    Spinlock::ranked(AcpiTopology::EMPTY, crate::sync::rank::ACPI_TOPOLOGY);

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

/// Upper bound on one table, as in the SDT walk.
#[cfg(target_arch = "x86_64")]
const MAX_TABLE_LEN: usize = 0x10_0000;

/// `(virt, len)` of the table at `phys` once the mapper admitted its header and
/// then its whole length, the length is plausible and the checksum is zero.
/// No byte is read before the mapper admitted it.
#[cfg(target_arch = "x86_64")]
fn checked_table(
    phys: u64,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
) -> Option<(usize, usize)> {
    let phys = usize::try_from(phys).ok().filter(|&phys| phys != 0)?;
    let header = map_physical(phys, 36)?;
    // SAFETY: the mapper admitted `[header, header + 36)`; the length is at 4.
    let len = unsafe { sdt::read_unaligned::<u32>(header + 4) } as usize;
    if !(36..=MAX_TABLE_LEN).contains(&len) {
        return None;
    }
    let virt = map_physical(phys, len)?;
    // SAFETY: the mapper admitted `[virt, virt + len)`.
    (unsafe { sdt::table_checksum(virt, len) } == 0).then_some((virt, len))
}

/// Run `visit` over the checksummed table at `phys`, header included; `None`
/// when the mapper refuses it or its length or checksum is invalid.
#[cfg(target_arch = "x86_64")]
pub(crate) fn with_table<R>(
    phys: u64,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
    visit: impl FnOnce(&[u8]) -> R,
) -> Option<R> {
    let (virt, len) = checked_table(phys, map_physical)?;
    // SAFETY: `checked_table` had the mapper admit `[virt, virt + len)`, and an
    // admitted range stays mapped for at least this call.
    Some(visit(unsafe {
        core::slice::from_raw_parts(virt as *const u8, len)
    }))
}

/// Root table `(physical address, entry width)` named by a valid RSDP: the
/// XSDT for revision 2 and later, otherwise the RSDT.
#[cfg(target_arch = "x86_64")]
fn root_table(
    rsdp_phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
) -> Option<(u64, usize)> {
    if rsdp_phys == 0 {
        return None;
    }
    let virt = map_physical(rsdp_phys, core::mem::size_of::<sdt::RsdpV2>())?;
    // SAFETY: the mapper admitted the 36-byte v2 layout.
    let rsdp: [u8; 36] = unsafe { sdt::read_unaligned(virt) };
    let v1_sum = rsdp[..20]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    if &rsdp[..8] != b"RSD PTR " || v1_sum != 0 {
        return None;
    }
    let field = |range: core::ops::Range<usize>| {
        rsdp[range]
            .iter()
            .rev()
            .fold(0u64, |value, byte| value << 8 | *byte as u64)
    };
    if rsdp[15] < 2 {
        return Some((field(16..20), 4));
    }
    let length = field(20..24) as usize;
    if !(36..=4096).contains(&length) {
        return None;
    }
    let full = map_physical(rsdp_phys, length)?;
    // SAFETY: the mapper admitted the published RSDP length.
    (unsafe { sdt::table_checksum(full, length) } == 0).then_some((field(24..32), 8))
}

/// Run `visit` over every checksummed table the root table lists, in list
/// order, until it returns `false`. A child that fails its checks is skipped;
/// an RSDP or root table that fails visits nothing.
#[cfg(target_arch = "x86_64")]
pub(crate) fn for_each_listed_table(
    rsdp_phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
    mut visit: impl FnMut(&[u8]) -> bool,
) {
    let Some((root, entry_len)) = root_table(rsdp_phys, map_physical) else {
        return;
    };
    let Some((virt, len)) = checked_table(root, map_physical) else {
        return;
    };
    for index in 0..(len - 36) / entry_len {
        let at = virt + 36 + index * entry_len;
        // SAFETY: the entry lies inside the admitted, checksummed root table.
        let child = unsafe {
            if entry_len == 8 {
                sdt::read_unaligned::<u64>(at)
            } else {
                sdt::read_unaligned::<u32>(at) as u64
            }
        };
        if child != 0 && with_table(child, map_physical, &mut visit) == Some(false) {
            return;
        }
    }
}
