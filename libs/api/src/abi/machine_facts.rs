// SPDX-License-Identifier: Apache-2.0
//! Machine facts record (`QueryMachineFacts = 433`; ADR-0029 §3).
//!
//! [`ViMachineFactsV1`] is a class K record (`docs/abi-boundary-records.md`):
//! syscall 433 plus an exact out length of [`MACHINE_FACTS_CAPACITY`], version
//! exactly 1, `total_len` equal to the bytes present, flag bits outside
//! [`FLAGS_KNOWN`] refused, every reserved byte zero, little-endian in declared
//! field order. Producer: the kernel. Consumer: init, which also persists the
//! exact bytes as the machine pin (class P, `/srv/.cellos-machine/pin`).
//!
//! The record carries no authority. It detects change: profile, ACPI root OEM,
//! SMBIOS type 1, DMAR/VT-d state, hypervisor bit and the PCI function
//! multiset. The trailing SHA-256 is a display and identity handle, not an
//! integrity check: this crate has no hash, so [`ViMachineFactsV1::decode`]
//! never verifies it, and consumers compare whole records byte for byte.
//!
//! Layout (`off`, size): 0 `version` u16 · 2 `flags` u16 · 4 `total_len` u32 ·
//! 8 `profile_source` u8 · 9 `slug_len` u8 · 10 `smbios_manufacturer_len` u8 ·
//! 11 `smbios_product_len` u8 · 12 `pci_count` u16 · 14 reserved u16 ·
//! 16 `acpi_oem_revision` u32 · 20 reserved u32 · 24 `acpi_oem_table_id` [8] ·
//! 32 `acpi_oem_id` [6] · 38 reserved [2] · 40 `smbios_uuid` [16] ·
//! 56 `slug` [64] · 120 `smbios_manufacturer` [64] · 184 `smbios_product` [64] ·
//! 248 reserved [8] · 256 `pci[n]` 8 bytes each · 256+8n `digest` [32].

use alloc::vec::Vec;

/// The only record version this layout writes and accepts.
pub const MACHINE_FACTS_VERSION: u16 = 1;
/// Most PCI functions one record holds. More than this is "no facts", never a
/// truncated set.
pub const MACHINE_FACTS_PCI_MAX: usize = 256;
/// Bytes before the PCI entries.
pub const MACHINE_FACTS_HEADER_LEN: usize = 256;
/// Bytes per PCI entry.
pub const MACHINE_FACTS_PCI_ENTRY_LEN: usize = 8;
/// Bytes of the trailing SHA-256.
pub const MACHINE_FACTS_DIGEST_LEN: usize = 32;
/// Shortest record: no PCI entries.
pub const MACHINE_FACTS_MIN_LEN: usize = MACHINE_FACTS_HEADER_LEN + MACHINE_FACTS_DIGEST_LEN;
/// Longest record, and the exact `out_len` syscall 433 accepts.
pub const MACHINE_FACTS_CAPACITY: usize =
    MACHINE_FACTS_MIN_LEN + MACHINE_FACTS_PCI_ENTRY_LEN * MACHINE_FACTS_PCI_MAX;
/// Bytes of each fixed string field.
pub const MACHINE_FACTS_STRING_MAX: usize = 64;

/// The ACPI root OEM fields are present.
pub const FLAG_ACPI_OEM: u16 = 1 << 0;
/// SMBIOS type 1 manufacturer and product are present.
pub const FLAG_SMBIOS: u16 = 1 << 1;
/// The SMBIOS UUID is present. Requires [`FLAG_SMBIOS`].
pub const FLAG_SMBIOS_UUID: u16 = 1 << 2;
/// Firmware published a DMAR table.
pub const FLAG_DMAR_PRESENT: u16 = 1 << 3;
/// VT-d DMA translation is active.
pub const FLAG_VTD_ACTIVE: u16 = 1 << 4;
/// `CPUID.1:ECX[31]`: a hypervisor reports itself.
pub const FLAG_HYPERVISOR: u16 = 1 << 5;
/// A validated MCFG window exists, so the PCI entries were read from config space.
pub const FLAG_PCI_ECAM: u16 = 1 << 6;
/// Every defined flag bit.
pub const FLAGS_KNOWN: u16 = 0x7F;

