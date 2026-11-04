// SPDX-License-Identifier: MPL-2.0
//! RSDP layouts, SDT validation and XSDT/RSDT iteration.

use super::records::AcpiTopology;
use super::{dmar, fadt, hpet, madt, mcfg, spcr, AcpiInfo, AcpiOem};

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
pub(super) struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    pub(super) revision: u8,
    pub(super) rsdt_address: u32,
}

/// RSDP v2 (revision ≥ 2) extension — layout after the v1 struct.
/// Total size: 36 bytes.
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub(super) struct RsdpV2 {
    v1: Rsdp,
    pub(super) length: u32,
    pub(super) xsdt_address: u64,
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
pub(super) unsafe fn read_unaligned<T: Copy>(virt: usize) -> T {
    // SAFETY: caller guarantees virt is valid.
    unsafe { core::ptr::read_unaligned(virt as *const T) }
}

/// Compute the byte checksum of a memory region (ACPI table validation).
///
/// # Safety
/// `[base, base + len)` must be readable virtual memory.
pub(super) unsafe fn table_checksum(base: usize, len: usize) -> u8 {
    let mut sum: u8 = 0;
    for i in 0..len {
        // SAFETY: caller guarantees [base, base+len) is readable.
        let byte = unsafe { core::ptr::read_volatile((base + i) as *const u8) };
        sum = sum.wrapping_add(byte);
    }
    sum
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
    topo: &mut AcpiTopology,
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
        dispatch_sdt(child_phys, mapper, info, topo);
    }
}

/// Iterate RSDT (32-bit pointer array) and dispatch each child SDT.
unsafe fn parse_rsdt(
    phys: usize,
    mapper: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
    topo: &mut AcpiTopology,
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
        dispatch_sdt(child_phys, mapper, info, topo);
    }
}

/// Validate and dispatch a child SDT by its 4-byte signature.
fn dispatch_sdt(
    phys: usize,
    mapper: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
    topo: &mut AcpiTopology,
) {
    let Some((virt, length)) = (unsafe { validate_sdt(phys, mapper) }) else {
        return;
    };
    // SAFETY: virt is the start of a validated SDT.
    let sig = unsafe { read_sig(virt) };

    match &sig {
        b"APIC" => madt::parse_madt(virt, length, info, topo),
        b"MCFG" => mcfg::parse_mcfg(virt, length, info, topo),
        b"HPET" => hpet::parse_hpet(virt, length, info),
        b"DMAR" => {
            info.dmar_present = true;
            dmar::parse_dmar(virt, length, info, topo)
        }
        b"FACP" => fadt::parse_fadt(virt, length, info),
        b"SPCR" => spcr::parse_spcr(virt, length, info),
        _ => {
            // Skip unknown/unneeded tables silently.
        }
    }
}

/// Validate the RSDP at `rsdp_phys` and walk the XSDT (ACPI 2.0+) or RSDT.
/// Every failure logs and returns with the gates it would have opened closed.
pub(super) fn walk(
    rsdp_phys: usize,
    map_physical: &mut impl FnMut(usize, usize) -> Option<usize>,
    info: &mut AcpiInfo,
    topo: &mut AcpiTopology,
) {
    if rsdp_phys == 0 {
        log::warn!("[acpi] RSDP physical address is null — hardware gates closed");
        return;
    }

    let Some(rsdp_virt) = map_physical(rsdp_phys, core::mem::size_of::<RsdpV2>()) else {
        log::warn!("[acpi] RSDP mapping failed — hardware gates closed");
        return;
    };

    // --- Validate RSDP signature ---
    // SAFETY: Limine guarantees the RSDP response contains a valid physical
    // address to the RSDP structure, mapped via HHDM. We read cautiously.
    let sig_bytes: [u8; 8] = unsafe { read_unaligned(rsdp_virt) };
    if &sig_bytes != b"RSD PTR " {
        log::warn!("[acpi] RSDP signature mismatch — hardware gates closed");
        return;
    }

    // --- Validate RSDP v1 checksum (bytes 0..19) ---
    // SAFETY: RSDP is at least 20 bytes (v1 size); Limine guarantees it.
    let cksum = unsafe { table_checksum(rsdp_virt, 20) };
    if cksum != 0 {
        log::warn!(
            "[acpi] RSDP v1 checksum failed ({}) — hardware gates closed",
            cksum
        );
        return;
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
            return;
        }
        let Some(rsdp_v2_virt) = map_physical(rsdp_phys, rsdp_length) else {
            log::warn!("[acpi] RSDP v2 mapping failed — hardware gates closed");
            return;
        };
        let extended_cksum = unsafe { table_checksum(rsdp_v2_virt, rsdp_length) };
        if extended_cksum != 0 {
            log::warn!(
                "[acpi] RSDP v2 checksum failed ({}) — hardware gates closed",
                extended_cksum
            );
            return;
        }

        // XSDT path: 64-bit table pointers.
        let rsdp_v2: RsdpV2 = unsafe { read_unaligned(rsdp_v2_virt) };
        let xsdt_phys = rsdp_v2.xsdt_address as usize;
        if xsdt_phys == 0 {
            log::warn!("[acpi] XSDT address is null — hardware gates closed");
            return;
        }
        // SAFETY: validate_sdt asks the mapper to cover the header and body
        // before either range is dereferenced.
        unsafe {
            parse_xsdt(xsdt_phys, map_physical, info, topo);
        }
    } else {
        // RSDT path: 32-bit table pointers.
        let rsdt_phys = rsdp.rsdt_address as usize;
        if rsdt_phys == 0 {
            log::warn!("[acpi] RSDT address is null — hardware gates closed");
            return;
        }
        // SAFETY: validate_sdt asks the mapper to cover the header and body
        // before either range is dereferenced.
        unsafe {
            parse_rsdt(rsdt_phys, map_physical, info, topo);
        }
    }
}
