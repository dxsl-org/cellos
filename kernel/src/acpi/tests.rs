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

// ── Phase 06: static `\_S5_` scan and power-fact discovery ─────────────────

use super::power::{discover, scan_definition_block, GpeBlock, S5};

/// The q35 DSDT QEMU 8.2.2 builds for `-machine q35,hpet=on -m 256M`
/// (captured with QMP `pmemsave`; OEM `BOCHS `/`BXPC`, 8390 bytes).
const Q35_DSDT: &[u8] = include_bytes!("../../../tests/fixtures/acpi/q35-dsdt.aml");
/// `Name (\_S5_, ..)` only inside `Method (_PTS)`; source beside the fixture.
const S5_IN_METHOD_DSDT: &[u8] =
    include_bytes!("../../../tests/fixtures/acpi/s5-in-method-dsdt.aml");
/// The DSDTs VMware Workstation 25.0.0 (virtual hardware 22) builds for the
/// S3 lane VM (2 vCPU, SATA): EFI at 512 MiB (37954 bytes) and BIOS at
/// 1024 MiB (66014 bytes), OEM `PTLTD `/`Custom  `. Carved from the `.vmem` of
/// the suspended guest by following RSDP → XSDT → FADT.
const VMWARE_EFI_DSDT: &[u8] = include_bytes!("../../../tests/fixtures/acpi/vmware-efi-dsdt.aml");
const VMWARE_BIOS_DSDT: &[u8] = include_bytes!("../../../tests/fixtures/acpi/vmware-bios-dsdt.aml");

fn definition_block(signature: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut table = vec![0u8; 36];
    table[8] = 2;
    table.extend_from_slice(body);
    seal_sdt(table, signature, b"CELLOS")
}

/// `Name (<name>, Package (4) { a, b, 0, 0 })` with byte-prefixed constants.
fn name_package(name: &[u8], a: u8, b: u8) -> Vec<u8> {
    let mut term = vec![0x08];
    term.extend_from_slice(name);
    term.extend_from_slice(&[0x12, 0x08, 0x04, 0x0A, a, 0x0A, b, 0x00, 0x00]);
    term
}

/// `op PkgLength <head> <body>` with a one-byte PkgLength.
fn package_term(op: &[u8], head: &[u8], body: &[u8]) -> Vec<u8> {
    let len = 1 + head.len() + body.len();
    assert!(len < 64);
    let mut term = op.to_vec();
    term.push(len as u8);
    term.extend_from_slice(head);
    term.extend_from_slice(body);
    term
}

#[test]
fn s5_scan_reads_the_q35_dsdt() {
    assert_eq!(&Q35_DSDT[..4], b"DSDT");
    assert_eq!(
        Q35_DSDT
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
        0
    );
    assert_eq!(
        scan_definition_block(Q35_DSDT),
        S5::Static {
            slp_typ_a: 0,
            slp_typ_b: 0
        }
    );
}

#[test]
fn s5_scan_refuses_a_name_created_inside_a_method() {
    let pattern = [0x08, b'\\', b'_', b'S', b'5', b'_', 0x12];
    assert!(S5_IN_METHOD_DSDT
        .windows(pattern.len())
        .any(|w| w == pattern));
    assert_eq!(scan_definition_block(S5_IN_METHOD_DSDT), S5::NotStatic);
}

#[test]
fn s5_scan_reads_the_vmware_dsdts() {
    for dsdt in [VMWARE_EFI_DSDT, VMWARE_BIOS_DSDT] {
        assert_eq!(&dsdt[..4], b"DSDT");
        assert_eq!(
            dsdt.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)),
            0
        );
        assert_eq!(
            scan_definition_block(dsdt),
            S5::Static {
                slp_typ_a: 0,
                slp_typ_b: 0
            }
        );
    }
}

