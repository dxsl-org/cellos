//! Core CellosFS Engine — Unified Filesystem Manager.

use crate::allocator::BitmapAllocator;
use crate::disk::{BlockDevice, FsError};
use crate::format::{
    DirEntryRecord, Extent, InodeRecord, Superblock, BLOCK_SIZE, FIRST_USABLE_BLOCK,
    INODES_PER_BLOCK, INODE_FLAG_INLINE, INODE_MODE_DIR, INODE_MODE_FILE, INODE_PAYLOAD_SIZE,
    INODE_SIZE, SUPERBLOCK_A_BLOCK, SUPERBLOCK_B_BLOCK,
};
use crate::inode::{read_inode, write_inode, DirPayload};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct CellosFs<D: BlockDevice> {
    disk: D,
    active_sb: Superblock,
    active_slot: u8, // 0 for A, 1 for B
    allocator: BitmapAllocator,
    dirty_inodes: BTreeMap<u64, InodeRecord>,
}
impl<D: BlockDevice> CellosFs<D> {
    /// Format a device with CellosFS.
    pub fn format(mut disk: D, total_blocks: u64) -> Result<Self, FsError> {
        if total_blocks < 16 {
            return Err(FsError::NoSpace);
        }

        // Calculate bitmap blocks: 1 block covers 32,768 blocks (128 MiB)
        let bitmap_blocks = ((total_blocks + 32767) / 32768) as u32;
        let mut allocator = BitmapAllocator::new(FIRST_USABLE_BLOCK, bitmap_blocks, total_blocks);

        // Allocate root inode block (FIRST_USABLE_BLOCK + bitmap_blocks)
        let root_inode_block = allocator.allocate_block()?;

        // Write root inode (inode 1)
        let mut root_inode = InodeRecord::new(1, INODE_MODE_DIR);
        write_inode(&mut disk, root_inode_block, 0, &mut root_inode)?;

        // Write empty Superblock B (slot 1)
        let empty_block = [0u8; BLOCK_SIZE];
        disk.write_block(SUPERBLOCK_B_BLOCK, &empty_block)?;

        // Create and write Superblock A (slot 0) with sequence 1
        let mut sb = Superblock::new(total_blocks, bitmap_blocks);
        sb.root_inode_block = root_inode_block;
        sb.sequence = 1;
        sb.update_checksum();
        disk.write_block(SUPERBLOCK_A_BLOCK, &sb.to_bytes())?;

        // Save allocator
        allocator.save_to_disk(&mut disk)?;
        disk.flush()?;

        Ok(Self {
            disk,
            active_sb: sb,
            active_slot: 0,
            allocator,
            dirty_inodes: BTreeMap::new(),
        })
    }

    /// Open an existing CellosFS volume.
    /// Evaluates both Superblock A and B, selecting the valid one with highest sequence.
    pub fn open(mut disk: D) -> Result<Self, FsError> {
        let mut buf_a = [0u8; BLOCK_SIZE];
        let mut buf_b = [0u8; BLOCK_SIZE];

        let sb_a = disk
            .read_block(SUPERBLOCK_A_BLOCK, &mut buf_a)
            .ok()
            .and_then(|_| Superblock::from_bytes(&buf_a));

        let sb_b = disk
            .read_block(SUPERBLOCK_B_BLOCK, &mut buf_b)
            .ok()
            .and_then(|_| Superblock::from_bytes(&buf_b));

        let (active_sb, active_slot) = match (sb_a, sb_b) {
            (Some(a), Some(b)) => {
                if a.sequence >= b.sequence {
                    (a, 0)
                } else {
                    (b, 1)
                }
            }
            (Some(a), None) => (a, 0),
            (None, Some(b)) => (b, 1),
            (None, None) => return Err(FsError::Corrupted),
        };

        let allocator = BitmapAllocator::load_from_disk(
            &mut disk,
            active_sb.free_bitmap_block,
            active_sb.free_bitmap_blocks,
            active_sb.total_blocks,
        )?;

        Ok(Self {
            disk,
            active_sb,
            active_slot,
            allocator,
            dirty_inodes: BTreeMap::new(),
        })
    }

    /// Return reference to the active Superblock.
    pub fn superblock(&self) -> &Superblock {
        &self.active_sb
    }

    /// Return current number of free blocks in the allocator.
    pub fn free_blocks(&self) -> u64 {
        self.allocator.free_block_count()
    }

