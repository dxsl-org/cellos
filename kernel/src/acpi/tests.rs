// SPDX-License-Identifier: MPL-2.0
use super::dmar::parse_dmar;
use super::mcfg::parse_mcfg;
use super::*;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

mod tables;

pub(super) fn topology() -> Box<AcpiTopology> {
    Box::new(AcpiTopology::EMPTY)
}

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
    parse_mcfg(
        table.as_ptr() as usize,
        table.len(),
        &mut info,
        &mut topology(),
    );
    assert_eq!(info.ecam_base, 0xB400_0000);
    assert_eq!(info.ecam_bus_start, 64);
    assert_eq!(info.ecam_bus_end, 65);
}

#[test]
fn mcfg_rejects_reversed_or_overflowing_allocations() {
    let reversed = mcfg_entry(0xB000_0000, 0, 65, 64);
    let mut info = AcpiInfo::default();
    parse_mcfg(
        reversed.as_ptr() as usize,
        reversed.len(),
        &mut info,
        &mut topology(),
    );
    assert_eq!(info.ecam_base, 0);

    let overflowing = mcfg_entry(u64::MAX & !0xF_FFFF, 0, 0, u8::MAX);
    parse_mcfg(
        overflowing.as_ptr() as usize,
        overflowing.len(),
        &mut info,
        &mut topology(),
    );
    assert_eq!(info.ecam_base, 0);
}

/// A DMAR body: 48-byte header (host_address_width, flags, reserved) then
/// the records.
pub(super) fn dmar_table(records: &[Vec<u8>]) -> Vec<u8> {
    let mut table = vec![0u8; 48];
    for record in records {
        table.extend_from_slice(record);
    }
    table
}

/// Type 0 DRHD with no device scopes (16 bytes).
pub(super) fn drhd(flags: u8, segment: u16, base: u64) -> Vec<u8> {
    let mut record = vec![0u8; 16];
    record[0..2].copy_from_slice(&0u16.to_le_bytes());
    record[2..4].copy_from_slice(&16u16.to_le_bytes());
    record[4] = flags;
    record[6..8].copy_from_slice(&segment.to_le_bytes());
    record[8..16].copy_from_slice(&base.to_le_bytes());
    record
}

/// Type 1 RMRR with no device scopes: ignored by the base decision.
fn rmrr() -> Vec<u8> {
    let mut record = vec![0xAAu8; 24];
    record[0..2].copy_from_slice(&1u16.to_le_bytes());
    record[2..4].copy_from_slice(&24u16.to_le_bytes());
    record
}

fn dmar(table: &[u8]) -> AcpiInfo {
    let mut info = AcpiInfo::default();
    parse_dmar(
        table.as_ptr() as usize,
        table.len(),
        &mut info,
        &mut topology(),
    );
    info
}

#[test]
fn dmar_prefers_the_all_inclusive_unit_and_skips_other_record_types() {
    let info = dmar(&dmar_table(&[
        rmrr(),
        drhd(0, 0, 0xAAAA_0000),
        drhd(1, 0, 0xFED9_0000),
    ]));
    assert_eq!(info.dmar_base, 0xFED9_0000);
    assert!(info.dmar_include_pci_all);
    assert_eq!(info.dmar_units, 2);
}

#[test]
fn dmar_accepts_a_scope_limited_unit_without_claiming_full_coverage() {
    let info = dmar(&dmar_table(&[drhd(0, 0, 0xFED9_0000)]));
    assert_eq!(info.dmar_base, 0xFED9_0000);
    assert!(!info.dmar_include_pci_all);
    assert_eq!(info.dmar_units, 1);
}

