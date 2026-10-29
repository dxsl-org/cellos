//! CellosFS Native — Pure-Rust extent filesystem.
//!
//! Designed for the Cellos Single Address Space (SAS) microkernel.
//!
//! Features:
//! - Dual alternating superblocks with sequence numbers and CRC.
//! - Packed 512-byte Inodes with small-file inlining (< 428 bytes).
//! - Extent-based B-tree allocation for NVMe and SSD speed with low tree depth.
//! - Strict partition bounds enforcement (`BoundedDisk`).
//! - SAS Grant zero-copy compatibility.
//!
//! Not crash-consistent: there is no journal or mount recovery, the allocation bitmap
//! is overwritten before the superblock, and freed blocks can be reused before a
//! commit (Phase 09 S9-4a audit). [`CellosFs::sync_durable`] therefore always reports
//! [`DurabilityUnavailable`]; see `.agents/261025-cellosfs-crash-safety/plan.md`.

#![no_std]

extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;

#[cfg(all(feature = "g2-cow-experiment", not(target_os = "none")))]
mod cow_experiment;
pub mod allocator;
pub mod crc32;
pub mod disk;
pub mod engine;
pub mod format;
pub mod inode;

pub use disk::{BlockDevice, BoundedDisk, FsError, MemDisk};
pub use engine::{CellosFs, DurabilityUnavailable, Expect};
pub use format::{
    DirEntryRecord, Extent, InodeRecord, Superblock, BLOCK_SIZE, FORMAT_VERSION, INODE_FLAG_INLINE,
    INODE_MODE_DIR, INODE_MODE_FILE, INODE_SIZE, MAGIC,
};
