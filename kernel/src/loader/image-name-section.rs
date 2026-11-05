//! Bounded read of the `__ViCell_image` program-name section, and the
//! measured identity an admitted image is published with.
//!
//! The signing tool emits the section into the bytes it signs. The name feeds
//! `ImageIdentity`, which VFS matches path rules against, so it is read with
//! the duplicate-refusing `ElfSections` parser rather than
//! `ElfLoader::get_section` (first match wins, no duplicate check).
use super::elf_section::{ElfSections, SHT_NOBITS};
use api::cell_image::{is_valid_image_name, ImageIdentity, IMAGE_NAME_SECTION};

/// The identity recorded for an admitted image. `verified` is true only when
/// its signature verified under this kernel's anchor: the name is believed
/// only then, because only then are the section bytes covered by a signature
/// the kernel checked. An unsigned image states its digest and nothing else,
/// whatever name its section claims.
pub(super) fn measured_identity(data: &[u8], digest: [u8; 32], verified: bool) -> ImageIdentity {
    verified
        .then(|| {
            let name = declared_name(data).unwrap_or(&[]);
            ImageIdentity::signed(digest, crate::signing::anchor_class(), name)
        })
        .flatten()
        .unwrap_or(ImageIdentity::unsigned(digest))
}

/// The declared program name, or `None` when the image declares none or the
/// declaration is unusable (duplicated, `NOBITS`, out of bounds, or not a valid
/// name). Unusable is treated as absent: the image still spawns, it just
/// carries no name, so no name-bound rule can ever match it.
fn declared_name(data: &[u8]) -> Option<&[u8]> {
    let elf = ElfSections::parse(data)?;
    let names = elf.names()?;
    let mut found = None;
    for index in 0..elf.count() {
        let section = elf.section(index)?;
        if elf.name(names, section)? != IMAGE_NAME_SECTION.as_bytes() {
            continue;
        }
        if section.kind == SHT_NOBITS || found.is_some() {
            return None;
        }
        found = Some(elf.bytes(section)?);
    }
    found.filter(|name| is_valid_image_name(name))
}

#[cfg(test)]
mod tests {
    use super::{declared_name, measured_identity};

    const NAMES: &[u8] = b"\0.shstrtab\0__ViCell_image\0";
    const DATA: usize = 96;
    const SHOFF: usize = 128;

    fn put(elf: &mut [u8], at: usize, value: u64, width: usize) {
        elf[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }

    fn section(elf: &mut [u8], index: usize, name: u32, kind: u32, offset: usize, size: usize) {
        let base = SHOFF + index * 64;
        put(elf, base, u64::from(name), 4);
        put(elf, base + 4, u64::from(kind), 4);
        put(elf, base + 24, offset as u64, 8);
        put(elf, base + 32, size as u64, 8);
    }

    /// Minimal ELF64: null section, `.shstrtab`, then `copies` image sections.
    fn elf64(name: &[u8], copies: usize, kind: u32) -> alloc::vec::Vec<u8> {
        let count = 2 + copies;
        let mut elf = alloc::vec![0u8; SHOFF + count * 64];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        put(&mut elf, 16, 2, 2);
        put(&mut elf, 18, 243, 2);
        put(&mut elf, 20, 1, 4);
        put(&mut elf, 40, SHOFF as u64, 8);
        put(&mut elf, 52, 64, 2);
        put(&mut elf, 58, 64, 2);
        put(&mut elf, 60, count as u64, 2);
        put(&mut elf, 62, 1, 2);
        elf[64..64 + NAMES.len()].copy_from_slice(NAMES);
        section(&mut elf, 1, 1, 3, 64, NAMES.len());
        elf[DATA..DATA + name.len()].copy_from_slice(name);
        for copy in 0..copies {
            section(&mut elf, 2 + copy, 11, kind, DATA, name.len());
        }
        elf
    }

    #[test]
    fn single_valid_section_is_the_name() {
        assert_eq!(declared_name(&elf64(b"sshd", 1, 1)), Some(&b"sshd"[..]));
    }

    #[test]
    fn absent_duplicate_nobits_or_invalid_declares_nothing() {
        assert_eq!(declared_name(&elf64(b"sshd", 0, 1)), None);
        assert_eq!(declared_name(&elf64(b"sshd", 2, 1)), None);
        assert_eq!(declared_name(&elf64(b"sshd", 1, 8)), None);
        assert_eq!(declared_name(&elf64(b"SSHD", 1, 1)), None);
        assert_eq!(declared_name(&elf64(b"ssh\0", 1, 1)), None);
        assert_eq!(declared_name(b"not an elf"), None);
    }

    #[test]
    fn unsigned_image_named_sshd_carries_no_name() {
        let elf = elf64(b"sshd", 1, 1);
        let unsigned = measured_identity(&elf, [4; 32], false);
        assert!(!unsigned.is_signed());
        assert!(unsigned.name().is_empty());
        assert!(!unsigned.is_signed_program("sshd"));
        assert_eq!(unsigned.digest(), &[4; 32]);
    }

    #[test]
    fn verified_image_carries_its_name_under_this_kernels_anchor() {
        let signed = measured_identity(&elf64(b"sshd", 1, 1), [5; 32], true);
        if crate::signing::anchor_class() == api::cell_image::ANCHOR_UNSIGNED {
            // No anchor compiled in: nothing can verify, so nothing is named.
            assert!(!signed.is_signed());
        } else {
            assert_eq!(signed.anchor(), crate::signing::anchor_class());
            assert!(signed.is_signed_program("sshd"));
        }
        // A verified image without the section loads with no name.
        let anonymous = measured_identity(&elf64(b"sshd", 0, 1), [5; 32], true);
        assert!(!anonymous.is_signed_program("sshd"));
    }
}