    /// Extract the underlying block device.
    pub fn into_disk(self) -> D {
        self.disk
    }
    fn get_inode(&mut self, inode_num: u64) -> Result<InodeRecord, FsError> {
        if let Some(record) = self.dirty_inodes.get(&inode_num) {
            return Ok(*record);
        }
        let slot = ((inode_num - 1) / (INODES_PER_BLOCK as u64)) as usize;
        if slot >= self.active_sb.inode_blocks.len() {
            return Err(FsError::NotFound);
        }
        let block = self.active_sb.inode_blocks[slot];
        if block == 0 {
            return Err(FsError::NotFound);
        }
        let idx = ((inode_num - 1) % (INODES_PER_BLOCK as u64)) as usize;
        read_inode(&mut self.disk, block, idx)
    }

    fn put_inode(&mut self, record: InodeRecord) {
        self.dirty_inodes.insert(record.inode_num, record);
    }

    /// Read the root directory inode.
    fn root_inode(&mut self) -> Result<InodeRecord, FsError> {
        self.get_inode(1)
    }

    /// Lookup an Inode by absolute path (e.g. "/foo.txt" or "/dir/bar.txt").
    pub fn lookup(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return self.root_inode();
        }

        let mut current_inode = self.root_inode()?;

        for component in trimmed.split('/') {
            if component.is_empty() {
                continue;
            }
            if !current_inode.is_dir() {
                return Err(FsError::NotADirectory);
            }

            let entry = DirPayload::find(
                &current_inode.payload,
                current_inode.extent_count as usize,
                component,
            )
            .ok_or(FsError::NotFound)?;
            current_inode = self.get_inode(entry.inode_num)?;
        }

