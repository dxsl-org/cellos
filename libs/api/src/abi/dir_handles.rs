// SPDX-License-Identifier: MPL-2.0
//! Directory-handle inheritance across a spawn.
//!
//! A directory handle is issued, validated and revoked by the filesystem
//! service. The kernel carries a set of them from a spawner to the cell it
//! spawns, and later states on its own authority which spawner supplied that
//! set. It never interprets a handle: it cannot tell a live one from a revoked
//! one and holds no opinion about what any of them refer to.
//!
//! ## Why the kernel is only a courier
//! Two records of the same authority drift, and drift in this direction widens
//! authority silently instead of failing to compile. The filesystem service
//! stays the only place a handle means anything — it checks the spawner
//! genuinely held every handle in the set before binding any of them to the
//! child.
//!
//! ## Invariants
//! - A set is bounded at [`MAX_SPAWN_DIR_HANDLES`] by the carrier's own shape,
//!   so no caller-supplied count ever sizes an allocation.
//! - Validation is all-or-nothing ([`DirHandleSet::from_carrier`]). A set that
//!   is silently narrowed is indistinguishable from the set the spawner meant to
//!   pass, so an over-long or malformed set fails the spawn instead.
//! - `0` is not a handle. It is what a zeroed struct carries, and accepting it
//!   would make "no set" and "a set of one unknown thing" the same value.
//! - A non-empty set always names the spawner it came from. A set with no
//!   attested source is worth nothing to the service that must check it.

/// Largest directory-handle set a spawn may carry.
///
/// The bound is structural: the carrier holds its handles inline, so a caller
/// cannot drive a kernel-side allocation with a count it chooses.
pub const MAX_SPAWN_DIR_HANDLES: usize = 8;

/// Layout version of [`ViSpawnDirHandles`]. A mismatch is rejected rather than
/// interpreted, so a future field can never be read as a handle.
pub const SPAWN_DIR_HANDLES_VERSION: u32 = 1;

/// A filesystem-service-issued directory handle, opaque to the kernel.
///
/// Deliberately not `CapId`: a kernel capability and a service-issued handle
/// share a representation and nothing else, and passing one where the other
/// belongs must be a compile error on a boundary whose purpose is that authority
/// cannot be mistaken.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViDirHandle(pub u64);

/// Caller-supplied carrier naming the directory handles a spawn should pass on.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ViSpawnDirHandles {
    /// Must equal [`SPAWN_DIR_HANDLES_VERSION`].
    pub version: u32,
    /// Number of leading entries of `handles` that are in use.
    pub count: u32,
    /// Handles to pass on; entries at or past `count` are ignored.
    pub handles: [u64; MAX_SPAWN_DIR_HANDLES],
}

impl ViSpawnDirHandles {
    /// A carrier naming no handles — what a spawner that inherits nothing sends.
    pub const EMPTY: Self = Self {
        version: SPAWN_DIR_HANDLES_VERSION,
        count: 0,
        handles: [0; MAX_SPAWN_DIR_HANDLES],
    };

    /// Build a carrier from the handles a spawner wants its child to receive.
    ///
    /// # Errors
    /// [`DirHandleSetError::TooMany`] when more than [`MAX_SPAWN_DIR_HANDLES`]
    /// handles are supplied. Truncating instead would hand the child a narrower
    /// set than the spawner asked for without either side being told.
    pub fn new(handles: &[ViDirHandle]) -> Result<Self, DirHandleSetError> {
        if handles.len() > MAX_SPAWN_DIR_HANDLES {
            return Err(DirHandleSetError::TooMany);
        }
        let mut out = Self::EMPTY;
        out.count = handles.len() as u32;
        for (slot, h) in out.handles.iter_mut().zip(handles) {
            *slot = h.0;
        }
        Ok(out)
    }
}

/// Why a carrier was refused. Every variant fails the spawn outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirHandleSetError {
    /// `version` is not [`SPAWN_DIR_HANDLES_VERSION`].
    UnsupportedVersion,
    /// More handles than [`MAX_SPAWN_DIR_HANDLES`].
    TooMany,
    /// A handle was `0`, which is the absent value and never a handle.
    ZeroHandle,
    /// The same handle appears twice, so the set does not say what it looks like
    /// it says about how many distinct directories the child receives.
    Duplicate,
}

/// A validated directory-handle set, as recorded against a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirHandleSet {
    count: u8,
    handles: [u64; MAX_SPAWN_DIR_HANDLES],
}

impl DirHandleSet {
    /// The set a cell spawned without a carrier receives.
    pub const EMPTY: Self = Self {
        count: 0,
        handles: [0; MAX_SPAWN_DIR_HANDLES],
    };

    /// Validate a caller-supplied carrier.
    ///
    /// # Errors
    /// See [`DirHandleSetError`]. Nothing is accepted partially: a carrier with
    /// one bad entry yields no set at all.
    pub fn from_carrier(carrier: &ViSpawnDirHandles) -> Result<Self, DirHandleSetError> {
        if carrier.version != SPAWN_DIR_HANDLES_VERSION {
            return Err(DirHandleSetError::UnsupportedVersion);
        }
        let count = carrier.count as usize;
        if count > MAX_SPAWN_DIR_HANDLES {
            return Err(DirHandleSetError::TooMany);
        }
        let mut out = Self::EMPTY;
        for i in 0..count {
            let h = carrier.handles[i];
            if h == 0 {
                return Err(DirHandleSetError::ZeroHandle);
            }
            if out.handles[..i].contains(&h) {
                return Err(DirHandleSetError::Duplicate);
            }
            out.handles[i] = h;
        }
        out.count = count as u8;
        Ok(out)
    }

    /// The handles in the set, in the order the spawner named them.
    pub fn as_slice(&self) -> &[u64] {
        &self.handles[..self.count as usize]
    }

    pub fn len(&self) -> usize {
        self.count as usize
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for DirHandleSet {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// What a task received from the cell that spawned it.
///
/// The spawner is recorded alongside the set because a set with no named source
/// cannot be checked: the filesystem service's question is "did *this* cell hold
/// *these* handles", and it has no way to ask it about a set that arrived
/// anonymously.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InheritedDirHandles {
    /// Cell that named the set. `0` when nothing was inherited.
    pub spawner_cell_id: u64,
    /// The spawner's generation at the moment of the spawn, so a set cannot be
    /// credited to a later cell that reused the id.
    pub spawner_generation: u64,
    /// The handles named. Empty whenever `spawner_cell_id` is `0`.
    pub set: DirHandleSet,
}

impl InheritedDirHandles {
    /// A task spawned without a carrier, and every task created before boot
    /// reaches userspace.
    pub const NONE: Self = Self {
        spawner_cell_id: 0,
        spawner_generation: 0,
        set: DirHandleSet::EMPTY,
    };
}

impl Default for InheritedDirHandles {
    fn default() -> Self {
        Self::NONE
    }
}
