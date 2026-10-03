//! Cell binary signing — Ed25519 signature verification for spawned cell ELFs.
//!
//! The kernel holds only a public key (fleet trust anchor). Cell binaries are
//! signed offline with the corresponding private key and carry the 64-byte
//! signature in an `__ViCell_sig` ELF section.
//!
//! Canonical signed payload: every byte of the final ELF container except the
//! 64 signature bytes themselves. The signature section header remains covered,
//! so section names, offsets, types, flags, links, relocation metadata, ELF
//! headers, program headers, and all load-affecting payload bytes are immutable.
//! The signer first adds a zero-filled signature section, signs that stable
//! container, then replaces only those excluded bytes.
//!
//! Verify-only: the kernel never signs (private key lives offline).

use alloc::vec::Vec;
use types::{ViError, ViResult};

/// Reused buffer for the signed payload (the ELF minus its 64 signature bytes).
///
/// The message a verification hashes is a *transient*: it exists for one
/// `verify_cell` call. Allocating it per spawn put two 78 KiB buffers in flight at
/// once (the ELF read plus this copy), and at ~194 parked cells the payload's
/// 78,760-byte request was the one that could no longer find a contiguous run
/// (measured 2026-10-03: 289 KiB free, no 78 KiB hole — the refusal that ended the
/// capacity sweep). One shared buffer, grown once and never shrunk, removes that
/// per-spawn churn; verification itself is unchanged (same bytes, same order).
static PAYLOAD_SCRATCH: crate::sync::Spinlock<Vec<u8>> = crate::sync::Spinlock::new(Vec::new());

/// Dev Ed25519 **public** key — derived from the fixed dev seed in
/// `scripts/sign-cell.py` (seed `[0x43]*32`, reproducible; never shipped in release).
#[cfg(feature = "dev-signing-key")]
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
/// Returns `None` unless exactly one in-bounds signature section has 64 bytes.
pub fn extract_sig(elf_bytes: &[u8]) -> Option<[u8; 64]> {
    let (offset, size) = signature_range(elf_bytes)?;
    if size != 64 {
        return None;
    }
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&elf_bytes[offset..offset + size]);
    Some(sig)
}

/// Verify the Ed25519 signature of a cell ELF binary.
///
/// Returns `Ok(false)` on any malformed input, missing section, or signature
/// mismatch — never panics. The signature covers every final ELF byte except
/// its own 64-byte payload, including all relocation section headers and data.
///
/// Returns `Err(ViError::OutOfMemory)` when the signed payload cannot be
/// allocated: the caller must fail closed *and* report capacity, because a
/// verification that never ran is not a signature mismatch.
pub fn verify_cell(elf_bytes: &[u8], sig: &[u8; 64]) -> ViResult<bool> {
    verify_cell_with_key(elf_bytes, sig, &CELL_SIGNER_PUBKEY)
}

/// Locate the single file-backed fixed-size signature payload with the bounded
/// ELF parser. Returning its byte range lets verification cover the table
/// itself without creating a signature recursion.
fn signature_range(elf_bytes: &[u8]) -> Option<(usize, usize)> {
    const SHT_PROGBITS: u64 = 1;

    let elf = crate::loader::elf_section::ElfSections::parse(elf_bytes)?;
    let names = elf.names()?;
    let mut signature = None;
    for index in 0..elf.count() {
        let section = elf.section(index)?;
        if elf.name(names, section)? != b"__ViCell_sig" {
            continue;
        }
        // Only a file-backed, ordinary data section can carry the excluded
        // bytes. Every other section's metadata stays inside the signed input.
        if signature.is_some() || section.kind != SHT_PROGBITS || section.size != 64 {
            return None;
        }
        elf.bytes(section)?;
        signature = Some((section.offset, section.size));
    }
    signature
}

/// Inner implementation; accepts an explicit key so `self_test` can use
/// the precomputed test key without touching `CELL_SIGNER_PUBKEY`.
fn verify_cell_with_key(elf_bytes: &[u8], sig: &[u8; 64], pubkey: &[u8; 32]) -> ViResult<bool> {
    let (signature_offset, signature_size) = match signature_range(elf_bytes) {
        Some(range @ (_, 64)) => range,
        _ => return Ok(false),
    };
    if extract_sig(elf_bytes).as_ref() != Some(sig) {
        return Ok(false);
    }

    // The signature section itself is the only excluded interval. Its header
    // and placement stay in the signed byte stream, so no parser-selected
    // relocation data can move outside authenticated bytes.
    //
    // The payload is a full copy of the ELF minus its 64 signature bytes, so it
    // must stay fallible: `Vec::with_capacity` reaches the infallible alloc error
    // handler, which halts the kernel instead of refusing the spawn. It is built
    // in `PAYLOAD_SCRATCH` rather than a fresh buffer because a per-spawn 78 KiB
    // allocation interleaved with the retained per-cell state is what fragmented
    // the heap: at 194 parked cells this exact request (78 760 bytes for
    // `/bin/bench-probe`) was the one that no longer found a contiguous run
    // (289 KiB free), and it ended the capacity sweep.
    let mut scratch = PAYLOAD_SCRATCH.lock();
    let payload = &mut *scratch;
    payload.clear();
    payload
        .try_reserve_exact(elf_bytes.len() - signature_size)
        .map_err(|_| {
            log::error!(
                "[signing] OOM: signed payload of {} bytes",
                elf_bytes.len() - signature_size
            );
            ViError::OutOfMemory
        })?;
    payload.extend_from_slice(&elf_bytes[..signature_offset]);
    payload.extend_from_slice(&elf_bytes[signature_offset + signature_size..]);
    Ok(crate::ed25519::verify(pubkey, payload, sig))
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
        0x22, 0xfc, 0x29, 0x77, 0x92, 0xf0, 0xb6, 0xff, 0xc0, 0xbf, 0xcf, 0xdb, 0x7e, 0xdb, 0x0c,
        0x0a, 0xa1, 0x4e, 0x02, 0x5a, 0x36, 0x5e, 0xc0, 0xe3, 0x42, 0xe8, 0x6e, 0x38, 0x29, 0xcb,
        0x74, 0xb6,
    ];
    const TEST_SIG: [u8; 64] = [
        0x22, 0xf6, 0x2e, 0xba, 0x53, 0x9c, 0x66, 0xa0, 0xc1, 0xed, 0x39, 0xc8, 0x90, 0x04, 0xf8,
        0xfc, 0x46, 0xb0, 0xe5, 0x42, 0xc9, 0x97, 0x22, 0x2d, 0x3f, 0x10, 0x17, 0xf3, 0xa4, 0x56,
        0x67, 0x58, 0x9b, 0x49, 0x98, 0x2b, 0x4a, 0x48, 0x23, 0x11, 0x90, 0x09, 0x25, 0xe3, 0x9f,
        0x02, 0x0b, 0x0e, 0x34, 0x70, 0x25, 0xfa, 0x10, 0xe3, 0x7e, 0xac, 0xd4, 0xb1, 0x6c, 0x66,
        0xcf, 0x7b, 0x1e, 0x0a,
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
