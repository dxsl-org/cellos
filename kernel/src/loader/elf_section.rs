//! Bounded, byte-oriented ELF section-table access for untrusted cell images.

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ELFDATA2MSB: u8 = 2;
const SHT_STRTAB: u64 = 3;
pub(crate) const SHT_NOBITS: u64 = 8;
const SHN_XINDEX: u16 = 0xffff;

#[derive(Clone, Copy)]
pub(crate) struct Section {
    pub(crate) name: usize,
    pub(crate) kind: u64,
    pub(crate) offset: usize,
    pub(crate) size: usize,
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

/// Validated section-table view. File-backed ranges remain checked by callers:
/// `SHT_NOBITS` intentionally has no backing bytes in an ELF container.
pub(crate) struct ElfSections<'a> {
    data: &'a [u8],
    header: Header,
}

impl<'a> ElfSections<'a> {
    /// Parse an ELF with structurally bounded program and section tables.
    pub(crate) fn parse(data: &'a [u8]) -> Option<Self> {
        if data.get(..4)? != ELF_MAGIC || *data.get(6)? != 1 {
            return None;
        }
        let class = *data.get(4)?;
        let little = match *data.get(5)? {
            ELFDATA2LSB => true,
            ELFDATA2MSB => false,
            _ => return None,
        };
        let (header_size, phoff_at, phentsize_at, phnum_at, shoff_at, ehsize_at, shentsize_at, shnum_at, shstrndx_at, expected_phent, expected_shent) = match class {
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
        let header = Header {
            class,
            little,
            shoff: to_usize(read(data, shoff_at, width, little)?)?,
            shentsize: read(data, shentsize_at, 2, little)? as usize,
            shnum: read(data, shnum_at, 2, little)? as usize,
            shstrndx: read(data, shstrndx_at, 2, little)? as usize,
        };
        if header.shnum == 0 {
            return (header.shoff == 0 && header.shstrndx == 0).then_some(Self { data, header });
        }
        if header.shstrndx == SHN_XINDEX as usize
            || header.shoff == 0
            || header.shentsize != expected_shent
            || header.shstrndx == 0
            || header.shstrndx >= header.shnum
        {
            return None;
        }
        checked_range(data, header.shoff, header.shentsize.checked_mul(header.shnum)?)?;
        Some(Self { data, header })
    }

    pub(crate) fn count(&self) -> usize {
        self.header.shnum
    }

    pub(crate) fn section(&self, index: usize) -> Option<Section> {
        if index >= self.header.shnum {
            return None;
        }
        let base = self
            .header
            .shoff
            .checked_add(self.header.shentsize.checked_mul(index)?)?;
        let (offset_at, size_at, width) = if self.header.class == ELFCLASS32 {
            (16, 20, 4)
        } else {
            (24, 32, 8)
        };
        Some(Section {
            name: to_usize(read(self.data, base, 4, self.header.little)?)?,
            kind: read(self.data, base.checked_add(4)?, 4, self.header.little)?,
            offset: to_usize(read(self.data, base.checked_add(offset_at)?, width, self.header.little)?)?,
            size: to_usize(read(self.data, base.checked_add(size_at)?, width, self.header.little)?)?,
        })
    }

    pub(crate) fn names(&self) -> Option<&'a [u8]> {
        let header = self.section(self.header.shstrndx)?;
        if header.kind != SHT_STRTAB {
            return None;
        }
        self.bytes(header)
    }

    pub(crate) fn name<'b>(&self, names: &'b [u8], section: Section) -> Option<&'b [u8]> {
        let tail = names.get(section.name..)?;
        let end = tail.iter().position(|byte| *byte == 0)?;
        Some(&tail[..end])
    }

    pub(crate) fn bytes(&self, section: Section) -> Option<&'a [u8]> {
        checked_range(self.data, section.offset, section.size)
    }
}

fn checked_range(data: &[u8], offset: usize, size: usize) -> Option<&[u8]> {
    data.get(offset..offset.checked_add(size)?)
}

fn read(data: &[u8], offset: usize, width: usize, little: bool) -> Option<u64> {
    let bytes = data.get(offset..offset.checked_add(width)?)?;
    let mut value = 0u64;
    if little {
        for (index, byte) in bytes.iter().enumerate() {
            value |= (*byte as u64) << (index * 8);
        }
    } else {
        for byte in bytes {
            value = (value << 8) | *byte as u64;
        }
    }
    Some(value)
}

fn to_usize(value: u64) -> Option<usize> {
    usize::try_from(value).ok()
}