#[test]
fn s5_scan_accepts_only_a_root_name_with_constant_slp_types() {
    let scan = |body: &[u8]| scan_definition_block(&definition_block(b"DSDT", body));
    let s5 = name_package(b"_S5_", 5, 7);
    let expected = S5::Static {
        slp_typ_a: 5,
        slp_typ_b: 7,
    };
    assert_eq!(scan(&s5), expected);
    assert_eq!(scan(&package_term(&[0x10], b"\\\0", &s5)), expected);
    // `^_S5_` from `\_SB_` names the root object.
    let parent = name_package(b"^_S5_", 5, 7);
    assert_eq!(scan(&package_term(&[0x10], b"_SB_", &parent)), expected);
    // `\_SB_.DEV0._S5_` is not the sleep object.
    let device = package_term(&[0x5B, 0x82], b"\\._SB_DEV0", &s5);
    assert_eq!(scan(&device), S5::Absent);
    // A method named `\_S5_` is computed.
    let method = package_term(&[0x14], b"\\_S5_\0", &[0xA4, 0x00]);
    assert_eq!(scan(&method), S5::NotStatic);
    // Conditional creation is not static.
    assert_eq!(scan(&package_term(&[0xA0], &[0x01], &s5)), S5::NotStatic);
    // SLP_TYP must be a constant of at most three bits.
    assert_eq!(scan(&name_package(b"_S5_", 8, 0)), S5::NotStatic);
    let reference = [
        0x08, b'_', b'S', b'5', b'_', 0x12, 0x07, 0x02, b'A', b'B', b'C', b'D', 0x00,
    ];
    assert_eq!(scan(&reference), S5::NotStatic);
    // An undecoded namespace-level opcode stops the scan before `\_S5_`.
    let mut store_first = vec![0x70, 0x00, 0x60];
    store_first.extend_from_slice(&s5);
    assert_eq!(
        scan(&store_first),
        S5::Unparsed {
            offset: 36,
            opcode: 0x70
        }
    );
    // `OperationRegion (REG0, SystemMemory, (ECFG + 0x4000), 0x4000)` (VMware
    // EFI): an integer expression whose result is discarded is skipped.
    let region = |target: &[u8]| {
        let mut term = b"\x5B\x80REG0\x00\x72ECFG\x0B\x00\x40".to_vec();
        term.extend_from_slice(target);
        term.extend_from_slice(&[0x0B, 0x00, 0x40]);
        term.extend_from_slice(&s5);
        term
    };
    assert_eq!(scan(&region(&[0x00])), expected);
    // One that stores its result while the table loads is not decoded.
    assert_eq!(
        scan(&region(b"\\_S5_")),
        S5::Unparsed {
            offset: 36,
            opcode: 0x5B
        }
    );
    // A name operand inside an expression must be followed by the next operand
    // (first position) or the `NullName` target (last position). Anything else
    // — here `Store`, as a method invocation's argument list could start — means
    // the name was not a plain reference, and the term is refused.
    for expression in [
        &b"\x72ECFG\x70\x0B\x00\x40\x00"[..],
        b"\x72\x0B\x00\x40ECFG\x70\x00",
    ] {
        let mut term = b"\x5B\x80REG0\x00".to_vec();
        term.extend_from_slice(expression);
        term.extend_from_slice(&[0x0B, 0x00, 0x40]);
        term.extend_from_slice(&s5);
        assert_eq!(
            scan(&term),
            S5::Unparsed {
                offset: 36,
                opcode: 0x5B
            }
        );
    }
    assert_eq!(scan(&[]), S5::Absent);
}

/// DSDT without `\_S5_`, an SSDT with it, and FADT GPE0 at 0x620 (16 bytes).
fn power_image() -> Vec<u8> {
    const DSDT_AT: usize = 0x3000;
    let mut fadt = vec![0u8; 244];
    fadt[8] = 3;
    fadt[40..44].copy_from_slice(&(DSDT_AT as u32).to_le_bytes());
    fadt[64..68].copy_from_slice(&0x604u32.to_le_bytes());
    fadt[80..84].copy_from_slice(&0x620u32.to_le_bytes());
    fadt[89] = 2;
    fadt[92] = 16;
    let fadt = seal_sdt(fadt, b"FACP", b"BOCHS ");
    let ssdt = definition_block(b"SSDT", &name_package(b"\\_S5_", 0, 0));
    let dsdt = definition_block(b"DSDT", &name_package(b"_S3_", 1, 1));
    let mut memory = firmware_image(2, b"BOCHS ", &[fadt, ssdt]);
    memory[DSDT_AT..DSDT_AT + dsdt.len()].copy_from_slice(&dsdt);
    memory
}

#[test]
fn discovery_scans_the_dsdt_then_listed_ssdts_and_reads_gpe_blocks() {
    let memory = power_image();
    let fadt = parse_image(&memory).0.fadt.expect("fadt parsed");
    let base = memory.as_ptr() as usize;
    let mut mapper =
        |phys: usize, len: usize| (phys.checked_add(len)? <= memory.len()).then_some(base + phys);
    let facts = discover(&fadt, 0x10, &mut mapper);
    assert_eq!(
        facts.s5,
        S5::Static {
            slp_typ_a: 0,
            slp_typ_b: 0
        }
    );
    assert_eq!(
        facts.gpe,
        [
            GpeBlock {
                port: 0x620,
                len: 16
            },
            GpeBlock::default()
        ]
    );
    assert_eq!((facts.reset_virt, facts.sleep_control_virt), (None, None));

    // A corrupt SSDT is skipped, leaving no `\_S5_`.
    let mut corrupt = power_image();
    let ssdt_at = 0x400 + 244usize.next_multiple_of(16);
    corrupt[ssdt_at + 40] ^= 0xFF;
    let base = corrupt.as_ptr() as usize;
    let mut mapper =
        |phys: usize, len: usize| (phys.checked_add(len)? <= corrupt.len()).then_some(base + phys);
    assert_eq!(discover(&fadt, 0x10, &mut mapper).s5, S5::Absent);
}