#[test]
fn dmar_ignores_other_segments_and_stops_at_a_malformed_record() {
    // A unit on another segment is counted but never selected.
    let info = dmar(&dmar_table(&[drhd(1, 2, 0xFED9_0000)]));
    assert_eq!(info.dmar_base, 0);
    assert_eq!(info.dmar_units, 1);

    // A record whose length runs past the table stops the walk: the base from
    // the valid record before it survives, and nothing is read out of bounds.
    let mut truncated = dmar_table(&[drhd(1, 0, 0xFED9_0000)]);
    truncated.extend_from_slice(&[0u8, 0, 0xFF, 0xFF]);
    assert_eq!(dmar(&truncated).dmar_base, 0xFED9_0000);

    // An undersized DRHD stops the walk without a base.
    let mut undersized = dmar_table(&[]);
    undersized.extend_from_slice(&[0u8, 0, 8, 0]);
    assert_eq!(dmar(&undersized).dmar_base, 0);
}

/// Stamp a common SDT header over `table[0..36]` and fix its checksum.
pub(super) fn seal_sdt(mut table: Vec<u8>, signature: &[u8; 4], oem_id: &[u8; 6]) -> Vec<u8> {
    let revision = table[8].max(1);
    let length = table.len() as u32;
    table[0..4].copy_from_slice(signature);
    table[4..8].copy_from_slice(&length.to_le_bytes());
    table[8] = revision;
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
pub(super) fn firmware_image(revision: u8, root_oem: &[u8; 6], children: &[Vec<u8>]) -> Vec<u8> {
    const RSDP: usize = 0x10;
    const ROOT: usize = 0x100;
    let mut memory = vec![0u8; 0x4000];
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

pub(super) fn parse_image(memory: &[u8]) -> (AcpiInfo, Box<AcpiTopology>) {
    let base = memory.as_ptr() as usize;
    let mut info = AcpiInfo::default();
    let mut topo = topology();
    let mut mapper =
        |phys: usize, len: usize| (phys.checked_add(len)? <= memory.len()).then_some(base + phys);
    parse_into(0x10, &mut mapper, &mut info, &mut topo);
    (info, topo)
}

#[test]
fn parse_retains_the_xsdt_oem_and_reports_a_validated_dmar() {
    let dmar = seal_sdt(dmar_table(&[drhd(1, 0, 0xFED9_0000)]), b"DMAR", b"BOCHS ");
    let (info, _) = parse_image(&firmware_image(2, b"BOCHS ", &[dmar]));
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
    let (info, _) = parse_image(&firmware_image(0, b"OEMRSD", &[]));
    assert_eq!(info.root_oem.map(|oem| oem.oem_id), Some(*b"OEMRSD"));
    assert!(!info.dmar_present);
    assert_eq!(info.dmar_units, 0);
}

#[test]
fn dmar_presence_is_independent_of_a_usable_unit() {
    let dmar = seal_sdt(dmar_table(&[drhd(1, 2, 0xFED9_0000)]), b"DMAR", b"BOCHS ");
    let (info, _) = parse_image(&firmware_image(2, b"BOCHS ", &[dmar]));
    assert!(info.dmar_present);
    assert_eq!(info.dmar_base, 0);
    assert_eq!(info.dmar_units, 1);

    // A DMAR that fails its checksum is not present.
    let mut corrupt = seal_sdt(dmar_table(&[drhd(1, 0, 0xFED9_0000)]), b"DMAR", b"BOCHS ");
    corrupt[40] ^= 0xFF;
    let (info, _) = parse_image(&firmware_image(2, b"BOCHS ", &[corrupt]));
    assert!(!info.dmar_present);
    assert_eq!(info.dmar_base, 0);
}

#[test]
fn a_root_table_failing_its_checksum_yields_no_oem() {
    let mut memory = firmware_image(2, b"BOCHS ", &[]);
    memory[0x100 + 10] ^= 0x20;
    let (info, _) = parse_image(&memory);
    assert_eq!(info.root_oem, None);
    assert!(!info.dmar_present);

    let mut memory = firmware_image(0, b"BOCHS ", &[]);
    memory[0x100 + 16] ^= 0x20;
    assert_eq!(parse_image(&memory).0.root_oem, None);
}