/// `profile_source`: the Limine command line named the profile.
pub const PROFILE_SOURCE_CMDLINE: u8 = 1;
/// `profile_source`: no option, so the strict default profile applies.
pub const PROFILE_SOURCE_DEFAULT: u8 = 2;

const PCI_ENTRIES_AT: usize = MACHINE_FACTS_HEADER_LEN;

/// Why bytes or a value are not a valid V1 record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineFactsError {
    /// Shorter than the fixed part, `total_len` disagrees with the bytes present
    /// or with `pci_count`, or more than [`MACHINE_FACTS_PCI_MAX`] entries.
    Length,
    /// `version` is not [`MACHINE_FACTS_VERSION`].
    UnsupportedVersion,
    /// `flags` has a bit outside [`FLAGS_KNOWN`].
    UnknownFlag,
    /// `profile_source` is neither command line nor default.
    ProfileSource,
    /// `slug` is empty, longer than 64 bytes, or outside `[a-z0-9_-]`.
    Slug,
    /// SMBIOS fields disagree with [`FLAG_SMBIOS`]/[`FLAG_SMBIOS_UUID`].
    Smbios,
    /// ACPI fields are non-zero without [`FLAG_ACPI_OEM`].
    Acpi,
    /// PCI entries without [`FLAG_PCI_ECAM`].
    Pci,
    /// PCI entries are not in ascending order.
    Unsorted,
    /// A reserved byte, or a string byte past its length, is non-zero.
    Reserved,
}

/// A fixed-capacity string field: the first `len` bytes of `bytes` are the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactString {
    len: u8,
    bytes: [u8; MACHINE_FACTS_STRING_MAX],
}

impl FactString {
    /// The empty string.
    pub const EMPTY: Self = Self { len: 0, bytes: [0; MACHINE_FACTS_STRING_MAX] };

    /// `value` if it fits; `None` past 64 bytes.
    pub fn new(value: &[u8]) -> Option<Self> {
        if value.len() > MACHINE_FACTS_STRING_MAX {
            return None;
        }
        let mut bytes = [0; MACHINE_FACTS_STRING_MAX];
        bytes[..value.len()].copy_from_slice(value);
        Some(Self { len: value.len() as u8, bytes })
    }

    /// The value bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

/// One PCI function, without its BDF. Field order is the sort order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PciFunctionId {
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

/// The decoded machine facts, version 1. `version`, `total_len`, the lengths
/// and `pci_count` are implied by the fields; the digest travels beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViMachineFactsV1 {
    /// Bits from [`FLAGS_KNOWN`].
    pub flags: u16,
    /// [`PROFILE_SOURCE_CMDLINE`] or [`PROFILE_SOURCE_DEFAULT`].
    pub profile_source: u8,
    /// Bound catalog slug.
    pub slug: FactString,
    /// ACPI root table OEM revision; zero without [`FLAG_ACPI_OEM`].
    pub acpi_oem_revision: u32,
    /// ACPI root table OEM table id; zero without [`FLAG_ACPI_OEM`].
    pub acpi_oem_table_id: [u8; 8],
    /// ACPI root table OEM id; zero without [`FLAG_ACPI_OEM`].
    pub acpi_oem_id: [u8; 6],
    /// SMBIOS type 1 UUID, RFC 4122 order; zero without [`FLAG_SMBIOS_UUID`].
    pub smbios_uuid: [u8; 16],
    /// Raw firmware bytes; empty without [`FLAG_SMBIOS`].
    pub smbios_manufacturer: FactString,
    /// Raw firmware bytes; empty without [`FLAG_SMBIOS`].
    pub smbios_product: FactString,
    /// Ascending multiset; empty without [`FLAG_PCI_ECAM`].
    pub pci: Vec<PciFunctionId>,
}

fn le(bytes: &[u8], at: usize, n: usize) -> u64 {
    bytes[at..at + n].iter().rev().fold(0u64, |acc, b| acc << 8 | u64::from(*b))
}

fn all_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}

fn slug_valid(slug: &[u8]) -> bool {
    !slug.is_empty()
        && slug.iter().all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}

