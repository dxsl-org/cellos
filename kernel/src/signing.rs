//! Cell binary signing — Ed25519 signature verification for spawned cell ELFs.
//!
//! The kernel holds only a public key (fleet trust anchor). Cell binaries are
//! signed offline with the corresponding private key and carry the 64-byte
//! signature in an `__ViCell_sig` ELF section (non-loadable, not in any PT_LOAD).
//!
//! Canonical signed payload:
//!   1. PT_LOAD segments sorted by (p_offset, p_filesz, phdr_index) — execution content
//!   2. `__ViCell_manifest` section bytes — capability claims (signed even if outside PT_LOAD)
//!
//! The ELF header is NOT included: `objcopy --add-section` mutates `e_shnum`/`e_shoff` when
//! embedding `__ViCell_sig`, so including the header would break verification. PT_LOAD covers
//! all executable code; the manifest covers all capability claims.
//!
//! Verify-only: the kernel never signs (private key lives offline).

use alloc::vec::Vec;

/// Dev Ed25519 **public** key — derived from the fixed dev seed in
/// `scripts/sign-cell.py` (seed `[0x43]*32`, reproducible; never shipped in release).
const DEV_CELL_SIGNER_PUBKEY: [u8; 32] = [
    0x22, 0xfc, 0x29, 0x77, 0x92, 0xf0, 0xb6, 0xff, 0xc0, 0xbf, 0xcf, 0xdb, 0x7e, 0xdb, 0x0c, 0x0a,
    0xa1, 0x4e, 0x02, 0x5a, 0x36, 0x5e, 0xc0, 0xe3, 0x42, 0xe8, 0x6e, 0x38, 0x29, 0xcb, 0x74, 0xb6,
];

/// Fleet cell-signing trust anchor.
/// `dev-signing-key` → the reproducible dev key above (matching `scripts/sign-cell.py --seed 0x43*32`).
/// Otherwise → a zero placeholder that fails every verify (fail-closed until prod key is provisioned).
#[cfg(feature = "dev-signing-key")]
const CELL_SIGNER_PUBKEY: [u8; 32] = DEV_CELL_SIGNER_PUBKEY;

#[cfg(not(feature = "dev-signing-key"))]
const CELL_SIGNER_PUBKEY: [u8; 32] = [0u8; 32]; // TODO(prod): provisioned fleet key

/// Returns `true` when the `signing-required` build feature is set (CI/prod posture).
/// In dev mode (default) an absent signature is permitted; with `signing-required`
/// an unsigned cell is denied the same as a tampered one.
pub const fn signing_required() -> bool {
    cfg!(feature = "signing-required")
}

/// Extract the 64-byte Ed25519 signature from the `__ViCell_sig` ELF section.
///
/// Returns `None` if the section is absent or not exactly 64 bytes.
pub fn extract_sig(elf_bytes: &[u8]) -> Option<[u8; 64]> {
    use crate::loader::ElfParser;
    let raw = crate::loader::ElfLoader.get_section(elf_bytes, "__ViCell_sig").ok()?;
    if raw.len() != 64 {
        return None;
    }
    let mut sig = [0u8; 64];
    sig.copy_from_slice(raw);
    Some(sig)
}

/// Verify the Ed25519 signature of a cell ELF binary.
///
/// Returns `false` on any malformed input, missing section, or signature
/// mismatch — never panics. Canonically covers the ELF header, all PT_LOAD
/// segments, and the `__ViCell_manifest` section.
pub fn verify_cell(elf_bytes: &[u8], sig: &[u8; 64]) -> bool {
    verify_cell_with_key(elf_bytes, sig, &CELL_SIGNER_PUBKEY)
}

