//! Forward-edge CFI opt-in: the GNU property note of an admitted cell image.
//!
//! A cell asks for hardware forward-edge CFI exactly the way a Linux ELF does:
//! the linker ANDs `GNU_PROPERTY_*_FEATURE_1_AND` across every input object and
//! emits the result as a `NT_GNU_PROPERTY_TYPE_0` note in its own
//! `PT_GNU_PROPERTY` segment. The bit survives only when *every* object was
//! compiled with landing pads (`-Z branch-protection=bti` / `-mbranch-protection=bti`
//! on AArch64, `-Z cf-protection=branch` / `-fcf-protection=branch` on x86_64),
//! so it is the toolchain's statement that enforcing CFI cannot break the image.
//!
//! The note is read from the same bytes the loader maps — the image admission
//! has already verified (signature, policy) — so the opt-in is covered by the
//! signature like the rest of the image. Only `PT_GNU_PROPERTY` is consulted,
//! as on Linux (`fs/binfmt_elf.c`): lld always emits it when a property note
//! exists, unless a linker script declares `PHDRS` (the cell script does not).
//!
//! An image without the segment, or whose property lacks the bit, is not opted
//! in and runs exactly as before. A *malformed* property segment refuses the
//! spawn (Linux returns `ENOEXEC`): the note is signed toolchain output, so a
//! malformed one means a broken or hostile image, not a choice to make silently.
//!
//! What the loader does with the result is [`arm`]: AArch64 maps the image's
//! executable pages guarded (stage-1 `GP`), x86_64 gives the task a non-zero
//! `IA32_U_CET`. Both only when the CPU implements the mechanism.

use types::{ViError, ViResult};

/// `p_type` of the program header that holds the property note.
const PT_GNU_PROPERTY: u32 = 0x6474_e553;
/// `n_type` of the property note.
const NT_GNU_PROPERTY_TYPE_0: u32 = 5;
/// `GNU_PROPERTY_AARCH64_FEATURE_1_AND` and its BTI bit.
const GNU_PROPERTY_AARCH64_FEATURE_1_AND: u32 = 0xc000_0000;
const GNU_PROPERTY_AARCH64_FEATURE_1_BTI: u32 = 1 << 0;
/// `GNU_PROPERTY_X86_FEATURE_1_AND` and its IBT bit.
const GNU_PROPERTY_X86_FEATURE_1_AND: u32 = 0xc000_0002;
const GNU_PROPERTY_X86_FEATURE_1_IBT: u32 = 1 << 0;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;
/// Upper bound for the property segment. Real notes are 32 bytes; the bound
/// keeps a hostile `p_filesz` from turning the walk into a long scan.
const MAX_PROPERTY_BYTES: usize = 1024;
/// ELF64 program-header entry size.
const PHENT64: usize = 56;

/// Forward-edge CFI mechanisms an image declares it is built for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForwardEdgeCfi {
    /// AArch64 BTI: every indirect-branch target carries a `BTI` landing pad.
    pub bti: bool,
    /// x86_64 CET-IBT: every indirect-branch target carries `ENDBR64`.
    pub ibt: bool,
}

/// What the loader arms for one spawn, given the image's opt-in and the CPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Armed {
    /// AArch64: map the image's executable pages with the BTI `GP` bit.
    pub guarded_code: bool,
    /// x86_64: initial `IA32_U_CET` image for the task (0 = not armed).
    pub user_cet: u64,
}

