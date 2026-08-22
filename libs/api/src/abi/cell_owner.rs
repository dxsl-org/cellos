//! Kernel-attested Cell lifetime endpoint ABI.
//!
//! A `CellOwner` is deliberately separate from the 32-byte receive trailer:
//! the trailer identifies the sending task and its principal, while this record
//! names the root task whose death terminates that principal.

/// Exact byte size written by `ResolveCellOwner` and `WatchCellOwner`.
pub const CELL_OWNER_LEN: usize = 32;

/// Exact byte size of an RV32-safe owner lookup request.
///
/// The legacy owner syscalls pass identity scalars in registers. RV32 cannot
/// carry either 64-bit scalar that way, so the additive record ABI carries both
/// values without changing the established owner response layout.
pub const CELL_OWNER_REQUEST_LEN: usize = 16;

/// Fixed request record for the RV32-safe owner lookup syscalls.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellOwnerRequest {
    pub cell_id: u64,
    pub generation: u64,
}

impl CellOwnerRequest {
    pub const fn new(cell_id: u64, generation: u64) -> Self {
        Self { cell_id, generation }
    }

    pub fn to_bytes(self) -> [u8; CELL_OWNER_REQUEST_LEN] {
        let mut out = [0; CELL_OWNER_REQUEST_LEN];
        out[0..8].copy_from_slice(&self.cell_id.to_le_bytes());
        out[8..16].copy_from_slice(&self.generation.to_le_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < CELL_OWNER_REQUEST_LEN {
            return None;
        }
        Some(Self {
            cell_id: u64::from_le_bytes(bytes[0..8].try_into().ok()?),
            generation: u64::from_le_bytes(bytes[8..16].try_into().ok()?),
        })
    }
}

/// Fixed, kernel-produced lifetime record for one live Cell generation.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellOwner {
    pub cell_id: u64,
    pub generation: u64,
    pub root_tid: u64,
    reserved: u64,
}

impl CellOwner {
    pub const fn new(cell_id: u64, generation: u64, root_tid: u64) -> Self {
        Self { cell_id, generation, root_tid, reserved: 0 }
    }

    pub const fn is_live(&self) -> bool {
        self.cell_id != 0 && self.generation != 0 && self.root_tid != 0 && self.reserved == 0
    }

    pub fn to_bytes(self) -> [u8; CELL_OWNER_LEN] {
        let mut out = [0; CELL_OWNER_LEN];
        out[0..8].copy_from_slice(&self.cell_id.to_le_bytes());
        out[8..16].copy_from_slice(&self.generation.to_le_bytes());
        out[16..24].copy_from_slice(&self.root_tid.to_le_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < CELL_OWNER_LEN {
            return None;
        }
        let word = |offset: usize| -> Option<u64> {
            Some(u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?))
        };
        let owner = Self {
            cell_id: word(0)?,
            generation: word(8)?,
            root_tid: word(16)?,
            reserved: word(24)?,
        };
        owner.is_live().then_some(owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_is_fixed_and_rejects_invalid_values() {
        let owner = CellOwner::new(3, 9, 71);
        assert_eq!(core::mem::size_of::<CellOwner>(), CELL_OWNER_LEN);
        assert_eq!(CellOwner::from_bytes(&owner.to_bytes()), Some(owner));
        assert!(CellOwner::from_bytes(&[0; CELL_OWNER_LEN]).is_none());
    }

    #[test]
    fn request_preserves_full_identity_words() {
        let request = CellOwnerRequest::new(0x89ab_cdef_0123_4567, 0xfeed_cafe_0123_4567);
        assert_eq!(core::mem::size_of::<CellOwnerRequest>(), CELL_OWNER_REQUEST_LEN);
        assert_eq!(CellOwnerRequest::from_bytes(&request.to_bytes()), Some(request));
    }
}
