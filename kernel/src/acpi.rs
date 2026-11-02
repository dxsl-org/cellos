// SPDX-License-Identifier: MPL-2.0
//! Minimal no_std ACPI table parser for x86_64 kernel.
//!
//! Parses RSDP → XSDT/RSDT → MADT (APIC), MCFG (PCIe ECAM), HPET.
//!
//! Design invariants:
//! - NEVER panics; every parse error leaves the affected hardware gate closed.
//! - The caller maps each physical range before this parser dereferences it.
//! - Only x86_64 needs this; other arches use DTB via `platform::init`.

/// Parsed addresses from ACPI tables.
///
/// A zero base means the corresponding firmware table was not validated. This
/// is deliberately fail-closed: q35 addresses are not portable defaults.
#[derive(Clone, Copy, Debug)]
pub struct AcpiInfo {
    /// Local APIC MMIO base (MADT Local APIC Address field or type-5 override).
    pub lapic_base: u64,
    /// I/O APIC MMIO base (MADT type-1 entry).
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
            dmar_base: 0,
            dmar_units: 0,
            dmar_include_pci_all: false,
            dmar_present: false,
            root_oem: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal layout types — packed structs used only for read_unaligned casts.
// ---------------------------------------------------------------------------

// NOTE: SdtHeader / MadtHeader / Gas are documented here as layout reference
// but not instantiated — we read individual fields by explicit byte offset
// to avoid references to packed struct fields (which Rust forbids).
// Table offsets are documented in each parser function.

/// RSDP v1 (revision 0) — 20 bytes.
#[derive(Copy, Clone)]
#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
}

/// RSDP v2 (revision ≥ 2) extension — layout after the v1 struct.
/// Total size: 36 bytes.
#[derive(Copy, Clone)]
#[repr(C, packed)]
struct RsdpV2 {
    v1: Rsdp,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    _reserved: [u8; 3],
}

// ---------------------------------------------------------------------------
// Safety helpers
// ---------------------------------------------------------------------------

/// Read a `T` from a raw virtual address without alignment requirement.
///
/// # Safety
/// `virt` must be a valid, readable virtual address pointing to at least
/// `size_of::<T>()` bytes. The value is read byte-by-byte to avoid UB from
/// unaligned packed struct access.
#[inline]
unsafe fn read_unaligned<T: Copy>(virt: usize) -> T {
    // SAFETY: caller guarantees virt is valid.
    unsafe { core::ptr::read_unaligned(virt as *const T) }
}