fn read_u16(data: &[u8], at: usize) -> ViResult<u16> {
    let bytes = data
        .get(at..at.checked_add(2).ok_or(ViError::InvalidInput)?)
        .ok_or(ViError::InvalidInput)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], at: usize) -> ViResult<u32> {
    let bytes = data
        .get(at..at.checked_add(4).ok_or(ViError::InvalidInput)?)
        .ok_or(ViError::InvalidInput)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(data: &[u8], at: usize) -> ViResult<u64> {
    let bytes = data
        .get(at..at.checked_add(8).ok_or(ViError::InvalidInput)?)
        .ok_or(ViError::InvalidInput)?;
    let mut word = [0u8; 8];
    word.copy_from_slice(bytes);
    Ok(u64::from_le_bytes(word))
}

/// The forward-edge CFI opt-in of an ELF image.
///
/// Returns the default (nothing declared) for an image without
/// `PT_GNU_PROPERTY`, for an ELF32 image (no 32-bit target has a mechanism to
/// arm), and for a machine with no property this kernel arms.
///
/// # Errors
/// `ViError::InvalidInput` for a truncated header or program-header table, more
/// than one `PT_GNU_PROPERTY`, a segment outside the file or larger than
/// [`MAX_PROPERTY_BYTES`], or a malformed note (see [`parse_property_note`]).
pub fn forward_edge_cfi(elf: &[u8]) -> ViResult<ForwardEdgeCfi> {
    // EI_CLASS 2 = ELF64, EI_DATA 1 = little-endian: the only layout any cell
    // target with a CFI mechanism uses.
    if elf.len() < 64 || elf[4] != 2 || elf[5] != 1 {
        return Ok(ForwardEdgeCfi::default());
    }
    let machine = read_u16(elf, 18)?;
    let phoff = usize::try_from(read_u64(elf, 32)?).map_err(|_| ViError::InvalidInput)?;
    let phentsize = usize::from(read_u16(elf, 54)?);
    let phnum = usize::from(read_u16(elf, 56)?);
    if phnum == 0 {
        return Ok(ForwardEdgeCfi::default());
    }
    if phentsize != PHENT64 {
        return Err(ViError::InvalidInput);
    }

    let mut found: Option<&[u8]> = None;
    for index in 0..phnum {
        let ph = index
            .checked_mul(PHENT64)
            .and_then(|off| phoff.checked_add(off))
            .ok_or(ViError::InvalidInput)?;
        if read_u32(elf, ph)? != PT_GNU_PROPERTY {
            continue;
        }
        if found.is_some() {
            return Err(ViError::InvalidInput);
        }
        let offset = usize::try_from(read_u64(elf, ph + 8)?).map_err(|_| ViError::InvalidInput)?;
        let size = usize::try_from(read_u64(elf, ph + 32)?).map_err(|_| ViError::InvalidInput)?;
        if size > MAX_PROPERTY_BYTES {
            return Err(ViError::InvalidInput);
        }
        let end = offset.checked_add(size).ok_or(ViError::InvalidInput)?;
        found = Some(elf.get(offset..end).ok_or(ViError::InvalidInput)?);
    }
    match found {
        Some(note) => parse_property_note(note, machine),
        None => Ok(ForwardEdgeCfi::default()),
    }
}

/// Parse the contents of a `PT_GNU_PROPERTY` segment for `machine` (ELF
/// `e_machine`).
///
/// Layout (ELF64, every field little-endian, 8-byte alignment):
/// `n_namesz = 4`, `n_descsz`, `n_type = NT_GNU_PROPERTY_TYPE_0`, `"GNU\0"`,
/// then `n_descsz` bytes of properties, each `pr_type: u32`, `pr_datasz: u32`,
/// `pr_data` padded to 8 bytes, in strictly ascending `pr_type` order.
///
/// # Errors
/// `ViError::InvalidInput` when any of the above does not hold, or when the
/// machine's `FEATURE_1_AND` property has a `pr_datasz` other than 4.
pub fn parse_property_note(note: &[u8], machine: u16) -> ViResult<ForwardEdgeCfi> {
    let namesz = read_u32(note, 0)?;
    let descsz = usize::try_from(read_u32(note, 4)?).map_err(|_| ViError::InvalidInput)?;
    let kind = read_u32(note, 8)?;
    if namesz != 4 || kind != NT_GNU_PROPERTY_TYPE_0 || note.get(12..16) != Some(b"GNU\0") {
        return Err(ViError::InvalidInput);
    }
    if !descsz.is_multiple_of(8) {
        return Err(ViError::InvalidInput);
    }
    let desc = note
        .get(16..16usize.checked_add(descsz).ok_or(ViError::InvalidInput)?)
        .ok_or(ViError::InvalidInput)?;

    let wanted = match machine {
        EM_AARCH64 => Some(GNU_PROPERTY_AARCH64_FEATURE_1_AND),
        EM_X86_64 => Some(GNU_PROPERTY_X86_FEATURE_1_AND),
        _ => None,
    };
    let mut cfi = ForwardEdgeCfi::default();
    let mut previous: Option<u32> = None;
    let mut at = 0;
    while at < desc.len() {
        let pr_type = read_u32(desc, at)?;
        let datasz = usize::try_from(read_u32(desc, at + 4)?).map_err(|_| ViError::InvalidInput)?;
        let data_start = at + 8;
        let data_end = data_start
            .checked_add(datasz)
            .filter(|&end| end <= desc.len())
            .ok_or(ViError::InvalidInput)?;
        if previous.is_some_and(|prev| pr_type <= prev) {
            return Err(ViError::InvalidInput);
        }
        previous = Some(pr_type);
        if Some(pr_type) == wanted {
            if datasz != 4 {
                return Err(ViError::InvalidInput);
            }
            let bits = read_u32(desc, data_start)?;
            match machine {
                EM_AARCH64 => cfi.bti = bits & GNU_PROPERTY_AARCH64_FEATURE_1_BTI != 0,
                _ => cfi.ibt = bits & GNU_PROPERTY_X86_FEATURE_1_IBT != 0,
            }
        }
        // `pr_data` is padded to the 8-byte ELF64 note alignment.
        at = data_end
            .checked_add(7)
            .map(|end| end & !7)
            .ok_or(ViError::InvalidInput)?;
    }
    Ok(cfi)
}

/// Decide what to arm for `cell` from its opt-in and this CPU, and log it.
///
/// The CPU half is what keeps an opted-in image running on hardware without
/// the mechanism: BTI and ENDBR64 are NOPs there, and the `GP` bit is RES0
/// before FEAT_BTI, so neither is set and the image runs unenforced — the same
/// as an image that never opted in.
pub fn arm(cfi: ForwardEdgeCfi, cell: &str) -> Armed {
    Armed {
        guarded_code: cfi.bti && arm_bti(cell),
        user_cet: if cfi.ibt { arm_ibt(cell) } else { 0 },
    }
}

/// AArch64: whether an image that opts into BTI gets guarded code pages.
#[cfg(target_arch = "aarch64")]
fn arm_bti(cell: &str) -> bool {
    let available = crate::hal::cfi::detect().bti;
    if available {
        log::info!(
            "[cfi] '{}': BTI armed (GNU property) — executable pages mapped guarded",
            cell
        );
    } else {
        log::info!(
            "[cfi] '{}': image opts into BTI but the CPU lacks FEAT_BTI — not enforced",
            cell
        );
    }
    available
}

/// Other targets have no BTI; an AArch64 property never reaches them anyway
/// (the parser only reads the property of the image's own `e_machine`).
#[cfg(not(target_arch = "aarch64"))]
fn arm_bti(_cell: &str) -> bool {
    false
}

/// x86_64: the initial `IA32_U_CET` for a task whose image opts into IBT.
#[cfg(all(target_arch = "x86_64", not(all(test, not(target_os = "none")))))]
fn arm_ibt(cell: &str) -> u64 {
    if crate::hal::cet::user_ibt_available() {
        log::info!(
            "[cfi] '{}': CET-IBT armed (GNU property) — IA32_U_CET.ENDBR_EN for this task",
            cell
        );
        crate::hal::cet::USER_CET_IBT
    } else {
        log::info!(
            "[cfi] '{}': image opts into CET-IBT but the CPU does not enumerate IBT — not enforced",
            cell
        );
        0
    }
}

/// Targets without CET (and the hosted test build, which cannot touch CR4).
#[cfg(not(all(target_arch = "x86_64", not(all(test, not(target_os = "none"))))))]
fn arm_ibt(_cell: &str) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// A property note carrying `props` (`(pr_type, pr_data)`), 8-byte padded.
    fn note(props: &[(u32, &[u8])]) -> Vec<u8> {
        let mut desc = Vec::new();
        for (pr_type, data) in props {
            desc.extend_from_slice(&pr_type.to_le_bytes());
            desc.extend_from_slice(&(data.len() as u32).to_le_bytes());
            desc.extend_from_slice(data);
            while desc.len() % 8 != 0 {
                desc.push(0);
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(&(desc.len() as u32).to_le_bytes());
        out.extend_from_slice(&NT_GNU_PROPERTY_TYPE_0.to_le_bytes());
        out.extend_from_slice(b"GNU\0");
        out.extend_from_slice(&desc);
        out
    }

    /// A minimal ELF64 LE image: header, program headers of `types`, then
    /// `payload`; every `PT_GNU_PROPERTY` header points at `payload`.
    fn elf(machine: u16, types: &[u32], payload: &[u8]) -> Vec<u8> {
        let phoff = 64usize;
        let data_off = phoff + types.len() * PHENT64;
        let mut out = alloc::vec![0u8; data_off];
        out[..4].copy_from_slice(b"\x7fELF");
        out[4] = 2;
        out[5] = 1;
        out[18..20].copy_from_slice(&machine.to_le_bytes());
        out[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
        out[54..56].copy_from_slice(&(PHENT64 as u16).to_le_bytes());
        out[56..58].copy_from_slice(&(types.len() as u16).to_le_bytes());
        for (index, p_type) in types.iter().enumerate() {
            let ph = phoff + index * PHENT64;
            out[ph..ph + 4].copy_from_slice(&p_type.to_le_bytes());
            out[ph + 8..ph + 16].copy_from_slice(&(data_off as u64).to_le_bytes());
            out[ph + 32..ph + 40].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    const BTI: &[u8] = &GNU_PROPERTY_AARCH64_FEATURE_1_BTI.to_le_bytes();
    const IBT: &[u8] = &GNU_PROPERTY_X86_FEATURE_1_IBT.to_le_bytes();

    #[test]
    fn aarch64_bti_bit_opts_in() {
        let n = note(&[(GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI)]);
        let cfi = forward_edge_cfi(&elf(EM_AARCH64, &[1, PT_GNU_PROPERTY], &n)).unwrap();
        assert_eq!(
            cfi,
            ForwardEdgeCfi {
                bti: true,
                ibt: false
            }
        );
    }

    #[test]
    fn x86_ibt_bit_opts_in() {
        // SHSTK (bit 1) alongside IBT does not change the IBT answer.
        let both = (GNU_PROPERTY_X86_FEATURE_1_IBT | 2).to_le_bytes();
        let n = note(&[(GNU_PROPERTY_X86_FEATURE_1_AND, &both)]);
        let cfi = forward_edge_cfi(&elf(EM_X86_64, &[PT_GNU_PROPERTY], &n)).unwrap();
        assert_eq!(
            cfi,
            ForwardEdgeCfi {
                bti: false,
                ibt: true
            }
        );
    }

    #[test]
    fn image_without_the_segment_is_not_opted_in() {
        let cfi = forward_edge_cfi(&elf(EM_AARCH64, &[1, 4], &[])).unwrap();
        assert_eq!(cfi, ForwardEdgeCfi::default());
    }

    #[test]
    fn property_without_the_bit_is_not_opted_in() {
        // PAC only (bit 1) on AArch64; SHSTK only (bit 1) on x86_64.
        let pac = 2u32.to_le_bytes();
        let n = note(&[(GNU_PROPERTY_AARCH64_FEATURE_1_AND, &pac)]);
        assert_eq!(
            parse_property_note(&n, EM_AARCH64).unwrap(),
            ForwardEdgeCfi::default()
        );
        let n = note(&[(GNU_PROPERTY_X86_FEATURE_1_AND, &pac)]);
        assert_eq!(
            parse_property_note(&n, EM_X86_64).unwrap(),
            ForwardEdgeCfi::default()
        );
    }

    #[test]
    fn another_machines_property_is_ignored() {
        // The AArch64 BTI property on an x86_64 image, and the x86 IBT
        // property on an AArch64 or RISC-V image, arm nothing.
        let a64 = note(&[(GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI)]);
        assert_eq!(
            parse_property_note(&a64, EM_X86_64).unwrap(),
            ForwardEdgeCfi::default()
        );
        let x86 = note(&[(GNU_PROPERTY_X86_FEATURE_1_AND, IBT)]);
        assert_eq!(
            parse_property_note(&x86, EM_AARCH64).unwrap(),
            ForwardEdgeCfi::default()
        );
        assert_eq!(
            parse_property_note(&x86, 243).unwrap(),
            ForwardEdgeCfi::default()
        );
    }

    #[test]
    fn other_properties_are_skipped_in_order() {
        // A 2-byte property ahead (padded to 8) and one behind the feature word.
        let n = note(&[
            (0x1, &[0xAA, 0xBB]),
            (GNU_PROPERTY_X86_FEATURE_1_AND, IBT),
            (0xc000_8002, &[0; 4]),
        ]);
        assert!(parse_property_note(&n, EM_X86_64).unwrap().ibt);
    }

    #[test]
    fn elf32_image_has_nothing_to_arm() {
        let mut image = elf(EM_AARCH64, &[], &[]);
        image[4] = 1;
        assert_eq!(forward_edge_cfi(&image).unwrap(), ForwardEdgeCfi::default());
    }

    #[test]
    fn malformed_notes_are_refused() {
        let good = note(&[(GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI)]);

        let mut wrong_name = good.clone();
        wrong_name[12] = b'X';
        let mut wrong_type = good.clone();
        wrong_type[8] = 1;
        let mut odd_descsz = good.clone();
        odd_descsz[4] = 12;
        let mut overlong_descsz = good.clone();
        overlong_descsz[4] = 64;
        let truncated = &good[..good.len() - 4];
        let wide_feature = note(&[(
            GNU_PROPERTY_AARCH64_FEATURE_1_AND,
            &[1, 0, 0, 0, 0, 0, 0, 0],
        )]);
        let unordered = note(&[(GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI), (0x1, &[0; 4])]);
        let duplicated = note(&[
            (GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI),
            (GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI),
        ]);
        let mut datasz_past_end = good.clone();
        datasz_past_end[20] = 0xFF;

        for bad in [
            &wrong_name[..],
            &wrong_type[..],
            &odd_descsz[..],
            &overlong_descsz[..],
            truncated,
            &wide_feature[..],
            &unordered[..],
            &duplicated[..],
            &datasz_past_end[..],
            &[][..],
        ] {
            assert_eq!(
                parse_property_note(bad, EM_AARCH64),
                Err(ViError::InvalidInput)
            );
        }
    }

    #[test]
    fn malformed_segments_are_refused() {
        let n = note(&[(GNU_PROPERTY_AARCH64_FEATURE_1_AND, BTI)]);
        // Two property segments.
        let two = elf(EM_AARCH64, &[PT_GNU_PROPERTY, PT_GNU_PROPERTY], &n);
        assert_eq!(forward_edge_cfi(&two), Err(ViError::InvalidInput));
        // A segment running past the end of the file.
        let mut past_end = elf(EM_AARCH64, &[PT_GNU_PROPERTY], &n);
        past_end.truncate(past_end.len() - 1);
        assert_eq!(forward_edge_cfi(&past_end), Err(ViError::InvalidInput));
        // A segment over the size bound.
        let mut huge = elf(EM_AARCH64, &[PT_GNU_PROPERTY], &n);
        huge[64 + 32..64 + 40].copy_from_slice(&((MAX_PROPERTY_BYTES + 8) as u64).to_le_bytes());
        assert_eq!(forward_edge_cfi(&huge), Err(ViError::InvalidInput));
        // A program-header table outside the file.
        let mut table_past_end = elf(EM_AARCH64, &[PT_GNU_PROPERTY], &n);
        table_past_end[32..40].copy_from_slice(&0xFFFF_FFFFu64.to_le_bytes());
        assert_eq!(
            forward_edge_cfi(&table_past_end),
            Err(ViError::InvalidInput)
        );
    }

    #[test]
    fn nothing_is_armed_without_an_opt_in() {
        assert_eq!(arm(ForwardEdgeCfi::default(), "test"), Armed::default());
    }
}
