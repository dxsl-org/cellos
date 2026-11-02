//! Host tests for the kernel machine facts encoder.

use super::encode;
use crate::acpi::AcpiOem;
use crate::board::{PlatformIdentity, ProfileSource, X86BootFacts};
use crate::smbios::{SmbiosString, SmbiosSystem};
use alloc::vec::Vec;
use api::machine_facts::{
    MachineFactsRecord, PciFunctionId, FLAGS_KNOWN, FLAG_DMAR_PRESENT, FLAG_PCI_ECAM,
    FLAG_VTD_ACTIVE, MACHINE_FACTS_PCI_MAX, PROFILE_SOURCE_CMDLINE, PROFILE_SOURCE_DEFAULT,
};

fn q35(source: ProfileSource) -> X86BootFacts {
    X86BootFacts {
        identity: PlatformIdentity {
            acpi_oem: Some(AcpiOem {
                oem_id: *b"BOCHS ",
                oem_table_id: *b"BXPC    ",
                oem_revision: 1,
            }),
            smbios: Some(SmbiosSystem {
                manufacturer: SmbiosString::new(b"QEMU"),
                product: SmbiosString::new(b"Standard PC (Q35 + ICH9, 2009)"),
                uuid: Some([0x42; 16]),
            }),
            hypervisor: true,
        },
        source,
        dmar_present: true,
    }
}

fn pci(vendor: u16, device: u16, class: u8) -> PciFunctionId {
    PciFunctionId {
        vendor,
        device,
        class,
        subclass: 0,
        prog_if: 0,
    }
}

fn devices() -> Vec<PciFunctionId> {
    alloc::vec![
        pci(0x8086, 0x29c0, 6),
        pci(0x1b36, 0x0010, 1),
        pci(0x8086, 0x100e, 2),
        pci(0x8086, 0x29c0, 6)
    ]
}

fn record(bytes: &[u8]) -> MachineFactsRecord {
    MachineFactsRecord::decode(bytes).expect("kernel output is a valid V1 record")
}

#[test]
fn record_carries_every_fact_and_a_sha256_of_its_body() {
    let bytes = encode(
        "qemu-q35-x86_64",
        &q35(ProfileSource::CommandLine),
        true,
        Some(devices()),
    )
    .expect("facts fit");
    let decoded = record(&bytes);
    let facts = decoded.facts();
    assert_eq!(facts.flags, FLAGS_KNOWN);
    assert_eq!(facts.profile_source, PROFILE_SOURCE_CMDLINE);
    assert_eq!(facts.slug.as_bytes(), b"qemu-q35-x86_64");
    assert_eq!(facts.acpi_oem_id, *b"BOCHS ");
    assert_eq!(
        facts.smbios_product.as_bytes(),
        b"Standard PC (Q35 + ICH9, 2009)"
    );
    assert_eq!(facts.smbios_uuid, [0x42; 16]);
    assert_eq!(facts.pci.len(), 4, "duplicates are kept: a multiset");
    let body = bytes.len() - 32;
    assert_eq!(decoded.digest(), crate::sha256::sha256(&bytes[..body]));
}

#[test]
fn pci_order_does_not_change_bytes_or_digest() {
    let facts = q35(ProfileSource::CommandLine);
    let forward = encode("qemu-q35-x86_64", &facts, true, Some(devices())).unwrap();
    let mut reversed_devices = devices();
    reversed_devices.reverse();
    let reversed = encode("qemu-q35-x86_64", &facts, true, Some(reversed_devices)).unwrap();
    assert_eq!(forward, reversed);
}

#[test]
fn each_input_changes_the_digest() {
    let base = record(
        &encode(
            "qemu-q35-x86_64",
            &q35(ProfileSource::CommandLine),
            true,
            Some(devices()),
        )
        .unwrap(),
    );
    let mut more = devices();
    more.push(pci(0x8086, 0x10d3, 2));
    let mut no_dmar = q35(ProfileSource::CommandLine);
    no_dmar.dmar_present = false;
    for changed in [
        encode(
            "qemu-q35-x86_64",
            &q35(ProfileSource::Default),
            true,
            Some(devices()),
        ),
        encode(
            "x86_64-pc",
            &q35(ProfileSource::CommandLine),
            true,
            Some(devices()),
        ),
        encode(
            "qemu-q35-x86_64",
            &q35(ProfileSource::CommandLine),
            false,
            Some(devices()),
        ),
        encode(
            "qemu-q35-x86_64",
            &q35(ProfileSource::CommandLine),
            true,
            Some(more),
        ),
        encode("qemu-q35-x86_64", &no_dmar, true, Some(devices())),
    ] {
        assert_ne!(record(&changed.unwrap()).digest(), base.digest());
    }
    let default = record(
        &encode(
            "x86_64-pc",
            &q35(ProfileSource::Default),
            true,
            Some(devices()),
        )
        .unwrap(),
    );
    assert_eq!(default.facts().profile_source, PROFILE_SOURCE_DEFAULT);
}

#[test]
fn absent_facts_clear_their_flags_and_fields() {
    let bare = X86BootFacts {
        identity: PlatformIdentity {
            acpi_oem: None,
            smbios: None,
            hypervisor: false,
        },
        source: ProfileSource::Default,
        dmar_present: false,
    };
    let decoded = record(&encode("x86_64-pc", &bare, false, None).unwrap());
    assert_eq!(decoded.facts().flags, 0);
    assert!(decoded.facts().pci.is_empty());
    let with_ecam = record(&encode("x86_64-pc", &bare, true, Some(Vec::new())).unwrap());
    assert_eq!(with_ecam.facts().flags, FLAG_PCI_ECAM | FLAG_VTD_ACTIVE);
    let mut dmar = bare;
    dmar.dmar_present = true;
    assert_eq!(
        record(&encode("x86_64-pc", &dmar, false, None).unwrap())
            .facts()
            .flags,
        FLAG_DMAR_PRESENT
    );
}

#[test]
fn more_functions_than_one_record_holds_is_no_facts() {
    let facts = q35(ProfileSource::CommandLine);
    let full: Vec<_> = (0..MACHINE_FACTS_PCI_MAX as u16)
        .map(|n| pci(0x8086, n, 1))
        .collect();
    assert!(encode("qemu-q35-x86_64", &facts, true, Some(full.clone())).is_some());
    let mut over = full;
    over.push(pci(0x8086, 0xffff, 1));
    assert_eq!(encode("qemu-q35-x86_64", &facts, true, Some(over)), None);
}
