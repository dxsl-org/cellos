//! Field-level difference between the pinned and the current machine facts.
//!
//! One line per changed field: `changed <field>: <old> -> <new>`. PCI changes
//! are multiset differences, `pci +VVVV:DDDD CC:SS:PP` / `pci -…`, at most
//! [`PCI_LINES_MAX`] of them plus `pci …and K more`. Firmware strings are
//! untrusted: they are bounded by the record and printed quoted, with `"`, `\`
//! and every byte outside printable ASCII escaped, so they cannot break a line
//! or forge another one.

use abi::machine_facts::{
    PciFunctionId, ViMachineFactsV1, FLAG_ACPI_OEM, FLAG_DMAR_PRESENT, FLAG_HYPERVISOR,
    FLAG_PCI_ECAM, FLAG_SMBIOS, FLAG_SMBIOS_UUID, FLAG_VTD_ACTIVE, PROFILE_SOURCE_CMDLINE,
};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

/// Most PCI lines printed before the rest are counted.
pub(crate) const PCI_LINES_MAX: usize = 16;

/// Lowercase hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// `bytes` quoted, with `"`, `\` and non-printable bytes escaped.
pub(crate) fn quoted(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &byte in bytes {
        let _ = match byte {
            b'"' | b'\\' => write!(out, "\\{}", byte as char),
            0x20..=0x7E => write!(out, "{}", byte as char),
            _ => write!(out, "\\x{byte:02x}"),
        };
    }
    out.push('"');
    out
}

fn flag(facts: &ViMachineFactsV1, bit: u16) -> bool {
    facts.flags & bit != 0
}

fn bit(facts: &ViMachineFactsV1, bit: u16) -> String {
    String::from(if flag(facts, bit) { "1" } else { "0" })
}

fn source(facts: &ViMachineFactsV1) -> String {
    String::from(if facts.profile_source == PROFILE_SOURCE_CMDLINE {
        "cmdline"
    } else {
        "default"
    })
}

fn when(present: bool, value: impl FnOnce() -> String) -> String {
    if present {
        value()
    } else {
        String::from("none")
    }
}

fn uuid(bytes: &[u8; 16]) -> String {
    let mut out = String::new();
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn pci_line(sign: char, entry: &PciFunctionId) -> String {
    format!(
        "pci {sign}{:04x}:{:04x} {:02x}:{:02x}:{:02x}",
        entry.vendor, entry.device, entry.class, entry.subclass, entry.prog_if
    )
}

/// Every difference, in field order, then the PCI multiset difference.
pub(crate) fn lines(old: &ViMachineFactsV1, new: &ViMachineFactsV1) -> Vec<String> {
    type Render = fn(&ViMachineFactsV1) -> String;
    let fields: [(&str, Render); 12] = [
        ("profile.slug", |f| quoted(f.slug.as_bytes())),
        ("profile.source", source),
        ("acpi.oem_id", |f| {
            when(flag(f, FLAG_ACPI_OEM), || quoted(&f.acpi_oem_id))
        }),
        ("acpi.oem_table_id", |f| {
            when(flag(f, FLAG_ACPI_OEM), || quoted(&f.acpi_oem_table_id))
        }),
        ("acpi.oem_revision", |f| {
            when(flag(f, FLAG_ACPI_OEM), || {
                format!("{:#x}", f.acpi_oem_revision)
            })
        }),
        ("smbios.manufacturer", |f| {
            when(flag(f, FLAG_SMBIOS), || {
                quoted(f.smbios_manufacturer.as_bytes())
            })
        }),
        ("smbios.product", |f| {
            when(flag(f, FLAG_SMBIOS), || quoted(f.smbios_product.as_bytes()))
        }),
        ("smbios.uuid", |f| {
            when(flag(f, FLAG_SMBIOS_UUID), || uuid(&f.smbios_uuid))
        }),
        ("dmar.present", |f| bit(f, FLAG_DMAR_PRESENT)),
        ("vtd.active", |f| bit(f, FLAG_VTD_ACTIVE)),
        ("cpu.hypervisor", |f| bit(f, FLAG_HYPERVISOR)),
        ("pci.ecam", |f| bit(f, FLAG_PCI_ECAM)),
    ];
    let mut out = Vec::new();
    for (name, render) in fields {
        let (before, after) = (render(old), render(new));
        if before != after {
            out.push(format!("changed {name}: {before} -> {after}"));
        }
    }
    let changes = pci_changes(&old.pci, &new.pci);
    let shown = changes.len().min(PCI_LINES_MAX);
    out.extend(
        changes[..shown]
            .iter()
            .map(|(sign, entry)| pci_line(*sign, entry)),
    );
    if changes.len() > shown {
        out.push(format!("pci …and {} more", changes.len() - shown));
    }
    out
}

/// Multiset difference of two ascending lists: `-` only in `old`, `+` only in
/// `new`, in ascending entry order.
fn pci_changes(old: &[PciFunctionId], new: &[PciFunctionId]) -> Vec<(char, PciFunctionId)> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < old.len() || j < new.len() {
        match (old.get(i), new.get(j)) {
            (Some(a), Some(b)) if a == b => {
                i += 1;
                j += 1;
            }
            (Some(a), Some(b)) if a < b => {
                out.push(('-', *a));
                i += 1;
            }
            (Some(a), None) => {
                out.push(('-', *a));
                i += 1;
            }
            (_, Some(b)) => {
                out.push(('+', *b));
                j += 1;
            }
            (None, None) => break,
        }
    }
    out
}
