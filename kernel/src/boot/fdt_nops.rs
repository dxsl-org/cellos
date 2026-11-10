//! Drop `FDT_NOP` tokens from a flattened device tree's structure block.
//!
//! libfdt deletes a node by overwriting it with `FDT_NOP` tokens
//! (`fdt_nop_node`); QEMU `raspi4b` does exactly that to the PCIe and GENET
//! nodes it does not model, and firmware that edits its tree the same way
//! leaves the same holes. The `fdt` crate (0.1.5) cannot walk past a NOP that
//! sits between a node's properties and its next child — `skip_current_node`
//! asserts `bad node` — so the boot hart panics in the first `/cpus` or
//! `/memory` lookup. NOPs carry no information (devicetree spec §5.4.1), so the
//! tree is compacted once, in place, before anything parses it.

/// `FDT_BEGIN_NODE`, `FDT_END_NODE`, `FDT_PROP`, `FDT_NOP`, `FDT_END`.
const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;
const END: u32 = 9;

const MAGIC: u32 = 0xd00d_feed;
/// `size_dt_struct` exists from header version 17.
const FIRST_VERSION_WITH_STRUCT_SIZE: u32 = 17;
const HEADER_LEN: usize = 40;

/// Why a blob was left untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripError {
    /// Not an FDT header, or a version without `size_dt_struct`.
    Header,
    /// The structure block runs past the blob or holds an unknown token.
    Structure,
}

fn be32(blob: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        blob.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

const fn align4(len: usize) -> usize {
    (len + 3) & !3
}

/// Length of the token at `at` in the structure block `block`, NOP included.
fn token_len(block: &[u8], at: usize) -> Result<(u32, usize), StripError> {
    let token = be32(block, at).ok_or(StripError::Structure)?;
    let len = match token {
        BEGIN_NODE => {
            let name = block.get(at + 4..).ok_or(StripError::Structure)?;
            let nul = name
                .iter()
                .position(|&b| b == 0)
                .ok_or(StripError::Structure)?;
            4 + align4(nul + 1)
        }
        PROP => {
            let value_len = be32(block, at + 4).ok_or(StripError::Structure)? as usize;
            12usize
                .checked_add(align4(value_len))
                .ok_or(StripError::Structure)?
        }
        END_NODE | NOP | END => 4,
        _ => return Err(StripError::Structure),
    };
    if at.checked_add(len).is_none_or(|end| end > block.len()) {
        return Err(StripError::Structure);
    }
    Ok((token, len))
}

/// The blob's `totalsize`, if `header` starts with an FDT header.
pub fn total_size(header: &[u8]) -> Option<usize> {
    if be32(header, 0)? != MAGIC {
        return None;
    }
    be32(header, 4).map(|size| size as usize)
}

/// Remove every `FDT_NOP` from `blob`'s structure block, shrinking
/// `size_dt_struct`; returns the number of bytes removed.
///
/// The structure block is validated in full before the first byte moves, so
/// a malformed tree is left exactly as it was. Bytes freed at the block's end
/// are filled with `FDT_NOP` again — no parser reads past `FDT_END`, and the
/// strings block keeps its offset.
pub fn strip(blob: &mut [u8]) -> Result<usize, StripError> {
    if blob.len() < HEADER_LEN
        || be32(blob, 0) != Some(MAGIC)
        || be32(blob, 20).is_none_or(|version| version < FIRST_VERSION_WITH_STRUCT_SIZE)
    {
        return Err(StripError::Header);
    }
    let start = be32(blob, 8).ok_or(StripError::Header)? as usize;
    let size = be32(blob, 36).ok_or(StripError::Header)? as usize;
    let end = start.checked_add(size).ok_or(StripError::Structure)?;
    let block = blob.get_mut(start..end).ok_or(StripError::Structure)?;

    // Validation pass: every token parses and the walk ends on FDT_END.
    let mut at = 0;
    let mut nops = 0;
    loop {
        let (token, len) = token_len(block, at)?;
        if token == NOP {
            nops += 1;
        }
        at += len;
        if token == END {
            break;
        }
    }
    if nops == 0 {
        return Ok(0);
    }

    let mut read = 0;
    let mut write = 0;
    loop {
        // Validated above, so the walk cannot fail here.
        let (token, len) = token_len(block, read).map_err(|_| StripError::Structure)?;
        if token != NOP {
            block.copy_within(read..read + len, write);
            write += len;
        }
        read += len;
        if token == END {
            break;
        }
    }
    let removed = read - write;
    for word in block[write..read].chunks_exact_mut(4) {
        word.copy_from_slice(&NOP.to_be_bytes());
    }
    blob[36..40].copy_from_slice(&((size - removed) as u32).to_be_bytes());
    Ok(removed)
}