        Ok(current_inode)
    }

    /// Create a regular file at `path`.
    pub fn create_file(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        self.create_entry(path, INODE_MODE_FILE)
    }

    /// Create a directory at `path`.
    pub fn create_dir(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        self.create_entry(path, INODE_MODE_DIR)
    }

    fn create_entry(&mut self, path: &str, mode: u16) -> Result<InodeRecord, FsError> {
        let trimmed = path.trim_matches('/');
        if trimmed.is_empty() {
            return Err(FsError::AlreadyExists);
        }

        let (parent_path, leaf_name) = match trimmed.rfind('/') {
            Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
            None => ("", trimmed),
        };

        let mut parent_inode = self.lookup(parent_path)?;
        if !parent_inode.is_dir() {
            return Err(FsError::NotADirectory);
        }

        if DirPayload::find(
            &parent_inode.payload,
            parent_inode.extent_count as usize,
            leaf_name,
        )
        .is_some()
        {
            return Err(FsError::AlreadyExists);
        }

        let new_inode_num = self.active_sb.next_inode_num;
        let slot = ((new_inode_num - 1) / (INODES_PER_BLOCK as u64)) as usize;
        if slot >= self.active_sb.inode_blocks.len() {
            return Err(FsError::NoSpace);
        }
        self.active_sb.next_inode_num += 1;
        let file_type = if mode == INODE_MODE_DIR { 2 } else { 1 };
        let dir_entry =
            DirEntryRecord::new(new_inode_num, file_type, leaf_name).ok_or(FsError::InvalidName)?;

        let mut count = parent_inode.extent_count as usize;
        DirPayload::insert(&mut parent_inode.payload, &mut count, &dir_entry)?;
        parent_inode.extent_count = count as u32;

        let new_inode = InodeRecord::new(new_inode_num, mode);
        self.put_inode(new_inode);
        self.put_inode(parent_inode);
        Ok(new_inode)
    }

    /// Read file content.
    pub fn read_file(&mut self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let inode = self.lookup(path)?;
        if !inode.is_file() {
            return Err(FsError::IsADirectory);
        }

        if offset >= inode.size {
            return Ok(0);
        }

        let available = (inode.size - offset) as usize;
        let to_read = buf.len().min(available);

        if inode.is_inline() {
            let start = offset as usize;
            buf[..to_read].copy_from_slice(&inode.payload[start..start + to_read]);
            return Ok(to_read);
        }

        // Extent-based reading
        let extents = self.read_extents(&inode);
        let mut bytes_read = 0usize;

        for ext in extents {
            let ext_start_bytes = (ext.logical_block as u64) * (BLOCK_SIZE as u64);
            let ext_len_bytes = (ext.block_count as u64) * (BLOCK_SIZE as u64);
            let ext_end_bytes = ext_start_bytes + ext_len_bytes;

            let cur_offset = offset + bytes_read as u64;
            if cur_offset >= ext_start_bytes && cur_offset < ext_end_bytes {
                let inside_offset = (cur_offset - ext_start_bytes) as usize;
                let chunk_size = (ext_len_bytes as usize - inside_offset).min(to_read - bytes_read);

                let start_phys_block = ext.physical_block + (inside_offset / BLOCK_SIZE) as u64;
                let block_offset = inside_offset % BLOCK_SIZE;

                let mut tmp = [0u8; BLOCK_SIZE];
                self.disk.read_block(start_phys_block, &mut tmp)?;

                let copy_len = chunk_size.min(BLOCK_SIZE - block_offset);
                buf[bytes_read..bytes_read + copy_len]
                    .copy_from_slice(&tmp[block_offset..block_offset + copy_len]);

                bytes_read += copy_len;
                if bytes_read >= to_read {
                    break;
                }
            }
        }

        Ok(bytes_read)
    }

    /// Write file content (inline or extent-allocated).
    pub fn write_file(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, FsError> {
        let mut inode = self.lookup(path)?;
        if !inode.is_file() {
            return Err(FsError::IsADirectory);
        }

        let new_size = offset + data.len() as u64;

        if new_size <= INODE_PAYLOAD_SIZE as u64 {
            // Can fit inline!
            let start = offset as usize;
            inode.payload[start..start + data.len()].copy_from_slice(data);
            inode.size = inode.size.max(new_size);
            inode.inline_size = inode.size as u32;
            inode.flags |= INODE_FLAG_INLINE;

            self.put_inode(inode);
            return Ok(data.len());
        }

        // Transition to extents if previously inline
        if inode.is_inline() {
            if inode.size > 0 {
                let old_data = inode.payload[..inode.size as usize].to_vec();
                let new_block = self.allocator.allocate_block()?;
                let mut block_buf = [0u8; BLOCK_SIZE];
                block_buf[..old_data.len()].copy_from_slice(&old_data);
                self.disk.write_block(new_block, &block_buf)?;

                let ext = Extent {
                    logical_block: 0,
                    block_count: 1,
                    physical_block: new_block,
                };
                inode.extent_count = 1;
                inode.payload.fill(0);
                inode.payload[..Extent::SIZE].copy_from_slice(&ext.to_bytes());
            } else {
                inode.payload.fill(0);
                inode.extent_count = 0;
            }
            inode.flags &= !INODE_FLAG_INLINE;
        }
        // Allocate additional blocks for incoming data
        let blocks_needed = ((new_size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64) as u32;
        let mut extents = self.read_extents(&inode);

        let current_blocks: u32 = extents.iter().map(|e| e.block_count).sum();
        if blocks_needed > current_blocks {
            let to_alloc = blocks_needed - current_blocks;
            for i in 0..to_alloc {
                let phys = self.allocator.allocate_block()?;
                extents.push(Extent {
                    logical_block: current_blocks + i,
                    block_count: 1,
                    physical_block: phys,
                });
            }
        }

        // Write incoming data to the appropriate blocks
        let mut written = 0usize;
        for ext in &extents {
            let ext_start_bytes = (ext.logical_block as u64) * (BLOCK_SIZE as u64);
            let ext_len_bytes = (ext.block_count as u64) * (BLOCK_SIZE as u64);
            let ext_end_bytes = ext_start_bytes + ext_len_bytes;

            let cur_offset = offset + written as u64;
            if cur_offset >= ext_start_bytes && cur_offset < ext_end_bytes {
                let inside_offset = (cur_offset - ext_start_bytes) as usize;
                let chunk_size = (ext_len_bytes as usize - inside_offset).min(data.len() - written);

                let phys_block = ext.physical_block + (inside_offset / BLOCK_SIZE) as u64;
                let block_offset = inside_offset % BLOCK_SIZE;

                let mut tmp = [0u8; BLOCK_SIZE];
                if block_offset > 0 || chunk_size < BLOCK_SIZE {
                    let _ = self.disk.read_block(phys_block, &mut tmp);
                }

                let copy_len = chunk_size.min(BLOCK_SIZE - block_offset);
                tmp[block_offset..block_offset + copy_len]
                    .copy_from_slice(&data[written..written + copy_len]);
                self.disk.write_block(phys_block, &tmp)?;

                written += copy_len;
                if written >= data.len() {
                    break;
                }
            }
        }

        // Save extents and update Inode
        inode.size = inode.size.max(new_size);
        inode.extent_count = extents.len() as u32;
        inode.payload.fill(0);
        for (i, ext) in extents.iter().enumerate() {
            let off = i * Extent::SIZE;
            if off + Extent::SIZE <= INODE_PAYLOAD_SIZE {
                inode.payload[off..off + Extent::SIZE].copy_from_slice(&ext.to_bytes());
            }
        }
        self.put_inode(inode);

        Ok(written)
    }

    fn read_extents(&self, inode: &InodeRecord) -> Vec<Extent> {
        let mut list = Vec::new();
        for i in 0..inode.extent_count as usize {
            let off = i * Extent::SIZE;
            if off + Extent::SIZE <= INODE_PAYLOAD_SIZE {
                let mut b = [0u8; Extent::SIZE];
                b.copy_from_slice(&inode.payload[off..off + Extent::SIZE]);
                list.push(Extent::from_bytes(&b));
            }
        }
        list
    }

    /// List directory contents: returns vector of `(entry_name, is_dir, size)`.
    pub fn list_dir(&mut self, path: &str) -> Result<Vec<(String, bool, u64)>, FsError> {
        let inode = self.lookup(path)?;
        if !inode.is_dir() {
            return Err(FsError::NotADirectory);
        }

        let entries = DirPayload::list(&inode.payload, inode.extent_count as usize);
        let mut res = Vec::new();

        for e in entries {
            let is_dir = e.file_type == 2;
            let child_inode = self.get_inode(e.inode_num)?;
            res.push((e.name_as_str().to_string(), is_dir, child_inode.size));
        }

        Ok(res)
    }

    /// Unlink (delete) a regular file.
    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        let trimmed = path.trim_matches('/');
        let (parent_path, leaf_name) = match trimmed.rfind('/') {
            Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
            None => ("", trimmed),
        };

        let child_inode = self.lookup(trimmed)?;
        if child_inode.is_dir() {
            return Err(FsError::IsADirectory);
        }

        let mut parent_inode = self.lookup(parent_path)?;
        if !parent_inode.is_dir() {
            return Err(FsError::NotADirectory);
        }

        let mut count = parent_inode.extent_count as usize;
        let removed_inode_num =
            DirPayload::remove(&mut parent_inode.payload, &mut count, leaf_name)?;
        parent_inode.extent_count = count as u32;

        if !child_inode.is_inline() {
            let extents = self.read_extents(&child_inode);
            for ext in extents {
                for b in 0..ext.block_count {
                    let _ = self.allocator.free_block(ext.physical_block + b as u64);
                }
            }
        }

        self.put_inode(parent_inode);
        self.dirty_inodes.remove(&removed_inode_num);
        Ok(())
    }

    /// Remove an EMPTY directory at `path`.
    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        let trimmed = path.trim_matches('/');
        let (parent_path, leaf_name) = match trimmed.rfind('/') {
            Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
            None => ("", trimmed),
        };

        let child_inode = self.lookup(trimmed)?;
        if !child_inode.is_dir() {
            return Err(FsError::NotADirectory);
        }

        if child_inode.extent_count > 0 {
            return Err(FsError::AlreadyExists); // Directory not empty
        }

        let mut parent_inode = self.lookup(parent_path)?;
        if !parent_inode.is_dir() {
            return Err(FsError::NotADirectory);
        }

        let mut count = parent_inode.extent_count as usize;
        let removed_inode_num =
            DirPayload::remove(&mut parent_inode.payload, &mut count, leaf_name)?;
        parent_inode.extent_count = count as u32;

        self.put_inode(parent_inode);
        self.dirty_inodes.remove(&removed_inode_num);
        Ok(())
    }
    /// Atomic non-replacing rename of a file from `old_path` to `new_path`.
    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), FsError> {
        let old_trimmed = old_path.trim_matches('/');
        let new_trimmed = new_path.trim_matches('/');

        if old_trimmed.is_empty() || new_trimmed.is_empty() {
            return Err(FsError::InvalidName);
        }

        let old_inode = self.lookup(old_trimmed)?;
        if old_inode.is_dir() {
            return Err(FsError::IsADirectory);
        }

        if old_trimmed == new_trimmed {
            return Ok(());
        }

        // Check target does not exist
        if self.lookup(new_trimmed).is_ok() {
            return Err(FsError::AlreadyExists);
        }
        let (old_parent_path, old_leaf) = match old_trimmed.rfind('/') {
            Some(pos) => (&old_trimmed[..pos], &old_trimmed[pos + 1..]),
            None => ("", old_trimmed),
        };

        let (new_parent_path, new_leaf) = match new_trimmed.rfind('/') {
            Some(pos) => (&new_trimmed[..pos], &new_trimmed[pos + 1..]),
            None => ("", new_trimmed),
        };

        let mut old_parent = self.lookup(old_parent_path)?;
        if !old_parent.is_dir() {
            return Err(FsError::NotADirectory);
        }

        let mut old_count = old_parent.extent_count as usize;
        let inode_num = DirPayload::remove(&mut old_parent.payload, &mut old_count, old_leaf)?;
        old_parent.extent_count = old_count as u32;

        let child_inode = self.get_inode(inode_num)?;
        let file_type = if child_inode.is_dir() { 2 } else { 1 };
        let new_entry =
            DirEntryRecord::new(inode_num, file_type, new_leaf).ok_or(FsError::InvalidName)?;

        if old_parent_path == new_parent_path {
            let mut count = old_parent.extent_count as usize;
            DirPayload::insert(&mut old_parent.payload, &mut count, &new_entry)?;
            old_parent.extent_count = count as u32;
            self.put_inode(old_parent);
        } else {
            let mut new_parent = self.lookup(new_parent_path)?;
            if !new_parent.is_dir() {
                return Err(FsError::NotADirectory);
            }
            let mut new_count = new_parent.extent_count as usize;
            DirPayload::insert(&mut new_parent.payload, &mut new_count, &new_entry)?;
            new_parent.extent_count = new_count as u32;

            self.put_inode(old_parent);
            self.put_inode(new_parent);
        }

        Ok(())
    }

    /// Commit active state and advance cyclic Superblock slot (A <-> B) via Copy-on-Write.
    pub fn sync(&mut self) -> Result<(), FsError> {
        let mut new_sb = self.active_sb;

        if !self.dirty_inodes.is_empty() {
            // Group dirty inodes by table block slot (each slot covers 8 inodes)
            let mut slots: BTreeMap<usize, Vec<(usize, InodeRecord)>> = BTreeMap::new();
            for (&inode_num, record) in &mut self.dirty_inodes {
                let slot = ((inode_num - 1) / (INODES_PER_BLOCK as u64)) as usize;
                let idx = ((inode_num - 1) % (INODES_PER_BLOCK as u64)) as usize;
                record.update_checksum();
                slots.entry(slot).or_default().push((idx, *record));
            }

            for (slot, records) in slots {
                if slot >= new_sb.inode_blocks.len() {
                    return Err(FsError::NoSpace);
                }
                let old_block = new_sb.inode_blocks[slot];
                let new_block = self.allocator.allocate_block()?;

                let mut block_buf = [0u8; BLOCK_SIZE];
                if old_block != 0 {
                    let _ = self.disk.read_block(old_block, &mut block_buf);
                }

                for (idx, record) in records {
                    let offset = idx * INODE_SIZE;
                    block_buf[offset..offset + INODE_SIZE].copy_from_slice(&record.to_bytes());
                }

                self.disk.write_block(new_block, &block_buf)?;

                if old_block != 0 {
                    let _ = self.allocator.free_block(old_block);
                }
                new_sb.inode_blocks[slot] = new_block;
            }

            self.dirty_inodes.clear();
        }

        new_sb.root_inode_block = new_sb.inode_blocks[0];

        // Save allocator bitmap changes
        self.allocator.save_to_disk(&mut self.disk)?;
        self.disk.flush()?;

        // Prepare new Superblock with incremented sequence
        new_sb.sequence += 1;
        new_sb.free_block_count = self.allocator.free_block_count();
        new_sb.update_checksum();

        // Alternate slot: 0 -> write to slot 1 (B); 1 -> write to slot 0 (A)
        let target_slot = 1 - self.active_slot;
        let target_block = if target_slot == 0 {
            SUPERBLOCK_A_BLOCK
        } else {
            SUPERBLOCK_B_BLOCK
        };

        self.disk.write_block(target_block, &new_sb.to_bytes())?;
        self.disk.flush()?;

        self.active_sb = new_sb;
        self.active_slot = target_slot;
        Ok(())
    }
}
