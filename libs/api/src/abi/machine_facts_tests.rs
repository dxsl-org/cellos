// SPDX-License-Identifier: Apache-2.0
//! Host tests for the `ViMachineFactsV1` record.

use super::*;

/// Stand-in for SHA-256: the record layer never verifies the digest.
fn fold(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (index, byte) in bytes.iter().enumerate() {
        out[index % 32] ^= byte.wrapping_add(index as u8);
    }
    out
}

fn pci(vendor: u16, device: u16, class: u8) -> PciFunctionId {
    PciFunctionId { vendor, device, class, subclass: 0, prog_if: 0 }
}

fn sample() -> ViMachineFactsV1 {
    ViMachineFactsV1 {
        flags: FLAG_ACPI_OEM | FLAG_SMBIOS | FLAG_SMBIOS_UUID | FLAG_DMAR_PRESENT
            | FLAG_VTD_ACTIVE | FLAG_HYPERVISOR | FLAG_PCI_ECAM,
        profile_source: PROFILE_SOURCE_CMDLINE,
        slug: FactString::new(b"qemu-q35-x86_64").unwrap(),
        acpi_oem_revision: 1,
        acpi_oem_table_id: *b"BXPC    ",
        acpi_oem_id: *b"BOCHS ",
        smbios_uuid: [0x11; 16],
        smbios_manufacturer: FactString::new(b"QEMU").unwrap(),
        smbios_product: FactString::new(b"Standard PC (Q35 + ICH9, 2009)").unwrap(),
        pci: alloc::vec![pci(0x1af4, 0x1000, 2), pci(0x8086, 0x100e, 2), pci(0x8086, 0x29c0, 6)],
    }
}

fn encoded() -> Vec<u8> {
    sample().encode(fold).expect("sample is valid")
}

#[test]
fn layout_offsets_and_lengths_are_as_declared() {
    let bytes = encoded();
    assert_eq!(MACHINE_FACTS_CAPACITY, 2336);
    assert_eq!(bytes.len(), 288 + 8 * 3);
    assert_eq!(&bytes[0..2], &1u16.to_le_bytes());
    assert_eq!(&bytes[2..4], &0x7Fu16.to_le_bytes());
    assert_eq!(&bytes[4..8], &(bytes.len() as u32).to_le_bytes());
    assert_eq!(bytes[8], PROFILE_SOURCE_CMDLINE);
    assert_eq!(bytes[9], 15);
    assert_eq!(bytes[10], 4);
    assert_eq!(bytes[11], 30);
    assert_eq!(&bytes[12..14], &3u16.to_le_bytes());
    assert_eq!(&bytes[16..20], &1u32.to_le_bytes());
    assert_eq!(&bytes[24..32], b"BXPC    ");
    assert_eq!(&bytes[32..38], b"BOCHS ");
    assert_eq!(&bytes[40..56], &[0x11; 16]);
    assert_eq!(&bytes[56..71], b"qemu-q35-x86_64");
    assert_eq!(&bytes[120..124], b"QEMU");
    assert_eq!(&bytes[184..188], b"Stan");
    assert_eq!(&bytes[256..264], &[0xf4, 0x1a, 0x00, 0x10, 2, 0, 0, 0]);
    assert_eq!(&bytes[264..266], &0x8086u16.to_le_bytes());
    let body = bytes.len() - 32;
    assert_eq!(&bytes[body..], &fold(&bytes[..body]));
}

#[test]
fn round_trip_keeps_every_field_and_the_stored_digest() {
    let bytes = encoded();
    let (facts, digest) = ViMachineFactsV1::decode(&bytes).expect("decodes");
    assert_eq!(facts, sample());
    assert_eq!(digest, fold(&bytes[..bytes.len() - 32]));
    let record = MachineFactsRecord::decode(&bytes).expect("decodes");
    assert_eq!(record.bytes(), &bytes[..]);
    assert_eq!(record.digest(), digest);
    assert_eq!(record.facts().slug.as_bytes(), b"qemu-q35-x86_64");
}

#[test]
fn minimal_and_full_records_round_trip() {
    let minimal = ViMachineFactsV1 {
        flags: 0,
        profile_source: PROFILE_SOURCE_DEFAULT,
        slug: FactString::new(b"x86_64-pc").unwrap(),
        acpi_oem_revision: 0,
        acpi_oem_table_id: [0; 8],
        acpi_oem_id: [0; 6],
        smbios_uuid: [0; 16],
        smbios_manufacturer: FactString::EMPTY,
        smbios_product: FactString::EMPTY,
        pci: Vec::new(),
    };
    let bytes = minimal.encode(fold).unwrap();
    assert_eq!(bytes.len(), MACHINE_FACTS_MIN_LEN);
    assert_eq!(ViMachineFactsV1::decode(&bytes).unwrap().0, minimal);

    let mut full = sample();
    full.pci = (0..MACHINE_FACTS_PCI_MAX as u16).map(|n| pci(0x8086, n, 1)).collect();
    let bytes = full.encode(fold).unwrap();
    assert_eq!(bytes.len(), MACHINE_FACTS_CAPACITY);
    assert_eq!(ViMachineFactsV1::decode(&bytes).unwrap().0, full);
}

