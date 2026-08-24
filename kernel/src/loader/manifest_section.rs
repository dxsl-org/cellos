//! Bounded, allocation-free classification of the manifest ELF section.
use super::elf_section::{ElfSections, SHT_NOBITS};
use api::manifest::{CellManifest, MANIFEST_VERSION, MANIFEST_VERSION_V1};

const MANIFEST_NAME: &[u8] = b"__ViCell_manifest";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ManifestVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ManifestSection {
    Absent,
    Valid {
        manifest: CellManifest,
        version: ManifestVersion,
    },
    Malformed,
}

pub(super) fn classify(data: &[u8]) -> ManifestSection {
    classify_inner(data).unwrap_or(ManifestSection::Malformed)
}

fn classify_inner(data: &[u8]) -> Option<ManifestSection> {
    let elf = ElfSections::parse(data)?;
    if elf.count() == 0 {
        return Some(ManifestSection::Absent);
    }
    let names = elf.names()?;
    let mut found = None;
    for index in 0..elf.count() {
        let section = elf.section(index)?;
        let name = elf.name(names, section)?;
        if section.kind != SHT_NOBITS {
            elf.bytes(section)?;
        }
        if name != MANIFEST_NAME {
            continue;
        }
        // A named manifest must be backed by bytes in the ELF container.
        if section.kind == SHT_NOBITS || found.is_some() {
            return Some(ManifestSection::Malformed);
        }
        let bytes = elf.bytes(section)?;
        let version = match (bytes.len(), bytes.get(4).copied()) {
            (8, Some(MANIFEST_VERSION_V1)) => ManifestVersion::V1,
            (16, Some(MANIFEST_VERSION)) => ManifestVersion::V2,
            _ => return Some(ManifestSection::Malformed),
        };
        let manifest = match CellManifest::from_bytes(bytes) {
            Some(manifest) => manifest,
            None => return Some(ManifestSection::Malformed),
        };
        found = Some(ManifestSection::Valid { manifest, version });
    }
    Some(found.unwrap_or(ManifestSection::Absent))
}
