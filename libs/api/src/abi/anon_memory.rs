// SPDX-License-Identifier: Apache-2.0
//! Tier-2 large anonymous memory syscall constants (`AnonMemory = 440`); ADR-0033, CP1 §6.

/// Operation: allocate / reserve anonymous memory.
pub const ANON_MAP: usize = 1;
/// Operation: unmap anonymous memory in range.
pub const ANON_UNMAP: usize = 2;
/// Operation: change protections of anonymous memory in range.
pub const ANON_PROTECT: usize = 3;

/// Protection flag: read access allowed.
pub const PROT_READ: usize = 1 << 0;
/// Protection flag: write access allowed.
pub const PROT_WRITE: usize = 1 << 1;

/// MAP flag: map at exact fixed address.
pub const ANON_FIXED: usize = 1 << 8;

/// Maximum bytes one MAP may eagerly commit (`prot != 0`): 64 MiB. A
/// reservation (`prot == 0`) commits nothing and is bounded by the window only.
pub const ANON_COMMIT_MAX_BYTES: usize = 64 * 1024 * 1024;

/// Error sentinel for `AnonMemory` (syscall 440): range touches a
/// non-anonymous mapping.
pub const ERR_PERMISSION_DENIED: usize = usize::MAX;
/// Error sentinel: page export-locked or VA pending invalidation.
pub const ERR_TRY_AGAIN: usize = usize::MAX - 1;
/// Error sentinel: malformed op, range, or flags.
pub const ERR_INVALID_INPUT: usize = usize::MAX - 3;
/// Error sentinel: caller not an x86_64 private root, root dying, or no budget.
pub const ERR_NOT_SUPPORTED: usize = usize::MAX - 5;
/// Error sentinel: budget, frames, window space, or extent table exhausted.
pub const ERR_OUT_OF_MEMORY: usize = usize::MAX - 6;

/// Decoded request operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnonOp {
    /// Reserve (`read == false`) or reserve and commit; `fixed` places it at `addr`.
    Map {
        fixed: bool,
        read: bool,
        write: bool,
    },
    /// Release the caller's anonymous pages in range.
    Unmap,
    /// Change protection of the caller's anonymous pages in range.
    Protect { read: bool, write: bool },
}

/// Decode and validate registers `(a0=op, a1=addr, a2=len, a3=flags)` for `AnonMemory`.
pub fn decode_anon_request(
    op: usize,
    addr: usize,
    len: usize,
    flags: usize,
) -> Result<AnonOp, usize> {
    const PAGE_SIZE: usize = 4096;
    if len == 0 || !len.is_multiple_of(PAGE_SIZE) {
        return Err(ERR_INVALID_INPUT);
    }
    if !addr.is_multiple_of(PAGE_SIZE) || addr.checked_add(len).is_none() {
        return Err(ERR_INVALID_INPUT);
    }
    match op {
        ANON_MAP => {
            let fixed = (flags & ANON_FIXED) != 0;
            let read = (flags & PROT_READ) != 0;
            let write = (flags & PROT_WRITE) != 0;
            let unknown = flags & !(ANON_FIXED | PROT_READ | PROT_WRITE);
            if unknown != 0 || (write && !read) {
                return Err(ERR_INVALID_INPUT);
            }
            if !fixed && addr != 0 {
                return Err(ERR_INVALID_INPUT);
            }
            if read && len > ANON_COMMIT_MAX_BYTES {
                return Err(ERR_INVALID_INPUT);
            }
            Ok(AnonOp::Map { fixed, read, write })
        }
        ANON_UNMAP => {
            if flags != 0 {
                return Err(ERR_INVALID_INPUT);
            }
            Ok(AnonOp::Unmap)
        }
        ANON_PROTECT => {
            let read = (flags & PROT_READ) != 0;
            let write = (flags & PROT_WRITE) != 0;
            let unknown = flags & !(PROT_READ | PROT_WRITE);
            if unknown != 0 || (write && !read) {
                return Err(ERR_INVALID_INPUT);
            }
            Ok(AnonOp::Protect { read, write })
        }
        _ => Err(ERR_INVALID_INPUT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anon_map_decode() {
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, 4096, PROT_READ | PROT_WRITE),
            Ok(AnonOp::Map {
                fixed: false,
                read: true,
                write: true
            })
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0x1000, 4096, PROT_READ | ANON_FIXED),
            Ok(AnonOp::Map {
                fixed: true,
                read: true,
                write: false
            })
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, 4096, PROT_WRITE),
            Err(ERR_INVALID_INPUT)
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, 4096, 0x4),
            Err(ERR_INVALID_INPUT)
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0x1000, 4096, PROT_READ),
            Err(ERR_INVALID_INPUT)
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, 0, PROT_READ),
            Err(ERR_INVALID_INPUT)
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, 4095, PROT_READ),
            Err(ERR_INVALID_INPUT)
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 1, 4096, ANON_FIXED | PROT_READ),
            Err(ERR_INVALID_INPUT)
        );
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, ANON_COMMIT_MAX_BYTES + 4096, PROT_READ),
            Err(ERR_INVALID_INPUT)
        );
        // A reservation commits nothing, so the per-op commit cap does not apply.
        assert_eq!(
            decode_anon_request(ANON_MAP, 0, ANON_COMMIT_MAX_BYTES + 4096, 0),
            Ok(AnonOp::Map {
                fixed: false,
                read: false,
                write: false
            })
        );
    }

    #[test]
    fn test_anon_overflowing_range_refused() {
        let top = usize::MAX - 4095;
        for op in [ANON_MAP, ANON_UNMAP, ANON_PROTECT] {
            let flags = if op == ANON_MAP { ANON_FIXED } else { 0 };
            assert_eq!(
                decode_anon_request(op, top, 8192, flags),
                Err(ERR_INVALID_INPUT)
            );
        }
        assert_eq!(
            decode_anon_request(ANON_UNMAP, top - 4096, 4096, 0),
            Ok(AnonOp::Unmap)
        );
    }

    #[test]
    fn test_anon_unmap_decode() {
        assert_eq!(
            decode_anon_request(ANON_UNMAP, 0x2000, 4096, 0),
            Ok(AnonOp::Unmap)
        );
        assert_eq!(
            decode_anon_request(ANON_UNMAP, 0x2000, 4096, 1),
            Err(ERR_INVALID_INPUT)
        );
    }

    #[test]
    fn test_anon_protect_decode() {
        assert_eq!(
            decode_anon_request(ANON_PROTECT, 0x2000, 4096, PROT_READ),
            Ok(AnonOp::Protect {
                read: true,
                write: false
            })
        );
        assert_eq!(
            decode_anon_request(ANON_PROTECT, 0x2000, 4096, PROT_READ | PROT_WRITE),
            Ok(AnonOp::Protect {
                read: true,
                write: true
            })
        );
        assert_eq!(
            decode_anon_request(ANON_PROTECT, 0x2000, 4096, PROT_WRITE),
            Err(ERR_INVALID_INPUT)
        );
    }

    #[test]
    fn test_anon_invalid_op() {
        assert_eq!(decode_anon_request(99, 0, 4096, 0), Err(ERR_INVALID_INPUT));
    }
}