#[test]
fn encoder_refuses_values_the_decoder_would_refuse() {
    let refuse = |facts: ViMachineFactsV1, error| assert_eq!(facts.encode(fold), Err(error));
    let mut v = sample(); v.flags |= 1 << 7; refuse(v, MachineFactsError::UnknownFlag);
    let mut v = sample(); v.profile_source = 3; refuse(v, MachineFactsError::ProfileSource);
    let mut v = sample(); v.slug = FactString::EMPTY; refuse(v, MachineFactsError::Slug);
    let mut v = sample(); v.slug = FactString::new(b"Q35").unwrap(); refuse(v, MachineFactsError::Slug);
    let mut v = sample(); v.flags &= !FLAG_SMBIOS; refuse(v, MachineFactsError::Smbios);
    let mut v = sample(); v.flags &= !FLAG_SMBIOS_UUID; refuse(v, MachineFactsError::Smbios);
    let mut v = sample(); v.flags &= !FLAG_ACPI_OEM; refuse(v, MachineFactsError::Acpi);
    let mut v = sample(); v.flags &= !FLAG_PCI_ECAM; refuse(v, MachineFactsError::Pci);
    let mut v = sample(); v.pci.reverse(); refuse(v, MachineFactsError::Unsorted);
    let mut v = sample();
    v.pci = (0..=MACHINE_FACTS_PCI_MAX as u16).map(|n| pci(1, n, 0)).collect();
    refuse(v, MachineFactsError::Length);
    assert!(FactString::new(&[b'a'; 65]).is_none());
}

/// Each case mutates one field of a valid record and names the refusal.
#[test]
fn decoder_refuses_every_layout_violation() {
    let base = encoded();
    let refuse = |mutate: &dyn Fn(&mut Vec<u8>), error: MachineFactsError| {
        let mut bytes = base.clone();
        mutate(&mut bytes);
        assert_eq!(ViMachineFactsV1::decode(&bytes).map(|_| ()), Err(error), "{error:?}");
    };
    use MachineFactsError as E;
    refuse(&|b| b[0] = 2, E::UnsupportedVersion);
    refuse(&|b| b.truncate(1), E::Length);
    refuse(&|b| b.truncate(200), E::Length);
    refuse(&|b| b.push(0), E::Length);
    refuse(&|b| b[4] ^= 8, E::Length);
    refuse(&|b| b[12] = 4, E::Length);
    refuse(&|b| { b[12] = 0x01; b[13] = 0x01; }, E::Length);
    refuse(&|b| b[3] = 0x80, E::UnknownFlag);
    refuse(&|b| b[8] = 0, E::ProfileSource);
    refuse(&|b| b[8] = 3, E::ProfileSource);
    refuse(&|b| b[9] = 0, E::Slug);
    refuse(&|b| b[9] = 65, E::Slug);
    refuse(&|b| b[56] = b'Q', E::Slug);
    refuse(&|b| b[10] = 65, E::Smbios);
    refuse(&|b| b[11] = 65, E::Smbios);
    refuse(&|b| b[2] &= !(FLAG_SMBIOS as u8), E::Smbios);
    refuse(&|b| b[2] &= !(FLAG_SMBIOS_UUID as u8), E::Smbios);
    refuse(&|b| b[2] &= !(FLAG_ACPI_OEM as u8), E::Acpi);
    refuse(&|b| b[2] &= !(FLAG_PCI_ECAM as u8), E::Pci);
    refuse(&|b| b[14] = 1, E::Reserved);
    refuse(&|b| b[20] = 1, E::Reserved);
    refuse(&|b| b[38] = 1, E::Reserved);
    refuse(&|b| b[255] = 1, E::Reserved);
    refuse(&|b| b[56 + 15] = b'x', E::Reserved);
    refuse(&|b| b[120 + 63] = 1, E::Reserved);
    refuse(&|b| b[184 + 63] = 1, E::Reserved);
    refuse(&|b| b[256 + 7] = 1, E::Reserved);
    // Swap the first two PCI entries.
    refuse(&|b| { let (x, y) = (b[256..264].to_vec(), b[264..272].to_vec());
        b[256..264].copy_from_slice(&y); b[264..272].copy_from_slice(&x); }, E::Unsorted);
}

#[test]
fn smbios_uuid_requires_smbios_and_strings_require_the_flag() {
    let mut v = sample();
    v.flags &= !(FLAG_SMBIOS | FLAG_SMBIOS_UUID);
    v.smbios_uuid = [0; 16];
    v.smbios_manufacturer = FactString::EMPTY;
    v.smbios_product = FactString::EMPTY;
    let mut bytes = v.encode(fold).unwrap();
    assert!(ViMachineFactsV1::decode(&bytes).is_ok());
    bytes[2] |= FLAG_SMBIOS_UUID as u8;
    assert_eq!(ViMachineFactsV1::decode(&bytes).map(|_| ()), Err(MachineFactsError::Smbios));
}

#[test]
fn duplicate_pci_functions_are_a_multiset() {
    let mut v = sample();
    v.pci = alloc::vec![pci(0x8086, 0x100e, 2), pci(0x8086, 0x100e, 2)];
    let bytes = v.encode(fold).unwrap();
    assert_eq!(ViMachineFactsV1::decode(&bytes).unwrap().0.pci.len(), 2);
}