impl ViMachineFactsV1 {
    /// Exact length of this value's record.
    pub fn record_len(&self) -> usize {
        MACHINE_FACTS_MIN_LEN + MACHINE_FACTS_PCI_ENTRY_LEN * self.pci.len()
    }

    /// Every rule of the layout, applied to a value.
    fn validate(&self) -> Result<(), MachineFactsError> {
        if self.pci.len() > MACHINE_FACTS_PCI_MAX {
            return Err(MachineFactsError::Length);
        }
        if self.flags & !FLAGS_KNOWN != 0 {
            return Err(MachineFactsError::UnknownFlag);
        }
        if !matches!(self.profile_source, PROFILE_SOURCE_CMDLINE | PROFILE_SOURCE_DEFAULT) {
            return Err(MachineFactsError::ProfileSource);
        }
        if !slug_valid(self.slug.as_bytes()) {
            return Err(MachineFactsError::Slug);
        }
        let smbios = self.flags & FLAG_SMBIOS != 0;
        let uuid = self.flags & FLAG_SMBIOS_UUID != 0;
        if (uuid && !smbios)
            || (!smbios && !(self.smbios_manufacturer.len == 0 && self.smbios_product.len == 0))
            || (!uuid && !all_zero(&self.smbios_uuid))
        {
            return Err(MachineFactsError::Smbios);
        }
        if self.flags & FLAG_ACPI_OEM == 0
            && (self.acpi_oem_revision != 0
                || !all_zero(&self.acpi_oem_table_id)
                || !all_zero(&self.acpi_oem_id))
        {
            return Err(MachineFactsError::Acpi);
        }
        if self.flags & FLAG_PCI_ECAM == 0 && !self.pci.is_empty() {
            return Err(MachineFactsError::Pci);
        }
        if self.pci.windows(2).any(|pair| pair[0] > pair[1]) {
            return Err(MachineFactsError::Unsorted);
        }
        for text in [&self.slug, &self.smbios_manufacturer, &self.smbios_product] {
            if !all_zero(&text.bytes[usize::from(text.len)..]) {
                return Err(MachineFactsError::Reserved);
            }
        }
        Ok(())
    }

    /// Validated wire bytes, ending in `digest(bytes before the digest)`.
    ///
    /// # Errors
    /// Any rule of the layout the value breaks; nothing is written then.
    pub fn encode(&self, digest: impl FnOnce(&[u8]) -> [u8; 32]) -> Result<Vec<u8>, MachineFactsError> {
        self.validate()?;
        let total = self.record_len();
        let mut out = alloc::vec![0u8; total];
        out[0..2].copy_from_slice(&MACHINE_FACTS_VERSION.to_le_bytes());
        out[2..4].copy_from_slice(&self.flags.to_le_bytes());
        out[4..8].copy_from_slice(&(total as u32).to_le_bytes());
        out[8] = self.profile_source;
        out[9] = self.slug.len;
        out[10] = self.smbios_manufacturer.len;
        out[11] = self.smbios_product.len;
        out[12..14].copy_from_slice(&(self.pci.len() as u16).to_le_bytes());
        out[16..20].copy_from_slice(&self.acpi_oem_revision.to_le_bytes());
        out[24..32].copy_from_slice(&self.acpi_oem_table_id);
        out[32..38].copy_from_slice(&self.acpi_oem_id);
        out[40..56].copy_from_slice(&self.smbios_uuid);
        out[56..120].copy_from_slice(&self.slug.bytes);
        out[120..184].copy_from_slice(&self.smbios_manufacturer.bytes);
        out[184..248].copy_from_slice(&self.smbios_product.bytes);
        for (index, entry) in self.pci.iter().enumerate() {
            let at = PCI_ENTRIES_AT + index * MACHINE_FACTS_PCI_ENTRY_LEN;
            out[at..at + 2].copy_from_slice(&entry.vendor.to_le_bytes());
            out[at + 2..at + 4].copy_from_slice(&entry.device.to_le_bytes());
            out[at + 4] = entry.class;
            out[at + 5] = entry.subclass;
            out[at + 6] = entry.prog_if;
        }
        let body = total - MACHINE_FACTS_DIGEST_LEN;
        let sealed = digest(&out[..body]);
        out[body..].copy_from_slice(&sealed);
        Ok(out)
    }

