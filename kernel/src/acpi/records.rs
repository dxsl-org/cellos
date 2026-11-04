// SPDX-License-Identifier: MPL-2.0
//! Firmware-reported topology kept for later consumers (APIC, VT-d, PCIe).
//!
//! These are facts, not policy: a record existing grants nothing, and a
//! missing or truncated table leaves the matching consumer's gate closed.
//! Capacities are fixed because parsing runs before the kernel heap exists.

use super::dmar::{DrhdEntry, RmrrEntry};

/// A bounded, copyable list. Entries past `N` are counted in `dropped`, never
/// stored, so a hostile table cannot grow kernel memory.
#[derive(Clone, Copy, Debug)]
pub struct BoundedVec<T: Copy, const N: usize> {
    items: [T; N],
    len: usize,
    dropped: usize,
}

impl<T: Copy, const N: usize> BoundedVec<T, N> {
    pub const fn new(zero: T) -> Self {
        Self {
            items: [zero; N],
            len: 0,
            dropped: 0,
        }
    }

    /// Append `item`; when full, count it as dropped and return `false`.
    pub fn push(&mut self, item: T) -> bool {
        if self.len == N {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.items[self.len] = item;
        self.len += 1;
        true
    }

    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Entries the table declared beyond capacity.
    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

/// ACPI Generic Address Structure (12 bytes on the wire).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Gas {
    /// 0 = system memory, 1 = system I/O, 2 = PCI configuration space.
    pub space_id: u8,
    pub bit_width: u8,
    pub bit_offset: u8,
    /// 0 undefined, 1 byte, 2 word, 3 dword, 4 qword.
    pub access_size: u8,
    pub address: u64,
}

impl Gas {
    pub const SYSTEM_MEMORY: u8 = 0;
    pub const SYSTEM_IO: u8 = 1;

    pub fn is_present(&self) -> bool {
        self.address != 0
    }
}

/// MADT processor entry (type 0 Local APIC or type 9 Local x2APIC).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuEntry {
    pub apic_id: u32,
    pub acpi_uid: u32,
    /// Bit 0 = enabled, bit 1 = online capable (ACPI 6.3+).
    pub flags: u32,
    /// Declared through a type-9 (x2APIC) record.
    pub x2apic: bool,
}

impl CpuEntry {
    pub const ZERO: Self = Self {
        apic_id: 0,
        acpi_uid: 0,
        flags: 0,
        x2apic: false,
    };

    /// Enabled now, or may be brought online later.
    pub fn usable(&self) -> bool {
        self.flags & 0b11 != 0
    }
}

/// MADT type-1 I/O APIC record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IoApicEntry {
    pub id: u8,
    pub address: u64,
    pub gsi_base: u32,
}

impl IoApicEntry {
    pub const ZERO: Self = Self {
        id: 0,
        address: 0,
        gsi_base: 0,
    };
}

/// MADT type-4 (Local APIC NMI) or type-0xA (Local x2APIC NMI) record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LapicNmi {
    /// `u32::MAX` means every processor (xAPIC `0xFF` is widened to it).
    pub acpi_uid: u32,
    /// MPS INTI flags: polarity bits 0-1, trigger mode bits 2-3.
    pub flags: u16,
    pub lint: u8,
}

impl LapicNmi {
    pub const ZERO: Self = Self {
        acpi_uid: 0,
        flags: 0,
        lint: 0,
    };
}

/// One raw MCFG allocation. `base` is bus-0 relative, exactly as published.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct McfgAlloc {
    pub base: u64,
    pub segment: u16,
    pub bus_start: u8,
    pub bus_end: u8,
}

impl McfgAlloc {
    pub const ZERO: Self = Self {
        base: 0,
        segment: 0,
        bus_start: 0,
        bus_end: 0,
    };
}

/// The firmware topology too large to copy around on the 64 KiB boot stack.
///
/// One instance lives in a static written by `acpi::parse` before any other
/// CPU starts; readers go through `acpi::with_topology`.
#[derive(Clone, Copy, Debug)]
pub struct AcpiTopology {
    pub cpus: BoundedVec<CpuEntry, 256>,
    pub ioapics: BoundedVec<IoApicEntry, 16>,
    pub lapic_nmis: BoundedVec<LapicNmi, 16>,
    pub mcfg: BoundedVec<McfgAlloc, 8>,
    pub drhds: BoundedVec<DrhdEntry, 16>,
    pub rmrrs: BoundedVec<RmrrEntry, 8>,
    /// DMAR header flags: bit 0 INTR_REMAP, bit 1 X2APIC_OPT_OUT.
    pub dmar_flags: u8,
    pub dmar_host_address_width: u8,
}

impl AcpiTopology {
    pub const EMPTY: Self = Self {
        cpus: BoundedVec::new(CpuEntry::ZERO),
        ioapics: BoundedVec::new(IoApicEntry::ZERO),
        lapic_nmis: BoundedVec::new(LapicNmi::ZERO),
        mcfg: BoundedVec::new(McfgAlloc::ZERO),
        drhds: BoundedVec::new(DrhdEntry::ZERO),
        rmrrs: BoundedVec::new(RmrrEntry::ZERO),
        dmar_flags: 0,
        dmar_host_address_width: 0,
    };

    pub fn dmar_intr_remap(&self) -> bool {
        self.dmar_flags & 1 != 0
    }

    pub fn dmar_x2apic_opt_out(&self) -> bool {
        self.dmar_flags & 2 != 0
    }

    /// `(table, kept, dropped)` for every list that hit its capacity.
    pub fn truncations(&self) -> [(&'static str, usize, usize); 6] {
        [
            ("MADT cpus", self.cpus.len(), self.cpus.dropped()),
            ("MADT ioapics", self.ioapics.len(), self.ioapics.dropped()),
            (
                "MADT nmis",
                self.lapic_nmis.len(),
                self.lapic_nmis.dropped(),
            ),
            ("MCFG", self.mcfg.len(), self.mcfg.dropped()),
            ("DMAR drhd", self.drhds.len(), self.drhds.dropped()),
            ("DMAR rmrr", self.rmrrs.len(), self.rmrrs.dropped()),
        ]
    }
}
