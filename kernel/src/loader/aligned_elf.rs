//! Borrow ELF bytes already 8-aligned or own an explicitly aligned copy.

pub(crate) enum AlignedElf<'a> {
    Borrowed(&'a [u8]),
    Owned {
        words: alloc::vec::Vec<u64>,
        len: usize,
    },
}

impl AsRef<[u8]> for AlignedElf<'_> {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Borrowed(data) => data,
            Self::Owned { words, len } => {
                // SAFETY: `Vec<u64>` is at least 8-aligned and owns `len` initialized bytes.
                unsafe { core::slice::from_raw_parts(words.as_ptr().cast::<u8>(), *len) }
            }
        }
    }
}

pub(crate) fn bytes(data: &[u8]) -> Result<AlignedElf<'_>, types::ViError> {
    if (data.as_ptr() as usize).is_multiple_of(8) {
        return Ok(AlignedElf::Borrowed(data));
    }
    // The copy is the ELF's own size, so it must be fallible: `vec![0u64; ..]`
    // goes through the infallible allocator and a full heap would halt the kernel
    // instead of refusing the spawn.
    let words_len = data.len().div_ceil(8);
    let mut words: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    words
        .try_reserve_exact(words_len)
        .map_err(|_| types::ViError::OutOfMemory)?;
    words.resize(words_len, 0);
    // SAFETY: the u64 backing has at least `data.len()` writable bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), words.as_mut_ptr().cast::<u8>(), data.len());
    }
    Ok(AlignedElf::Owned {
        words,
        len: data.len(),
    })
}