    /// Decode and fully validate wire bytes. Returns the facts and the digest
    /// field as stored; the digest is not verified (see the module contract).
    ///
    /// # Errors
    /// The first rule the bytes break, before any field is returned.
    pub fn decode(bytes: &[u8]) -> Result<(Self, [u8; 32]), MachineFactsError> {
        if bytes.len() < 2 {
            return Err(MachineFactsError::Length);
        }
        if le(bytes, 0, 2) as u16 != MACHINE_FACTS_VERSION {
            return Err(MachineFactsError::UnsupportedVersion);
        }
        if bytes.len() < MACHINE_FACTS_MIN_LEN {
            return Err(MachineFactsError::Length);
        }
        let pci_count = le(bytes, 12, 2) as usize;
        let total = le(bytes, 4, 4) as usize;
        if pci_count > MACHINE_FACTS_PCI_MAX
            || total != bytes.len()
            || total != MACHINE_FACTS_MIN_LEN + MACHINE_FACTS_PCI_ENTRY_LEN * pci_count
        {
            return Err(MachineFactsError::Length);
        }
        if !all_zero(&bytes[14..16]) || !all_zero(&bytes[20..24]) || !all_zero(&bytes[38..40])
            || !all_zero(&bytes[248..256])
        {
            return Err(MachineFactsError::Reserved);
        }
        let flags = le(bytes, 2, 2) as u16;
        if flags & !FLAGS_KNOWN != 0 {
            return Err(MachineFactsError::UnknownFlag);
        }
        let text = |len_at: usize, at: usize, error| {
            let len = usize::from(bytes[len_at]);
            if len > MACHINE_FACTS_STRING_MAX {
                return Err(error);
            }
            let mut field = FactString::EMPTY;
            field.len = len as u8;
            field.bytes.copy_from_slice(&bytes[at..at + MACHINE_FACTS_STRING_MAX]);
            Ok(field)
        };
        let mut pci = Vec::with_capacity(pci_count);
        for index in 0..pci_count {
            let at = PCI_ENTRIES_AT + index * MACHINE_FACTS_PCI_ENTRY_LEN;
            if bytes[at + 7] != 0 {
                return Err(MachineFactsError::Reserved);
            }
            pci.push(PciFunctionId {
                vendor: le(bytes, at, 2) as u16,
                device: le(bytes, at + 2, 2) as u16,
                class: bytes[at + 4],
                subclass: bytes[at + 5],
                prog_if: bytes[at + 6],
            });
        }
        let facts = Self {
            flags,
            profile_source: bytes[8],
            slug: text(9, 56, MachineFactsError::Slug)?,
            acpi_oem_revision: le(bytes, 16, 4) as u32,
            acpi_oem_table_id: bytes[24..32].try_into().map_err(|_| MachineFactsError::Length)?,
            acpi_oem_id: bytes[32..38].try_into().map_err(|_| MachineFactsError::Length)?,
            smbios_uuid: bytes[40..56].try_into().map_err(|_| MachineFactsError::Length)?,
            smbios_manufacturer: text(10, 120, MachineFactsError::Smbios)?,
            smbios_product: text(11, 184, MachineFactsError::Smbios)?,
            pci,
        };
        facts.validate()?;
        let digest = bytes[total - MACHINE_FACTS_DIGEST_LEN..]
            .try_into()
            .map_err(|_| MachineFactsError::Length)?;
        Ok((facts, digest))
    }
}

/// A decoded record together with the exact bytes it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineFactsRecord {
    bytes: Vec<u8>,
    facts: ViMachineFactsV1,
    digest: [u8; 32],
}

impl MachineFactsRecord {
    /// Decode `bytes`, keeping a copy of them.
    ///
    /// # Errors
    /// As [`ViMachineFactsV1::decode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, MachineFactsError> {
        let (facts, digest) = ViMachineFactsV1::decode(bytes)?;
        Ok(Self { bytes: bytes.to_vec(), facts, digest })
    }

    /// The exact record bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The decoded fields.
    pub fn facts(&self) -> &ViMachineFactsV1 {
        &self.facts
    }

    /// The stored digest field.
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

#[cfg(test)]
#[path = "machine_facts_tests.rs"]
mod tests;
