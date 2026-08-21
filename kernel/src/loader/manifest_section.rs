//! Bounded, allocation-free classification of the manifest ELF section.
use api::manifest::{CellManifest, MANIFEST_VERSION, MANIFEST_VERSION_V1};
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ELFDATA2MSB: u8 = 2;
const SHT_NOBITS: u64 = 8;
const SHN_XINDEX: u16 = 0xffff;
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
#[derive(Clone, Copy)]
struct Header {
    class: u8,
    little: bool,
    shoff: usize,
    shentsize: usize,
    shnum: usize,
    shstrndx: usize,
}
pub(super) fn classify(data: &[u8]) -> ManifestSection {
    classify_inner(data).unwrap_or(ManifestSection::Malformed)
}
fn classify_inner(data: &[u8]) -> Option<ManifestSection> {
    if data.get(..4)? != ELF_MAGIC || *data.get(6)? != 1 {
        return None;
    }
    let class = *data.get(4)?;
    let little = match *data.get(5)? {
        ELFDATA2LSB => true,
        ELFDATA2MSB => false,
        _ => return None,
    };
    let (
        header_size,
        phoff_at,
        phentsize_at,
        phnum_at,
        shoff_at,
        ehsize_at,
        shentsize_at,
        shnum_at,
        shstrndx_at,
        expected_phent,
        expected_shent,
    ) = match class {
        ELFCLASS32 => (52, 28, 42, 44, 32, 40, 46, 48, 50, 32, 40),
        ELFCLASS64 => (64, 32, 54, 56, 40, 52, 58, 60, 62, 56, 64),
        _ => return None,
    };
    if data.len() < header_size
        || read(data, 20, 4, little)? != 1
        || read(data, ehsize_at, 2, little)? as usize != header_size
    {
        return None;
    }
    let width = if class == ELFCLASS32 { 4 } else { 8 };
    let phoff = to_usize(read(data, phoff_at, width, little)?)?;
    let phentsize = read(data, phentsize_at, 2, little)? as usize;
    let phnum = read(data, phnum_at, 2, little)? as usize;
    if phnum == 0 {
        if phoff != 0 {
            return None;
        }
    } else {
        if phnum == 0xffff || phoff == 0 || phentsize != expected_phent {
            return None;
        }
        checked_range(data, phoff, phentsize.checked_mul(phnum)?)?;
        for index in 0..phnum {
            let base = phoff.checked_add(phentsize.checked_mul(index)?)?;
            let (offset_at, size_at) = if class == ELFCLASS32 { (4, 16) } else { (8, 32) };
            let kind = read(data, base, 4, little)?;
            let offset = to_usize(read(data, base.checked_add(offset_at)?, width, little)?)?;
            let size = to_usize(read(data, base.checked_add(size_at)?, width, little)?)?;
            if kind != 0 && size != 0 {
                checked_range(data, offset, size)?;
            }
        }
    }
    let h = Header {
        class,
        little,
        shoff: to_usize(read(data, shoff_at, width, little)?)?,
        shentsize: read(data, shentsize_at, 2, little)? as usize,
        shnum: read(data, shnum_at, 2, little)? as usize,
        shstrndx: read(data, shstrndx_at, 2, little)? as usize,
    };

    if h.shnum == 0 {
        // A zero section table is valid only in its unambiguous no-table form.
        // A non-zero offset denotes ELF extended numbering, which is unsupported.
        return if h.shoff == 0 && h.shstrndx == 0 {
            Some(ManifestSection::Absent)
        } else {
            None
        };
    }
    if h.shstrndx == SHN_XINDEX as usize
        || h.shoff == 0
        || h.shentsize != expected_shent
        || h.shstrndx == 0
        || h.shstrndx >= h.shnum
    {
        return None;
    }
    checked_range(data, h.shoff, h.shentsize.checked_mul(h.shnum)?)?;

    let str_header = section_header(data, h, h.shstrndx)?;
    if str_header.kind != 3 {
        return None;
    }
    let names = checked_range(data, str_header.offset, str_header.size)?;
    let mut found = None;
    for index in 0..h.shnum {
        let section = section_header(data, h, index)?;
        let name = section_name(names, section.name)?;
        if section.kind != SHT_NOBITS {
            checked_range(data, section.offset, section.size)?;
        }
        if name == MANIFEST_NAME {
            // SHT_NOBITS has no file-backed payload by definition. A named
            // manifest must therefore be rejected categorically, even when its
            // offset happens to point at bytes elsewhere in the ELF image.
            if section.kind == SHT_NOBITS {
                return Some(ManifestSection::Malformed);
            }
            if found.is_some() {
                return Some(ManifestSection::Malformed);
            }
            let bytes = checked_range(data, section.offset, section.size)?;
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
    }
    Some(found.unwrap_or(ManifestSection::Absent))
}
#[derive(Clone, Copy)]
struct Section {
    name: usize,
    kind: u64,
    offset: usize,
    size: usize,
}

fn section_header(data: &[u8], h: Header, index: usize) -> Option<Section> {
    let base = h.shoff.checked_add(h.shentsize.checked_mul(index)?)?;
    let (offset_at, size_at, width) = if h.class == ELFCLASS32 {
        (16, 20, 4)
    } else {
        (24, 32, 8)
    };
    Some(Section {
        name: to_usize(read(data, base, 4, h.little)?)?,
        kind: read(data, base.checked_add(4)?, 4, h.little)?,
        offset: to_usize(read(data, base.checked_add(offset_at)?, width, h.little)?)?,
        size: to_usize(read(data, base.checked_add(size_at)?, width, h.little)?)?,
    })
}

fn section_name(names: &[u8], offset: usize) -> Option<&[u8]> {
    let tail = names.get(offset..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    Some(&tail[..end])
}

fn checked_range(data: &[u8], offset: usize, size: usize) -> Option<&[u8]> {
    data.get(offset..offset.checked_add(size)?)
}

fn read(data: &[u8], offset: usize, width: usize, little: bool) -> Option<u64> {
    let bytes = data.get(offset..offset.checked_add(width)?)?;
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        let shift = if little { index * 8 } else { (width - index - 1) * 8 };
        value |= (*byte as u64) << shift;
    }
    Some(value)
}

fn to_usize(value: u64) -> Option<usize> {
    usize::try_from(value).ok()
}
