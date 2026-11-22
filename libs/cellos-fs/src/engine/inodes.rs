//! Inode-number reuse and all-or-nothing staging.
//!
//! The inode table holds 256 numbers. Without reuse, every create (including
//! each D03 save stage) consumes one for good and the volume refuses creates
//! forever once the high-water mark is reached.
//!
//! Reuse must not reintroduce ABA for `(inode_num, generation)` observations:
//! a retired number keeps a tombstone record (mode `INODE_MODE_UNUSED`) whose
//! generation is past every state the live inode had, and a reused inode
//! starts after the tombstone. On-disk format is unchanged; older volumes,
//! whose unlinked inodes kept their last live record, are scanned at open.

use super::CellosFs;
use crate::allocator::BitmapAllocator;
use crate::disk::{BlockDevice, FsError};
use crate::format::{
    InodeRecord, Superblock, BLOCK_SIZE, INODES_PER_BLOCK, INODE_MODE_UNUSED, INODE_SIZE,
};
use crate::inode::DirPayload;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

/// In-memory state a request may stage before `sync`.
struct Staged {
    active_sb: Superblock,
    allocator: BitmapAllocator,
    dirty_inodes: BTreeMap<u64, InodeRecord>,
    free_inodes: BTreeMap<u64, u64>,
}

impl<D: BlockDevice> CellosFs<D> {
    /// Run `f`; if it fails, restore every in-memory change it staged.
    ///
    /// Before `sync` publishes a superblock, a request only changes memory and
    /// blocks free in the committed bitmap, so after a refusal (NotFound,
    /// NoSpace, a full directory, …) the volume is exactly as before and can
    /// keep serving. A storage fault ([`FsError::is_storage_fault`]) may have
    /// written in-place blocks; callers must stop using the instance then.
    pub fn atomically<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<R, FsError>,
    ) -> Result<R, FsError> {
        let staged = Staged {
            active_sb: self.active_sb,
            allocator: self.allocator.clone(),
            dirty_inodes: self.dirty_inodes.clone(),
            free_inodes: self.free_inodes.clone(),
        };
        let result = f(self);
        if result.is_err() {
            self.active_sb = staged.active_sb;
            self.allocator = staged.allocator;
            self.dirty_inodes = staged.dirty_inodes;
            self.free_inodes = staged.free_inodes;
        }
        result
    }

    /// Stage a tombstone for the unlinked `record` (its latest, possibly
    /// unsynced state) and make its number reusable.
    pub(super) fn retire_inode(&mut self, record: InodeRecord) {
        let mut tombstone = InodeRecord::new(record.inode_num, INODE_MODE_UNUSED);
        tombstone.generation = record.generation;
        self.put_inode(tombstone);
        let generation = self.dirty_inodes[&record.inode_num].generation;
        self.free_inodes.insert(record.inode_num, generation);
    }

    /// Numbers below `next_inode_num` that no directory reaches, with the
    /// generation of their last on-disk record. An unreadable table block or a
    /// malformed directory yields no candidates, and a number without a valid
    /// record is skipped: reuse needs proof the number is unreferenced and a
    /// generation to continue from.
    pub(super) fn scan_free_inodes(&mut self) -> BTreeMap<u64, u64> {
        self.try_scan_free_inodes().unwrap_or_default()
    }

    fn try_scan_free_inodes(&mut self) -> Result<BTreeMap<u64, u64>, FsError> {
        // `validate_geometry` bounds this by the 256-entry table + 1.
        let limit =
            usize::try_from(self.active_sb.next_inode_num).map_err(|_| FsError::Corrupted)?;
        let mut table: Vec<Option<InodeRecord>> = vec![None; limit];
        let blocks = self.active_sb.inode_blocks;
        for (slot, block) in blocks.into_iter().enumerate() {
            let first = slot * INODES_PER_BLOCK + 1;
            if block == 0 || first >= limit {
                continue;
            }
            let mut buf = [0u8; BLOCK_SIZE];
            self.disk.read_block(block, &mut buf)?;
            for (idx, bytes) in buf.as_chunks::<INODE_SIZE>().0.iter().enumerate() {
                let num = first + idx;
                if num >= limit {
                    break;
                }
                table[num] = InodeRecord::from_bytes(bytes).filter(|r| r.inode_num == num as u64);
            }
        }
        let mut reachable = vec![false; limit];
        reachable[1] = true;
        let mut pending = vec![1usize];
        while let Some(num) = pending.pop() {
            let dir = table[num].ok_or(FsError::Corrupted)?;
            if !dir.is_dir() {
                return Err(FsError::Corrupted);
            }
            self.validate_inode(&dir)?;
            for entry in DirPayload::list(&dir.payload, dir.extent_count as usize) {
                let child = usize::try_from(entry.inode_num).map_err(|_| FsError::Corrupted)?;
                if child >= limit {
                    return Err(FsError::Corrupted);
                }
                if !reachable[child] {
                    reachable[child] = true;
                    if entry.file_type == 2 {
                        pending.push(child);
                    }
                }
            }
        }
        let mut free = BTreeMap::new();
        for num in 2..limit {
            if let (false, Some(record)) = (reachable[num], table[num]) {
                free.insert(num as u64, record.generation);
            }
        }
        Ok(free)
    }
}
