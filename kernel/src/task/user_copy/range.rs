//! Validated user-range vocabulary for the copy boundary: typed slices,
//! the execution view, and the recoverable error. No allocation, no
//! dereferences — construction is pure arithmetic validation.

use crate::memory::address_space::AddressSpace;
use types::ViError;

/// Upper bound of the canonical user half for every view this kernel serves.
/// Anything at or above this boundary is non-canonical or kernel-owned and is
/// rejected before any mapping is consulted.
const USER_LIMIT: usize = 1usize << 38;

/// Recoverable failure of the copy transaction. Deliberately a single variant:
/// every rejection — bad arithmetic, missing mapping, missing permission, a
/// domain caught mid-retirement, or a recovered in-window fault — maps to the
/// same recoverable invalid-address ABI error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyError {
    /// Null / overflow / non-canonical / kernel-range / unmapped /
    /// no-permission / dying-domain / recovered guard fault.
    InvalidAddress,
}

impl CopyError {
    // The phase-03 shared contract fixes this exact signature (`&self`).
    #[allow(clippy::wrong_self_convention, dead_code)]
    pub(crate) fn to_abi(&self) -> ViError {
        ViError::InvalidInput
    }
}

/// Execution view for one copy. `Sas` keeps today's shared-address-space
/// semantics (no raw-pointer bypasses elsewhere); `Domain` carries the pinned
/// `Arc<AddressSpace>` derived from the task's `TaskAddressSpace::Domain`
/// binding (same source as `DomainRef::from_task`).
#[derive(Clone)]
pub(crate) enum CopyView {
    Sas,
    Domain(alloc::sync::Arc<AddressSpace>),
}

/// Validated user read range; construction rejects null (where disallowed),
/// overflow, non-canonical, and kernel-range addresses. No allocation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UserReadSlice {
    ptr: usize,
    len: usize,
}

/// Validated user write range; construction rules identical to
/// [`UserReadSlice`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct UserWriteSlice {
    ptr: usize,
    len: usize,
}

impl UserReadSlice {
    pub(crate) fn new(ptr: usize, len: usize, allow_empty: bool) -> Result<Self, CopyError> {
        validate_range(ptr, len, allow_empty).map(|_| Self { ptr, len })
    }
    pub(crate) fn ptr(&self) -> usize {
        self.ptr
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

impl UserWriteSlice {
    pub(crate) fn new(ptr: usize, len: usize, allow_empty: bool) -> Result<Self, CopyError> {
        validate_range(ptr, len, allow_empty).map(|_| Self { ptr, len })
    }
    pub(crate) fn ptr(&self) -> usize {
        self.ptr
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

/// Arithmetic preflight shared by both slice constructors. Returns the checked
/// exclusive end so callers never re-add.
fn validate_range(ptr: usize, len: usize, allow_empty: bool) -> Result<usize, CopyError> {
    if len == 0 {
        if allow_empty {
            return Ok(ptr);
        }
        return Err(CopyError::InvalidAddress);
    }
    if ptr == 0 {
        return Err(CopyError::InvalidAddress);
    }
    let end = ptr.checked_add(len).ok_or(CopyError::InvalidAddress)?;
    if end > USER_LIMIT {
        return Err(CopyError::InvalidAddress);
    }
    Ok(end)
}

/// Direction of the byte movement through the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    /// Read from user memory into a kernel buffer.
    FromUser,
    /// Write from a kernel buffer into user memory.
    ToUser,
}
