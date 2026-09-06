//! CellosFS Native — Pure-Rust, Crash-Resilient Extent Filesystem.
//!
//! Designed for the Cellos Single Address Space (SAS) microkernel.
//!
//! Features:
//! - Dual cyclic Superblock ring for power-loss immunity without journal log replay.
//! - Packed 512-byte Inodes with small-file inlining (< 428 bytes).
//! - Extent-based B-tree allocation for NVMe and SSD speed with low tree depth.
//! - Strict partition bounds enforcement (`BoundedDisk`).
//! - SAS Grant zero-copy compatibility.

#![no_std]

extern crate alloc;

#[cfg(any(test, feature = "std"))]
extern crate std;

pub mod allocator;
pub mod crc32;
pub mod disk;
pub mod engine;
pub mod format;
pub mod inode;

pub use disk::{BlockDevice, BoundedDisk, FsError, MemDisk};
pub use engine::CellosFs;
pub use format::{
    DirEntryRecord, Extent, InodeRecord, Superblock, BLOCK_SIZE, FORMAT_VERSION, INODE_FLAG_INLINE,
    INODE_MODE_DIR, INODE_MODE_FILE, INODE_SIZE, MAGIC,
};
