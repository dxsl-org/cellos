//! Kernel-owned, 8-aligned ELF bytes for admission and loading.
//!
//! Admission reads an image several times — SHA-256, Ed25519 `verify_cell`,
//! manifest classification, segment load — and every pass must see the bytes
//! the previous one judged. Caller memory (a grant or user buffer) stays
//! writable by the caller's other threads and by grant peers, so an image from
//! there must be captured into an [`ElfSnapshot`] before the first pass.

use alloc::vec::Vec;
use types::{ViError, ViResult};

/// An owned copy of an ELF image. The `u64` backing keeps it 8-aligned so
/// [`bytes`] borrows it instead of copying a second time.
pub(crate) struct ElfSnapshot {
    words: Vec<u64>,
    len: usize,
}

impl ElfSnapshot {
    /// A zero-filled snapshot of `len` bytes for the caller to fill once.
    ///
    /// `len` is the image's own (caller-chosen) size, so the reservation is
    /// fallible: `vec![0u64; ..]` would reach the infallible alloc error handler
    /// and halt the kernel instead of refusing the spawn.
    pub(crate) fn zeroed(len: usize) -> ViResult<Self> {
        let words_len = len.div_ceil(8);
        let mut words = Vec::new();
        words
            .try_reserve_exact(words_len)
            .map_err(|_| ViError::OutOfMemory)?;
        words.resize(words_len, 0);
        Ok(Self { words, len })
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: the `u64` backing holds at least `len` initialized bytes.
        unsafe { core::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), self.len) }
    }

    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the `u64` backing holds at least `len` initialized bytes and
        // `&mut self` makes this the only view of them.
        unsafe { core::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast::<u8>(), self.len) }
    }
}

pub(crate) enum AlignedElf<'a> {
    Borrowed(&'a [u8]),
    Owned(ElfSnapshot),
}

impl AsRef<[u8]> for AlignedElf<'_> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(data) => data,
            Self::Owned(snapshot) => snapshot.bytes(),
        }
    }
}

/// View `data` 8-aligned, borrowing it when it already is.
///
/// Precondition: `data` is kernel-owned and immutable while the result lives
/// (a disk/VIFS read, an embedded image, or an [`ElfSnapshot`]). The aligned
/// fast path reads `data` itself on every pass, so caller memory passed here
/// reopens the verify-then-load TOCTOU.
pub(crate) fn bytes(data: &[u8]) -> ViResult<AlignedElf<'_>> {
    if (data.as_ptr() as usize).is_multiple_of(8) {
        return Ok(AlignedElf::Borrowed(data));
    }
    let mut copy = ElfSnapshot::zeroed(data.len())?;
    copy.bytes_mut().copy_from_slice(data);
    Ok(AlignedElf::Owned(copy))
}

#[cfg(all(
    test,
    feature = "dev-signing-key",
    not(feature = "phase02-terminal-profile")
))]
mod tests {
    use super::{bytes, AlignedElf, ElfSnapshot};
    use crate::loader::elf_tests::{build_minimal_signed_elf, MINIMAL_ELF_DEV_SIG};

    /// A writer that still holds the source after capture (another caller
    /// thread, a grant peer) cannot change what admission verifies or loads.
    #[test]
    fn snapshot_keeps_signed_bytes_after_source_rewrite() {
        let mut source = build_minimal_signed_elf(MINIMAL_ELF_DEV_SIG);
        let original_digest = crate::sha256::sha256(&source);
        let sig = crate::signing::extract_sig(&source).expect("fixture carries a signature");

        let mut snapshot = ElfSnapshot::zeroed(source.len()).expect("fixture fits the heap");
        snapshot.bytes_mut().copy_from_slice(&source);
        source[120] ^= 0xff; // Rewrite the PT_LOAD code bytes after capture.

        assert!(!crate::signing::verify_cell(&source, &sig).expect("verifier has memory"));
        let admitted = bytes(snapshot.bytes()).expect("aligned view");
        assert!(matches!(admitted, AlignedElf::Borrowed(_)));
        assert!(crate::signing::verify_cell(admitted.as_ref(), &sig).expect("verifier has memory"));
        assert_eq!(crate::sha256::sha256(admitted.as_ref()), original_digest);
        assert_eq!(admitted.as_ref()[120], 0);
    }
}
