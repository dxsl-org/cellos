//! The Cell initial stack, v1 (CP1 `cp1-cell-initial-stack-auxv.md`, AX-2…AX-5).
//!
//! The loader writes this block just below the top of every Cell's root user
//! stack and enters the Cell with `sp` at its first byte. Layout, low to high
//! address, all words 64-bit little-endian:
//!
//! ```text
//! sp →  argc = 0
//!       argv terminator (NULL)
//!       envp terminator (NULL)
//!       auxv pairs: AT_PHDR, AT_PHENT, AT_PHNUM, AT_PAGESZ, AT_BASE, AT_ENTRY,
//!                   [AT_RANDOM], then AT_NULL
//!       program-header copy (phnum × 56 bytes, 8-byte aligned)
//!       [16 entropy bytes]
//!       padding up to the stack top
//! ```
//!
//! `sp` is 16-byte aligned on every architecture. `AT_PHDR` and `AT_RANDOM`
//! hold absolute addresses inside the block. `AT_BASE` carries the Cell's load
//! base (Cellos has no interpreter). Pointers are computed from `top`, so the
//! block is position-dependent on the stack it is written to and nothing else.

#![no_std]
#![forbid(unsafe_code)]

/// ELF64 program-header entry size.
pub const PHENT: usize = 56;
/// AX-5: a Cell with more program headers is refused at spawn.
pub const MAX_PHDRS: usize = 16;
/// Page size reported in `AT_PAGESZ`.
pub const PAGE_SIZE: u64 = 4096;

pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_ENTRY: u64 = 9;
pub const AT_RANDOM: u64 = 25;

const HEAD_WORDS: usize = 3; // argc, argv NULL, envp NULL
const MAX_AUX_PAIRS: usize = 8; // seven entries + AT_NULL
const RANDOM_BYTES: usize = 16;

/// Upper bound of the block size, rounded to 16 bytes.
pub const MAX_BYTES: usize =
    (((HEAD_WORDS + 2 * MAX_AUX_PAIRS) * 8 + MAX_PHDRS * PHENT + RANDOM_BYTES) + 15) & !15;

/// What the loader knows about the image when it builds the block.
pub struct Image<'a> {
    /// The ELF program-header table, `phnum × PHENT` bytes, copied verbatim.
    pub phdrs: &'a [u8],
    /// Runtime load base of the image (0 for a non-PIE image).
    pub load_base: u64,
    /// Runtime entry address (`e_entry + load_base`).
    pub entry: u64,
    /// Kernel entropy; `None` omits `AT_RANDOM` (AX-4).
    pub random: Option<[u8; RANDOM_BYTES]>,
}

/// The block to copy to `[sp, top)`.
pub struct Block {
    bytes: [u8; MAX_BYTES],
    len: usize,
    sp: u64,
}

impl Block {
    /// Entry stack pointer: the address of `argc`.
    pub fn sp(&self) -> u64 {
        self.sp
    }

    /// Bytes to write at `sp`; their length is `top - sp`.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The table is not a whole number of 56-byte entries.
    BadPhdrTable,
    /// More than [`MAX_PHDRS`] program headers (AX-5).
    TooManyPhdrs,
    /// `top` is not 16-byte aligned or too low to hold the block.
    BadTop,
}

/// Lay out the block for a stack whose highest address (exclusive) is `top`.
pub fn build(top: u64, image: &Image<'_>) -> Result<Block, Error> {
    if !image.phdrs.len().is_multiple_of(PHENT) {
        return Err(Error::BadPhdrTable);
    }
    let phnum = image.phdrs.len() / PHENT;
    if phnum > MAX_PHDRS {
        return Err(Error::TooManyPhdrs);
    }
    let aux_pairs = 7 + usize::from(image.random.is_some());
    let words_len = (HEAD_WORDS + 2 * aux_pairs) * 8;
    let phdr_off = words_len;
    let random_off = phdr_off + image.phdrs.len();
    let used = random_off
        + if image.random.is_some() {
            RANDOM_BYTES
        } else {
            0
        };
    let len = (used + 15) & !15;
    if !top.is_multiple_of(16) || top < len as u64 {
        return Err(Error::BadTop);
    }
    let sp = top - len as u64;

    let mut bytes = [0u8; MAX_BYTES];
    let mut at = 0;
    let mut word = |value: u64| {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        at += 8;
    };
    word(0); // argc
    word(0); // argv terminator
    word(0); // envp terminator
    for (key, value) in [
        (AT_PHDR, sp + phdr_off as u64),
        (AT_PHENT, PHENT as u64),
        (AT_PHNUM, phnum as u64),
        (AT_PAGESZ, PAGE_SIZE),
        (AT_BASE, image.load_base),
        (AT_ENTRY, image.entry),
    ] {
        word(key);
        word(value);
    }
    if image.random.is_some() {
        word(AT_RANDOM);
        word(sp + random_off as u64);
    }
    word(AT_NULL);
    word(0);

    bytes[phdr_off..random_off].copy_from_slice(image.phdrs);
    if let Some(random) = image.random {
        bytes[random_off..random_off + RANDOM_BYTES].copy_from_slice(&random);
    }
    Ok(Block { bytes, len, sp })
}

#[cfg(test)]
mod tests;