/// Compute the byte checksum of a memory region (ACPI table validation).
///
/// # Safety
/// `[base, base + len)` must be readable virtual memory.
unsafe fn table_checksum(base: usize, len: usize) -> u8 {
    let mut sum: u8 = 0;
    for i in 0..len {
        // SAFETY: caller guarantees [base, base+len) is readable.
        let byte = unsafe { core::ptr::read_volatile((base + i) as *const u8) };
        sum = sum.wrapping_add(byte);
    }
    sum
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse ACPI tables starting from `rsdp_phys`.
///
/// `map_physical` must make `[phys, phys + len)` readable and return its virtual
/// base. Returning `None` closes the affected hardware gates.
///
/// Returns `AcpiInfo` with parsed values; any field that cannot be validated
/// remains zero so callers cannot accidentally touch q35-specific MMIO.
///
/// # Panics
/// Never — all errors produce a log warning and fall through to defaults.
pub fn parse(
    rsdp_phys: usize,
    mut map_physical: impl FnMut(usize, usize) -> Option<usize>,
) -> AcpiInfo {
    let mut info = AcpiInfo::default();

    if rsdp_phys == 0 {
        log::warn!("[acpi] RSDP physical address is null — hardware gates closed");
        return info;
    }

    let Some(rsdp_virt) = map_physical(rsdp_phys, core::mem::size_of::<RsdpV2>()) else {
        log::warn!("[acpi] RSDP mapping failed — hardware gates closed");
        return info;
    };

    // --- Validate RSDP signature ---
    // SAFETY: Limine guarantees the RSDP response contains a valid physical
    // address to the RSDP structure, mapped via HHDM. We read cautiously.
    let sig_bytes: [u8; 8] = unsafe { read_unaligned(rsdp_virt) };
    if &sig_bytes != b"RSD PTR " {
        log::warn!("[acpi] RSDP signature mismatch — hardware gates closed");
        return info;
    }

    // --- Validate RSDP v1 checksum (bytes 0..19) ---
    // SAFETY: RSDP is at least 20 bytes (v1 size); Limine guarantees it.
    let cksum = unsafe { table_checksum(rsdp_virt, 20) };
    if cksum != 0 {
        log::warn!(
            "[acpi] RSDP v1 checksum failed ({}) — hardware gates closed",
            cksum
        );
        return info;
    }

    // SAFETY: RSDP is at least size_of::<Rsdp>() bytes.
    let rsdp: Rsdp = unsafe { read_unaligned(rsdp_virt) };
    let revision = rsdp.revision;

    if revision >= 2 {
        // ACPI 2.0+ extends the RSDP checksum across the length published in
        // the v2 header. Do not trust the XSDT pointer until that full record
        // has been mapped and validated.
        let rsdp_v2: RsdpV2 = unsafe { read_unaligned(rsdp_virt) };
        let rsdp_length = rsdp_v2.length as usize;
        if !(core::mem::size_of::<RsdpV2>()..=4096).contains(&rsdp_length) {
            log::warn!(
                "[acpi] RSDP v2 has implausible length {} — hardware gates closed",
                rsdp_length
            );
            return info;
        }
        let Some(rsdp_v2_virt) = map_physical(rsdp_phys, rsdp_length) else {
            log::warn!("[acpi] RSDP v2 mapping failed — hardware gates closed");
            return info;
        };
        let extended_cksum = unsafe { table_checksum(rsdp_v2_virt, rsdp_length) };
        if extended_cksum != 0 {
            log::warn!(
                "[acpi] RSDP v2 checksum failed ({}) — hardware gates closed",
                extended_cksum
            );
            return info;
        }

        // XSDT path: 64-bit table pointers.
        let rsdp_v2: RsdpV2 = unsafe { read_unaligned(rsdp_v2_virt) };
        let xsdt_phys = rsdp_v2.xsdt_address as usize;
        if xsdt_phys == 0 {
            log::warn!("[acpi] XSDT address is null — hardware gates closed");
            return info;
        }
        // SAFETY: validate_sdt asks the mapper to cover the header and body
        // before either range is dereferenced.
        unsafe {
            parse_xsdt(xsdt_phys, &mut map_physical, &mut info);
        }
    } else {
        // RSDT path: 32-bit table pointers.
        let rsdt_phys = rsdp.rsdt_address as usize;
        if rsdt_phys == 0 {
            log::warn!("[acpi] RSDT address is null — hardware gates closed");
            return info;
        }
        // SAFETY: validate_sdt asks the mapper to cover the header and body
        // before either range is dereferenced.
        unsafe {
            parse_rsdt(rsdt_phys, &mut map_physical, &mut info);
        }
    }

    info
}

// ---------------------------------------------------------------------------
// XSDT / RSDT iteration
// ---------------------------------------------------------------------------

/// Validate an SDT header checksum and return `(virt_base, length)` if valid.
///
/// Returns `None` and logs a warning if the checksum fails.
unsafe fn validate_sdt(
    phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
) -> Option<(usize, usize)> {
    let Some(header_virt) = map_physical(phys, 36) else {
        log::warn!("[acpi] SDT header mapping failed at {:#x}", phys);
        return None;
    };
    // Read the length field at offset 4 (4 bytes into the header).
    // SAFETY: phys points to a valid SDT; length field is at offset 4.
    let length = unsafe { core::ptr::read_unaligned((header_virt + 4) as *const u32) } as usize;
    if !(36..=0x10_0000).contains(&length) {
        log::warn!(
            "[acpi] SDT at {:#x} has implausible length {} — skipping",
            phys,
            length
        );
        return None;
    }
    let Some(virt) = map_physical(phys, length) else {
        log::warn!("[acpi] SDT body mapping failed at {:#x}", phys);
        return None;
    };
    // SAFETY: the mapper confirmed [virt, virt+length) is readable.
    let cksum = unsafe { table_checksum(virt, length) };
    if cksum != 0 {
        log::warn!(
            "[acpi] SDT at {:#x} checksum failed ({}) — skipping",
            phys,
            cksum
        );
        return None;
    }
    Some((virt, length))
}

/// Read the 4-byte signature at `virt`.
#[inline]
unsafe fn read_sig(virt: usize) -> [u8; 4] {
    // SAFETY: caller guarantees virt is within a valid SDT.
    unsafe { core::ptr::read_unaligned(virt as *const [u8; 4]) }
}

/// Read the OEM fields of the SDT header at `virt`.
///
/// # Safety
/// `[virt, virt + 36)` must be a readable, validated SDT header.
unsafe fn read_sdt_oem(virt: usize) -> AcpiOem {
    // SAFETY: offsets 10..28 lie inside the caller's 36-byte header.
    unsafe {
        AcpiOem {
            oem_id: core::ptr::read_unaligned((virt + 10) as *const [u8; 6]),
            oem_table_id: core::ptr::read_unaligned((virt + 16) as *const [u8; 8]),
            oem_revision: core::ptr::read_unaligned((virt + 24) as *const u32),
        }
    }
}

/// Iterate XSDT (64-bit pointer array) and dispatch each child SDT.
unsafe fn parse_xsdt(
    phys: usize,
    mapper: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
) {
    let Some((virt, length)) = (unsafe { validate_sdt(phys, mapper) }) else {
        log::warn!("[acpi] XSDT validation failed — hardware gates closed");
        return;
    };
    // SAFETY: validate_sdt returned a checksummed table of at least 36 bytes.
    info.root_oem = Some(unsafe { read_sdt_oem(virt) });

    // Entries start at byte 36 (after the 36-byte common header).
    // Each entry is an 8-byte physical pointer.
    let entries_start = virt + 36;
    let entries_len = (length - 36) / 8;

    for i in 0..entries_len {
        // SAFETY: within validated XSDT body.
        let child_phys =
            unsafe { core::ptr::read_unaligned((entries_start + i * 8) as *const u64) } as usize;
        if child_phys == 0 {
            continue;
        }
        dispatch_sdt(child_phys, mapper, info);
    }
}

/// Iterate RSDT (32-bit pointer array) and dispatch each child SDT.
unsafe fn parse_rsdt(
    phys: usize,
    mapper: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
) {
    let Some((virt, length)) = (unsafe { validate_sdt(phys, mapper) }) else {
        log::warn!("[acpi] RSDT validation failed — hardware gates closed");
        return;
    };
    // SAFETY: validate_sdt returned a checksummed table of at least 36 bytes.
    info.root_oem = Some(unsafe { read_sdt_oem(virt) });

    // Entries start at byte 36; each entry is a 4-byte physical pointer.
    let entries_start = virt + 36;
    let entries_len = (length - 36) / 4;

    for i in 0..entries_len {
        // SAFETY: within validated RSDT body.
        let child_phys =
            unsafe { core::ptr::read_unaligned((entries_start + i * 4) as *const u32) } as usize;
        if child_phys == 0 {
            continue;
        }
        dispatch_sdt(child_phys, mapper, info);
    }
}

/// Validate and dispatch a child SDT by its 4-byte signature.
fn dispatch_sdt(
    phys: usize,
    mapper: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
) {
    let Some((virt, length)) = (unsafe { validate_sdt(phys, mapper) }) else {
        return;
    };
    // SAFETY: virt is the start of a validated SDT.
    let sig = unsafe { read_sig(virt) };

    match &sig {
        b"APIC" => parse_madt(virt, length, info),
        b"MCFG" => parse_mcfg(virt, length, info),
        b"HPET" => parse_hpet(virt, length, info),
        b"DMAR" => {
            info.dmar_present = true;
            parse_dmar(virt, length, info)
        }
        _ => {
            // Skip unknown/unneeded tables silently.
        }
    }
}

// ---------------------------------------------------------------------------
// MADT parser
// ---------------------------------------------------------------------------

/// Parse MADT (Multiple APIC Description Table) for LAPIC/IOAPIC/IRQ overrides.
///
/// Reads:
///   - MADT header `local_apic_address` (32-bit)
///   - Type 1 (I/O APIC): ioapic_addr, gsi_base
///   - Type 2 (Int Source Override): ISA IRQ → GSI mapping
///   - Type 5 (Local APIC Address Override): 64-bit lapic_addr
fn parse_madt(virt: usize, length: usize, info: &mut AcpiInfo) {
    // MADT-specific header starts after the common 36-byte SDT header.
    // Layout: local_apic_address (u32) at offset 36, flags (u32) at offset 40.
    // Entry records begin at offset 44.
    if length < 44 {
        log::warn!("[acpi] MADT too short ({} bytes)", length);
        return;
    }

    // Read Local APIC 32-bit base from MADT header.
    // SAFETY: virt+36 is within the validated MADT body.
    let lapic_addr_32 = unsafe { core::ptr::read_unaligned((virt + 36) as *const u32) };
    if lapic_addr_32 != 0 {
        info.lapic_base = lapic_addr_32 as u64;
    }

    let mut offset = 44usize; // start of MADT entry records
    let end = virt + length;

    while virt + offset + 2 <= end {
        // Each entry: type (u8), length (u8), then type-specific payload.
        // SAFETY: offset is within the validated MADT.
        let entry_type = unsafe { core::ptr::read_volatile((virt + offset) as *const u8) };
        let entry_len =
            unsafe { core::ptr::read_volatile((virt + offset + 1) as *const u8) } as usize;

        if entry_len < 2 || virt + offset + entry_len > end {
            // Malformed entry — stop parsing entries but keep what we got.
            log::warn!(
                "[acpi] MADT entry type={} has bad length={}",
                entry_type,
                entry_len
            );
            break;
        }

        match entry_type {
            // Type 0: Processor Local APIC (length 8) — skip (no SMP enum needed).
            0 => {}

            // Type 1: I/O APIC (length 12).
            //   offset+2: ioapic_id (u8)
            //   offset+3: reserved (u8)
            //   offset+4: ioapic_address (u32)
            //   offset+8: gsi_base (u32)
            1 if entry_len >= 12 => {
                let ioapic_addr =
                    unsafe { core::ptr::read_unaligned((virt + offset + 4) as *const u32) };
                let gsi_base =
                    unsafe { core::ptr::read_unaligned((virt + offset + 8) as *const u32) };
                if ioapic_addr != 0 {
                    info.ioapic_base = ioapic_addr as u64;
                    info.ioapic_gsi_base = gsi_base;
                }
            }

            // Type 2: Interrupt Source Override (length 10).
            //   offset+2: bus (u8) — always 0 for ISA
            //   offset+3: source (u8) — ISA IRQ number
            //   offset+4: gsi (u32)
            //   offset+8: flags (u16)
            2 if entry_len >= 10 => {
                let bus = unsafe { core::ptr::read_volatile((virt + offset + 2) as *const u8) };
                let source = unsafe { core::ptr::read_volatile((virt + offset + 3) as *const u8) };
                let gsi = unsafe { core::ptr::read_unaligned((virt + offset + 4) as *const u32) };
                // Only remap ISA bus (bus==0) IRQs that fit in our table.
                if bus == 0 && (source as usize) < 16 {
                    info.irq_overrides[source as usize] = gsi;
                }
            }

            // Type 4: Non-Maskable Interrupt (NMI) (length 6) — skip.
            4 => {}

            // Type 5: Local APIC Address Override (length 12).
            //   offset+2: reserved (u16)
            //   offset+4: lapic_address (u64)
            5 if entry_len >= 12 => {
                let lapic64 =
                    unsafe { core::ptr::read_unaligned((virt + offset + 4) as *const u64) };
                if lapic64 != 0 {
                    info.lapic_base = lapic64;
                }
            }

            // Ignore all other entry types (type 3, 6, 7, 9, 0xA, etc.).
            _ => {}
        }

        offset += entry_len;
    }
}

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
fn parse_mcfg(virt: usize, length: usize, info: &mut AcpiInfo) {
    // First allocation entry starts at offset 44 (36-byte header + 8-byte reserved).
    if length < 44 + 16 {
        log::warn!(
            "[acpi] MCFG too short ({} bytes) for any allocation entry",
            length
        );
        return;
    }

    let entry_count = (length - 44) / 16;
    for index in 0..entry_count {
        let entry = virt + 44 + index * 16;
        // SAFETY: every field is within this validated 16-byte allocation entry.
        let base_addr = unsafe { core::ptr::read_unaligned(entry as *const u64) };
        let segment = unsafe { core::ptr::read_unaligned((entry + 8) as *const u16) };
        let bus_start = unsafe { core::ptr::read_volatile((entry + 10) as *const u8) };
        let bus_end = unsafe { core::ptr::read_volatile((entry + 11) as *const u8) };
        let window_len = crate::task::drivers::pcie_ecam::ecam_window_size(bus_start, bus_end);
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
fn parse_hpet(virt: usize, length: usize, info: &mut AcpiInfo) {
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
    if hpet_addr != 0 {
        info.hpet_base = hpet_addr;
        log::info!("[acpi] HPET: event timer block = {:#x}", hpet_addr);
    } else {
        log::warn!("[acpi] HPET: GAS address is 0 — timer gate closed");
    }
}

// ---------------------------------------------------------------------------
// DMAR parser
// ---------------------------------------------------------------------------

/// Parse DMAR (DMA Remapping Reporting Table) for the DRHD register base.
///
/// DMAR layout (after the 36-byte SDT header):
///   offset 36: host_address_width (u8)
///   offset 37: flags (u8) — bit 0 = INTR_REMAP
///   offset 38: reserved (10 bytes)
///   offset 48: device-scope records
///
/// Each record: type (u16), length (u16), then type-specific payload.
///   - Type 0 (DRHD): flags (u8 @4, bit 0 = INCLUDE_PCI_ALL), size (u8 @5),
///     segment (u16 @6), register_base (u64 @8), device scopes after @16.
///   - Types 1/2/3 (RMRR/ATSR/SAT): skipped by their own length. They do not
///     move the register base; RMRR correctness matters only for the devices
///     this kernel drives, which declare no RMRR in the exercised machines.
///
/// Selection: the first DRHD carrying INCLUDE_PCI_ALL covers every PCI device
/// and is preferred; otherwise the first segment-0 DRHD is used and
/// [`AcpiInfo::dmar_include_pci_all`] stays false so the caller can see the unit
/// is scope-limited. Units on other segments are counted but not selected — the
/// kernel drives segment 0. A malformed record stops the walk and leaves
/// whatever was already validated, which keeps the caller's decision fail-closed
/// instead of dereferencing past the table.
fn parse_dmar(virt: usize, length: usize, info: &mut AcpiInfo) {
    if length < 48 {
        log::warn!("[acpi] DMAR too short ({} bytes) for any unit", length);
        return;
    }

    let mut units: u8 = 0;
    let mut selected: Option<(u64, bool)> = None;
    let mut offset = 48usize;

    while offset + 4 <= length {
        // SAFETY: the record header is inside the validated DMAR body.
        let record_type = unsafe { core::ptr::read_unaligned((virt + offset) as *const u16) };
        // SAFETY: same record header.
        let record_len =
            unsafe { core::ptr::read_unaligned((virt + offset + 2) as *const u16) } as usize;
        if record_len < 4 || offset + record_len > length {
            log::warn!(
                "[acpi] DMAR record type {} has invalid length {} — stopping the walk",
                record_type,
                record_len
            );
            break;
        }

        if record_type == 0 {
            if record_len < 16 {
                log::warn!("[acpi] DMAR DRHD shorter than its fixed fields — stopping");
                break;
            }
            // SAFETY: fixed DRHD fields are within this validated record.
            let flags = unsafe { core::ptr::read_volatile((virt + offset + 4) as *const u8) };
            // SAFETY: as above.
            let segment = unsafe { core::ptr::read_unaligned((virt + offset + 6) as *const u16) };
            // SAFETY: as above.
            let register_base =
                unsafe { core::ptr::read_unaligned((virt + offset + 8) as *const u64) };
            units = units.saturating_add(1);

            if segment == 0 && register_base != 0 {
                let include_all = flags & 1 != 0;
                let better = match selected {
                    None => true,
                    // A scope-limited unit never displaces an all-inclusive one.
                    Some((_, true)) => false,
                    Some(_) => include_all,
                };
                if better {
                    selected = Some((register_base, include_all));
                }
            }
        }
        offset += record_len;
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    fn mcfg_entry(base: u64, segment: u16, bus_start: u8, bus_end: u8) -> Vec<u8> {
        let mut table = vec![0u8; 60];
        table[44..52].copy_from_slice(&base.to_le_bytes());
        table[52..54].copy_from_slice(&segment.to_le_bytes());
        table[54] = bus_start;
        table[55] = bus_end;
        table
    }

    #[test]
    fn mcfg_normalizes_nonzero_start_to_the_admitted_window() {
        let table = mcfg_entry(0xB000_0000, 0, 64, 65);
        let mut info = AcpiInfo::default();
        parse_mcfg(table.as_ptr() as usize, table.len(), &mut info);
        assert_eq!(info.ecam_base, 0xB400_0000);
        assert_eq!(info.ecam_bus_start, 64);
        assert_eq!(info.ecam_bus_end, 65);
    }

    #[test]
    fn mcfg_rejects_reversed_or_overflowing_allocations() {
        let reversed = mcfg_entry(0xB000_0000, 0, 65, 64);
        let mut info = AcpiInfo::default();
        parse_mcfg(reversed.as_ptr() as usize, reversed.len(), &mut info);
        assert_eq!(info.ecam_base, 0);

        let overflowing = mcfg_entry(u64::MAX & !0xF_FFFF, 0, 0, u8::MAX);
        parse_mcfg(overflowing.as_ptr() as usize, overflowing.len(), &mut info);
        assert_eq!(info.ecam_base, 0);
    }

    /// A DMAR body: 48-byte header (host_address_width, flags, reserved) then
    /// the records.
    fn dmar_table(records: &[Vec<u8>]) -> Vec<u8> {
        let mut table = vec![0u8; 48];
        for record in records {
            table.extend_from_slice(record);
        }
        table
    }

    /// Type 0 DRHD with no device scopes (16 bytes).
    fn drhd(flags: u8, segment: u16, base: u64) -> Vec<u8> {
        let mut record = vec![0u8; 16];
        record[0..2].copy_from_slice(&0u16.to_le_bytes());
        record[2..4].copy_from_slice(&16u16.to_le_bytes());
        record[4] = flags;
        record[6..8].copy_from_slice(&segment.to_le_bytes());
        record[8..16].copy_from_slice(&base.to_le_bytes());
        record
    }

    /// Type 1 RMRR: opaque to the base decision, skipped by its own length.
    fn rmrr() -> Vec<u8> {
        let mut record = vec![0xAAu8; 24];
        record[0..2].copy_from_slice(&1u16.to_le_bytes());
        record[2..4].copy_from_slice(&24u16.to_le_bytes());
        record
    }

    #[test]
    fn dmar_prefers_the_all_inclusive_unit_and_skips_other_record_types() {
        let table = dmar_table(&[rmrr(), drhd(0, 0, 0xAAAA_0000), drhd(1, 0, 0xFED9_0000)]);
        let mut info = AcpiInfo::default();
        parse_dmar(table.as_ptr() as usize, table.len(), &mut info);
        assert_eq!(info.dmar_base, 0xFED9_0000);
        assert!(info.dmar_include_pci_all);
        assert_eq!(info.dmar_units, 2);
    }

    #[test]
    fn dmar_accepts_a_scope_limited_unit_without_claiming_full_coverage() {
        let table = dmar_table(&[drhd(0, 0, 0xFED9_0000)]);
        let mut info = AcpiInfo::default();
        parse_dmar(table.as_ptr() as usize, table.len(), &mut info);
        assert_eq!(info.dmar_base, 0xFED9_0000);
        assert!(!info.dmar_include_pci_all);
        assert_eq!(info.dmar_units, 1);
    }

    #[test]
    fn dmar_ignores_other_segments_and_stops_at_a_malformed_record() {
        // A unit on another segment is counted but never selected.
        let other_segment = dmar_table(&[drhd(1, 2, 0xFED9_0000)]);
        let mut info = AcpiInfo::default();
        parse_dmar(
            other_segment.as_ptr() as usize,
            other_segment.len(),
            &mut info,
        );
        assert_eq!(info.dmar_base, 0);
        assert_eq!(info.dmar_units, 1);

        // A record whose length runs past the table stops the walk: the base from
        // the valid record before it survives, and nothing is read out of bounds.
        let mut truncated = dmar_table(&[drhd(1, 0, 0xFED9_0000)]);
        truncated.extend_from_slice(&[0u8, 0, 0xFF, 0xFF]);
        let mut info = AcpiInfo::default();
        parse_dmar(truncated.as_ptr() as usize, truncated.len(), &mut info);
        assert_eq!(info.dmar_base, 0xFED9_0000);

        // An undersized DRHD stops the walk without a base.
        let mut undersized = dmar_table(&[]);
        undersized.extend_from_slice(&[0u8, 0, 8, 0]);
        let mut info = AcpiInfo::default();
        parse_dmar(undersized.as_ptr() as usize, undersized.len(), &mut info);
        assert_eq!(info.dmar_base, 0);
    }

    /// Stamp a common SDT header over `table[0..36]` and fix its checksum.
    fn seal_sdt(mut table: Vec<u8>, signature: &[u8; 4], oem_id: &[u8; 6]) -> Vec<u8> {
        let length = table.len() as u32;
        table[0..4].copy_from_slice(signature);
        table[4..8].copy_from_slice(&length.to_le_bytes());
        table[8] = 1;
        table[9] = 0;
        table[10..16].copy_from_slice(oem_id);
        table[16..24].copy_from_slice(b"BXPC    ");
        table[24..28].copy_from_slice(&1u32.to_le_bytes());
        let sum = table.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        table[9] = 0u8.wrapping_sub(sum);
        table
    }

    /// Physical memory image: RSDP at 0x10, root table at 0x100, children from
    /// 0x400. `revision >= 2` builds RSDP v2 + XSDT, otherwise RSDP v1 + RSDT.
    fn firmware_image(revision: u8, root_oem: &[u8; 6], children: &[Vec<u8>]) -> Vec<u8> {
        const RSDP: usize = 0x10;
        const ROOT: usize = 0x100;
        let mut memory = vec![0u8; 0x2000];
        let mut child_phys = Vec::new();
        let mut cursor = 0x400;
        for child in children {
            memory[cursor..cursor + child.len()].copy_from_slice(child);
            child_phys.push(cursor as u64);
            cursor += child.len().next_multiple_of(16);
        }

        let entry_size = if revision >= 2 { 8 } else { 4 };
        let mut root = vec![0u8; 36];
        for phys in &child_phys {
            root.extend_from_slice(&phys.to_le_bytes()[..entry_size]);
        }
        let signature = if revision >= 2 { b"XSDT" } else { b"RSDT" };
        let root = seal_sdt(root, signature, root_oem);
        memory[ROOT..ROOT + root.len()].copy_from_slice(&root);

        let rsdp = &mut memory[RSDP..RSDP + 36];
        rsdp[0..8].copy_from_slice(b"RSD PTR ");
        rsdp[9..15].copy_from_slice(b"RSDPOE");
        rsdp[15] = revision;
        if revision >= 2 {
            rsdp[20..24].copy_from_slice(&36u32.to_le_bytes());
            rsdp[24..32].copy_from_slice(&(ROOT as u64).to_le_bytes());
        } else {
            rsdp[16..20].copy_from_slice(&(ROOT as u32).to_le_bytes());
        }
        let v1 = rsdp[..20]
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        rsdp[8] = 0u8.wrapping_sub(v1);
        if revision >= 2 {
            let all = rsdp.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
            rsdp[32] = 0u8.wrapping_sub(all);
        }
        memory
    }

    fn parse_image(memory: &[u8]) -> AcpiInfo {
        let base = memory.as_ptr() as usize;
        parse(0x10, |phys: usize, len: usize| {
            (phys.checked_add(len)? <= memory.len()).then_some(base + phys)
        })
    }

    #[test]
    fn parse_retains_the_xsdt_oem_and_reports_a_validated_dmar() {
        let dmar = seal_sdt(dmar_table(&[drhd(1, 0, 0xFED9_0000)]), b"DMAR", b"BOCHS ");
        let info = parse_image(&firmware_image(2, b"BOCHS ", &[dmar]));
        assert_eq!(
            info.root_oem,
            Some(AcpiOem {
                oem_id: *b"BOCHS ",
                oem_table_id: *b"BXPC    ",
                oem_revision: 1,
            })
        );
        assert!(info.dmar_present);
        assert_eq!(info.dmar_base, 0xFED9_0000);
    }

    #[test]
    fn parse_retains_the_rsdt_oem_and_keeps_dmar_absent_without_a_table() {
        let info = parse_image(&firmware_image(0, b"OEMRSD", &[]));
        assert_eq!(info.root_oem.map(|oem| oem.oem_id), Some(*b"OEMRSD"));
        assert!(!info.dmar_present);
        assert_eq!(info.dmar_units, 0);
    }

    #[test]
    fn dmar_presence_is_independent_of_a_usable_unit() {
        let dmar = seal_sdt(dmar_table(&[drhd(1, 2, 0xFED9_0000)]), b"DMAR", b"BOCHS ");
        let info = parse_image(&firmware_image(2, b"BOCHS ", &[dmar]));
        assert!(info.dmar_present);
        assert_eq!(info.dmar_base, 0);
        assert_eq!(info.dmar_units, 1);

        // A DMAR that fails its checksum is not present.
        let mut corrupt = seal_sdt(dmar_table(&[drhd(1, 0, 0xFED9_0000)]), b"DMAR", b"BOCHS ");
        corrupt[40] ^= 0xFF;
        let info = parse_image(&firmware_image(2, b"BOCHS ", &[corrupt]));
        assert!(!info.dmar_present);
        assert_eq!(info.dmar_base, 0);
    }

    #[test]
    fn a_root_table_failing_its_checksum_yields_no_oem() {
        let mut memory = firmware_image(2, b"BOCHS ", &[]);
        memory[0x100 + 10] ^= 0x20;
        let info = parse_image(&memory);
        assert_eq!(info.root_oem, None);
        assert!(!info.dmar_present);

        let mut memory = firmware_image(0, b"BOCHS ", &[]);
        memory[0x100 + 16] ^= 0x20;
        assert_eq!(parse_image(&memory).root_oem, None);
    }
}