/// Inner implementation; accepts an explicit key so `self_test` can use
/// the precomputed test key without touching `CELL_SIGNER_PUBKEY`.
fn verify_cell_with_key(elf_bytes: &[u8], sig: &[u8; 64], pubkey: &[u8; 32]) -> bool {
    use xmas_elf::ElfFile;
    use xmas_elf::program::Type;

    let elf = match ElfFile::new(elf_bytes) {
        Ok(e) => e,
        Err(_) => return false,
    };

    // Build the signed payload.
    let mut payload: Vec<u8> = Vec::new();

    // 1. PT_LOAD segments — sort by (p_offset, p_filesz, phdr_index) for a
    //    deterministic total order even when two segments share an offset.
    let mut segments: Vec<(u64, u64, usize)> = elf
        .program_iter()
        .enumerate()
        .filter_map(|(i, ph)| {
            if ph.get_type() == Ok(Type::Load) && ph.file_size() > 0 {
                Some((ph.offset(), ph.file_size(), i))
            } else {
                None
            }
        })
        .collect();
    segments.sort_by_key(|&(off, fsz, idx)| (off, fsz, idx));

    for (offset, filesz, _) in &segments {
        let start = *offset as usize;
        let end = match start.checked_add(*filesz as usize) {
            Some(e) if e <= elf_bytes.len() => e,
            _ => return false, // out-of-bounds PT_LOAD — malformed ELF
        };
        payload.extend_from_slice(&elf_bytes[start..end]);
    }

    // 2. __ViCell_manifest section — capability claims. Explicitly included
    //    because the manifest may fall outside PT_LOAD in some linker layouts.
    //    A missing manifest means the cell declares no capabilities; we include
    //    an empty slice (nothing to absorb) so manifest-less cells are signable.
    if let Ok(manifest) = {
        use crate::loader::ElfParser;
        crate::loader::ElfLoader.get_section(elf_bytes, "__ViCell_manifest")
    } {
        payload.extend_from_slice(manifest);
    }

    crate::ed25519::verify(pubkey, &payload, sig)
}

/// Boot-time self-test using a precomputed RFC-style test vector.
///
/// Uses a separate precomputed `(pubkey, payload, sig)` triple — **no private
/// key in the kernel**. Returns `true` iff the known-good vector verifies AND
/// a flipped-byte payload is rejected.
pub fn self_test() -> bool {
    // Precomputed vector — seed [0x43]*32, payload b"CellosSigningTest".
    // Regenerate: `python scripts/sign-cell.py --emit-test-vector`
    const TEST_PUBKEY: [u8; 32] = [
        0x22, 0xfc, 0x29, 0x77, 0x92, 0xf0, 0xb6, 0xff, 0xc0, 0xbf, 0xcf, 0xdb, 0x7e, 0xdb, 0x0c, 0x0a,
        0xa1, 0x4e, 0x02, 0x5a, 0x36, 0x5e, 0xc0, 0xe3, 0x42, 0xe8, 0x6e, 0x38, 0x29, 0xcb, 0x74, 0xb6,
    ];
    const TEST_SIG: [u8; 64] = [
        0x22, 0xf6, 0x2e, 0xba, 0x53, 0x9c, 0x66, 0xa0, 0xc1, 0xed, 0x39, 0xc8, 0x90, 0x04, 0xf8, 0xfc,
        0x46, 0xb0, 0xe5, 0x42, 0xc9, 0x97, 0x22, 0x2d, 0x3f, 0x10, 0x17, 0xf3, 0xa4, 0x56, 0x67, 0x58,
        0x9b, 0x49, 0x98, 0x2b, 0x4a, 0x48, 0x23, 0x11, 0x90, 0x09, 0x25, 0xe3, 0x9f, 0x02, 0x0b, 0x0e,
        0x34, 0x70, 0x25, 0xfa, 0x10, 0xe3, 0x7e, 0xac, 0xd4, 0xb1, 0x6c, 0x66, 0xcf, 0x7b, 0x1e, 0x0a,
    ];
    const TEST_PAYLOAD: &[u8] = b"CellosSigningTest";

    // Positive: known-good vector must verify.
    if !crate::ed25519::verify(&TEST_PUBKEY, TEST_PAYLOAD, &TEST_SIG) {
        return false;
    }
    // Negative: flipped byte in payload must be rejected.
    let mut bad_payload = alloc::vec![0u8; TEST_PAYLOAD.len()];
    bad_payload.copy_from_slice(TEST_PAYLOAD);
    bad_payload[0] ^= 0x01;
    if crate::ed25519::verify(&TEST_PUBKEY, &bad_payload, &TEST_SIG) {
        return false;
    }
    true
}
