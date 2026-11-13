//! Capability types for ViCell.
//!
//! A capability is an opaque, unforgeable token assigned by the kernel.
//! Holding a `CapId` grants the owner a bounded set of operations on a
//! specific resource (file, socket, surface, …).  The kernel validates
//! ownership on every use — callers cannot forge or guess valid IDs.

/// Opaque kernel-assigned capability identifier.
///
/// Values are assigned by the kernel cap registry and are unique per session.
/// `CapId(0)` is the null / invalid sentinel — the registry never assigns it.
#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct CapId(pub u64);

impl CapId {
    /// The invalid/null capability sentinel.
    pub const INVALID: Self = Self(0);

    /// Returns `true` if this is the null sentinel.
    pub fn is_invalid(self) -> bool {
        self.0 == 0
    }
}

/// Permissions encoded in a capability.
///
/// Packed as a `u32` for compact IPC transfer.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub struct CapPerms(pub u32);

impl CapPerms {
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const SEEK: Self = Self(1 << 2);

    /// Read + Seek (standard file-read access).
    pub const FILE_READ: Self = Self(Self::READ.0 | Self::SEEK.0);
    /// Read + Write + Seek (read-write file access).
    pub const FILE_RW: Self = Self(Self::READ.0 | Self::WRITE.0 | Self::SEEK.0);

    pub fn has(self, perm: Self) -> bool {
        (self.0 & perm.0) == perm.0
    }
}
