//! Kernel producer of the machine facts record (syscall 433, ADR-0029 §3).
//!
//! [`query`] builds one canonical `api::machine_facts::ViMachineFactsV1` at
//! call time: the bound x86 profile and its source, the ACPI root OEM fields,
//! SMBIOS type 1, the DMAR/VT-d/hypervisor flags, and the multiset of
//! registered PCI functions re-read from config space, sorted without BDF.
//! The record ends in a SHA-256 of the bytes before it. It grants nothing and
//! changes nothing; init compares it with the stored pin.

use alloc::vec::Vec;

#[cfg(target_arch = "x86_64")]
use api::machine_facts as wire;

/// The current record, or `None` when there are no facts: not x86, no
/// profile bound yet, or more PCI functions than one record holds.
pub(crate) fn query() -> Option<Vec<u8>> {
    #[cfg(target_arch = "x86_64")]
    {
        let (board, facts) = crate::board::x86_machine_inputs()?;
        encode(
            board.slug,
            &facts,
            crate::dev::iommu::is_active(),
            crate::dev::pcie_ecam::fingerprint_functions(),
        )
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

/// Canonical record for these inputs. `pci` is `None` without an admitted
/// ECAM window; its order does not matter. `None` when a value does not fit
/// the record (more than `MACHINE_FACTS_PCI_MAX` functions, an invalid slug).
#[cfg(target_arch = "x86_64")]
fn encode(
    slug: &str,
    facts: &crate::board::X86BootFacts,
    vtd_active: bool,
    pci: Option<Vec<wire::PciFunctionId>>,
) -> Option<Vec<u8>> {
    let identity = &facts.identity;
    let mut flags = 0u16;
    let mut record = wire::ViMachineFactsV1 {
        flags: 0,
        profile_source: match facts.source {
            crate::board::ProfileSource::CommandLine => wire::PROFILE_SOURCE_CMDLINE,
            crate::board::ProfileSource::Default => wire::PROFILE_SOURCE_DEFAULT,
        },
        slug: wire::FactString::new(slug.as_bytes())?,
        acpi_oem_revision: 0,
        acpi_oem_table_id: [0; 8],
        acpi_oem_id: [0; 6],
        smbios_uuid: [0; 16],
        smbios_manufacturer: wire::FactString::EMPTY,
        smbios_product: wire::FactString::EMPTY,
        pci: Vec::new(),
    };
    if let Some(oem) = identity.acpi_oem {
        flags |= wire::FLAG_ACPI_OEM;
        record.acpi_oem_id = oem.oem_id;
        record.acpi_oem_table_id = oem.oem_table_id;
        record.acpi_oem_revision = oem.oem_revision;
    }
    if let Some(system) = identity.smbios {
        flags |= wire::FLAG_SMBIOS;
        record.smbios_manufacturer = wire::FactString::new(system.manufacturer.as_bytes())?;
        record.smbios_product = wire::FactString::new(system.product.as_bytes())?;
        if let Some(uuid) = system.uuid {
            flags |= wire::FLAG_SMBIOS_UUID;
            record.smbios_uuid = uuid;
        }
    }
    if facts.dmar_present {
        flags |= wire::FLAG_DMAR_PRESENT;
    }
    if vtd_active {
        flags |= wire::FLAG_VTD_ACTIVE;
    }
    if identity.hypervisor {
        flags |= wire::FLAG_HYPERVISOR;
    }
    if let Some(mut functions) = pci {
        if functions.len() > wire::MACHINE_FACTS_PCI_MAX {
            return None;
        }
        flags |= wire::FLAG_PCI_ECAM;
        functions.sort_unstable();
        record.pci = functions;
    }
    record.flags = flags;
    record.encode(crate::sha256::sha256).ok()
}

#[cfg(all(test, target_arch = "x86_64"))]
#[path = "machine_facts_tests.rs"]
mod tests;
