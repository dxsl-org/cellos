use crate::fs::{InodeId, FsError};
use bitflags::bitflags;
use core::time::Duration;

use super::FileType;

// Simplified Uid/Gid for ViOS (initially)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Uid(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gid(pub u32);

impl Uid {
    pub const fn new(id: u32) -> Self { Self(id) }
    pub const fn new_root() -> Self { Self(0) }
    pub fn is_root(&self) -> bool { self.0 == 0 }
}

impl Gid {
    pub const fn new(id: u32) -> Self { Self(id) }
    pub const fn new_root_group() -> Self { Self(0) }
}

#[derive(Debug, Clone, Copy)]
pub enum CapabilitiesFlags {
    CapDacOverride,
    CapDacReadSearch,
    CapFowner,
}

// Simplified Capabilities stub
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    dummy_has_all: bool,
}

impl Capabilities {
    pub fn new_empty() -> Self { Self { dummy_has_all: false } }
    
    // Stub for tests
    pub fn new_cap(_flag: CapabilitiesFlags) -> Self { 
        Self { dummy_has_all: true }
    }
    
    pub fn is_capable(&self, _flag: CapabilitiesFlags) -> bool { self.dummy_has_all }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct AccessMode: i32 {
        /// Execution is permitted
        const X_OK = 1;
        /// Writing is permitted
        const W_OK = 2;
        /// Reading is permitted
        const R_OK = 4;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct FilePermissions: u16 {
        // Owner permissions
        const S_IRUSR = 0o400; // Read permission, owner
        const S_IWUSR = 0o200; // Write permission, owner
        const S_IXUSR = 0o100; // Execute/search permission, owner

        // Group permissions
        const S_IRGRP = 0o040; // Read permission, group
        const S_IWGRP = 0o020; // Write permission, group
        const S_IXGRP = 0o010; // Execute/search permission, group

        // Others permissions
        const S_IROTH = 0o004; // Read permission, others
        const S_IWOTH = 0o002; // Write permission, others
        const S_IXOTH = 0o001; // Execute/search permission, others

        // Special permissions
        const S_ISUID = 0o4000; // Set-user-ID on execution
        const S_ISGID = 0o2000; // Set-group-ID on execution
        const S_ISVTX = 0o1000; // Sticky bit
    }
}

/// Represents file metadata, similar to `stat`.
#[derive(Debug, Clone)]
pub struct FileAttr {
    pub id: InodeId,
    pub size: u64,
    pub block_size: u32,
    pub blocks: u64,
    pub atime: Duration, 
    pub btime: Duration, 
    pub mtime: Duration, 
    pub ctime: Duration, 
    pub file_type: FileType,
    pub mode: FilePermissions,
    pub nlinks: u32,
    pub uid: Uid,
    pub gid: Gid,
}

impl Default for FileAttr {
    fn default() -> Self {
        Self {
            id: InodeId::dummy(),
            size: 0,
            block_size: 0,
            blocks: 0,
            atime: Duration::new(0, 0),
            btime: Duration::new(0, 0),
            mtime: Duration::new(0, 0),
            ctime: Duration::new(0, 0),
            file_type: FileType::File,
            mode: FilePermissions::empty(),
            nlinks: 1,
            uid: Uid::new_root(),
            gid: Gid::new_root_group(),
        }
    }
}

type Result<T> = core::result::Result<T, FsError>;

impl FileAttr {
    pub fn check_access(
        &self,
        uid: Uid,
        gid: Gid,
        caps: Capabilities,
        requested_mode: AccessMode,
    ) -> Result<()> {
        // For filesystem related tasks, the CAP_DAC_OVERRIDE bypasses all permission checks.
        if caps.is_capable(CapabilitiesFlags::CapDacOverride) {
            return Ok(());
        }

        // root (UID 0) bypasses most permission checks. For execute, at
        // least one execute bit must be set.
        if uid.is_root() {
            if requested_mode.contains(AccessMode::X_OK) {
                // Root still needs at least one execute bit to be set for X_OK
                if self.mode.intersects(
                    FilePermissions::S_IXUSR | FilePermissions::S_IXGRP | FilePermissions::S_IXOTH,
                ) {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        // Determine which set of permission bits to use (owner, group, or other)
        let perms_to_check = if self.uid == uid {
            // User is the owner
            self.mode
        } else if self.gid == gid {
            // User is in the file's group. Shift group bits to align with owner bits for easier checking.
            FilePermissions::from_bits_truncate(self.mode.bits() << 3)
        } else {
            // Others. Shift other bits to align with owner bits.
            FilePermissions::from_bits_truncate(self.mode.bits() << 6)
        };

        if requested_mode.contains(AccessMode::R_OK)
            && !perms_to_check.contains(FilePermissions::S_IRUSR)
            && !caps.is_capable(CapabilitiesFlags::CapDacReadSearch)
        {
            return Err(FsError::Other(alloc::string::String::from("Permission denied (read)")));
        }
        if requested_mode.contains(AccessMode::W_OK)
            && !perms_to_check.contains(FilePermissions::S_IWUSR)
        {
            return Err(FsError::Other(alloc::string::String::from("Permission denied (write)")));
        }
        if requested_mode.contains(AccessMode::X_OK)
            && !perms_to_check.contains(FilePermissions::S_IXUSR)
            && (self.file_type != FileType::Directory 
                || !caps.is_capable(CapabilitiesFlags::CapDacReadSearch))
        {
            return Err(FsError::Other(alloc::string::String::from("Permission denied (execute)")));
        }

        Ok(())
    }
}
