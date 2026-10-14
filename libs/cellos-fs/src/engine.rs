//! Core CellosFS Engine — Unified Filesystem Manager.

#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
use crate::allocator::BitmapAllocator;
use crate::disk::{BlockDevice, FsError};
#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
use crate::format::{
    DirEntryRecord, Extent, InodeRecord, Superblock, BLOCK_SIZE, FIRST_USABLE_BLOCK,
    INODES_PER_BLOCK, INODE_FLAG_INLINE, INODE_MODE_DIR, INODE_MODE_FILE, INODE_PAYLOAD_SIZE,
    INODE_SIZE, SUPERBLOCK_A_BLOCK, SUPERBLOCK_B_BLOCK,
};
#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
use crate::inode::{read_inode, write_inode, DirPayload};
#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
use alloc::collections::BTreeMap;
#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(all(feature = "g2-cow-experiment", not(target_os = "none")))]
use crate::format::{InodeRecord, Superblock, BLOCK_SIZE, INODE_MODE_DIR, INODE_MODE_FILE};
#[cfg(all(feature = "g2-cow-experiment", not(target_os = "none")))]
use alloc::string::String;

#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
pub struct CellosFs<D: BlockDevice> {
    disk: D,
    active_sb: Superblock,
    active_slot: u8, // 0 for A, 1 for B
    allocator: BitmapAllocator,
    dirty_inodes: BTreeMap<u64, InodeRecord>,
}
#[cfg(not(all(feature = "g2-cow-experiment", not(target_os = "none"))))]
impl<D: BlockDevice> CellosFs<D> {
    /// Format a device with CellosFS.
    pub fn format(mut disk: D, total_blocks: u64) -> Result<Self, FsError> {
        if total_blocks != disk.total_blocks() {
            return Err(FsError::PartitionOverflow);
        }
        if total_blocks < 16 {
            return Err(FsError::NoSpace);
        }

        // Calculate bitmap blocks: 1 block covers 32,768 blocks (128 MiB)
        let bitmap_blocks = u32::try_from(total_blocks.div_ceil(32_768))
            .map_err(|_| FsError::NoSpace)?;
        let mut allocator = BitmapAllocator::new(FIRST_USABLE_BLOCK, bitmap_blocks, total_blocks)?;

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

        disk.read_block(SUPERBLOCK_A_BLOCK, &mut buf_a)?;
        disk.read_block(SUPERBLOCK_B_BLOCK, &mut buf_b)?;
        let sb_a = Superblock::from_bytes(&buf_a);
        let sb_b = Superblock::from_bytes(&buf_b);
        for sb in [sb_a, sb_b].into_iter().flatten() {
            if !sb.validate_geometry(disk.total_blocks()) {
                return Err(FsError::Corrupted);
            }
        }

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
        // A selected metadata pointer cannot be simultaneously available for allocation.
        // This checks existing bits only; it does not reconstruct allocation or repair cuts.
        for &block in &active_sb.inode_blocks {
            if block != 0 && !allocator.is_allocated(block)? {
                return Err(FsError::Corrupted);
            }
        }

        let mut fs = Self {
            disk,
            active_sb,
            active_slot,
            allocator,
            dirty_inodes: BTreeMap::new(),
        };
        if !fs.root_inode()?.is_dir() {
            return Err(FsError::Corrupted);
        }
        Ok(fs)
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
        if inode_num == 0 || inode_num >= self.active_sb.next_inode_num {
            return Err(FsError::Corrupted);
        }
        let record = if let Some(record) = self.dirty_inodes.get(&inode_num) {
            *record
        } else {
            let slot = ((inode_num - 1) / INODES_PER_BLOCK as u64) as usize;
            let block = *self.active_sb.inode_blocks.get(slot).ok_or(FsError::Corrupted)?;
            if block == 0 {
                return Err(FsError::Corrupted);
            }
            let idx = ((inode_num - 1) % INODES_PER_BLOCK as u64) as usize;
            read_inode(&mut self.disk, block, idx)?
        };
        if record.inode_num != inode_num {
            return Err(FsError::Corrupted);
        }
        self.validate_inode(&record)?;
        Ok(record)
    }

    fn ensure_mutable(&self) -> Result<(), FsError> {
        self.active_sb.sequence.checked_add(1).ok_or(FsError::NoSpace)?;
        Ok(())
    }

    fn validate_inode(&self, inode: &InodeRecord) -> Result<(), FsError> {
        if inode.is_dir() {
            if !inode.is_inline() || inode.extent_count as usize > INODE_PAYLOAD_SIZE / DirEntryRecord::SIZE {
                return Err(FsError::Corrupted);
            }
            for i in 0..inode.extent_count as usize {
                let off = i * DirEntryRecord::SIZE;
                let bytes = inode.payload[off..off + DirEntryRecord::SIZE].try_into()
                    .map_err(|_| FsError::Corrupted)?;
                let entry = DirEntryRecord::from_bytes(bytes).ok_or(FsError::Corrupted)?;
                if entry.inode_num == 0 || entry.inode_num >= self.active_sb.next_inode_num
                    || !matches!(entry.file_type, 1 | 2)
                    || entry.name_len == 0
                    || core::str::from_utf8(&entry.name[..entry.name_len as usize]).is_err()
                    || entry.name_as_str().contains('/')
                {
                    return Err(FsError::Corrupted);
                }
                for previous in 0..i {
                    let off = previous * DirEntryRecord::SIZE;
                    let bytes = inode.payload[off..off + DirEntryRecord::SIZE].try_into()
                        .map_err(|_| FsError::Corrupted)?;
                    let other = DirEntryRecord::from_bytes(bytes).ok_or(FsError::Corrupted)?;
                    if other.name_as_str() == entry.name_as_str() {
                        return Err(FsError::Corrupted);
                    }
                }
            }
        } else if inode.is_file() {
            if inode.is_inline() {
                if inode.size > INODE_PAYLOAD_SIZE as u64 || inode.inline_size as u64 != inode.size
                    || inode.extent_count != 0
                {
                    return Err(FsError::Corrupted);
                }
            } else {
                self.validate_extents(inode)?;
            }
        } else {
            return Err(FsError::Corrupted);
        }
        Ok(())
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
            if (entry.file_type == 2) != current_inode.is_dir() {
                return Err(FsError::Corrupted);
            }
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
        self.ensure_mutable()?;
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
        let next_inode_num = new_inode_num.checked_add(1).ok_or(FsError::NoSpace)?;
        if new_inode_num == 0
            || new_inode_num > self.active_sb.inode_blocks.len() as u64 * INODES_PER_BLOCK as u64
        {
            return Err(FsError::NoSpace);
        }
        let file_type = if mode == INODE_MODE_DIR { 2 } else { 1 };
        let dir_entry =
            DirEntryRecord::new(new_inode_num, file_type, leaf_name).ok_or(FsError::InvalidName)?;

        let mut count = parent_inode.extent_count as usize;
        DirPayload::insert(&mut parent_inode.payload, &mut count, &dir_entry)?;
        parent_inode.extent_count = count as u32;
        self.active_sb.next_inode_num = next_inode_num;

        let new_inode = InodeRecord::new(new_inode_num, mode);
        self.put_inode(new_inode);
        self.put_inode(parent_inode);
        Ok(new_inode)
    }

    /// Read file content, returning the complete requested in-file range or an error.
    pub fn read_file(&mut self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        let inode = self.lookup(path)?;
        if !inode.is_file() {
            return Err(FsError::IsADirectory);
        }
        if offset >= inode.size {
            return Ok(0);
        }
        let to_read = buf.len().min(usize::try_from(inode.size - offset).unwrap_or(usize::MAX));
        if inode.is_inline() {
            let start = offset as usize;
            buf[..to_read].copy_from_slice(&inode.payload[start..start + to_read]);
            return Ok(to_read);
        }
        let mut bytes_read = 0;
        while bytes_read < to_read {
            let position = offset + bytes_read as u64;
            let logical = (position / BLOCK_SIZE as u64) as u32;
            let block_offset = (position % BLOCK_SIZE as u64) as usize;
            let physical = self.mapped_block(&inode, logical)?;
            let mut block = [0u8; BLOCK_SIZE];
            self.disk.read_block(physical, &mut block)?;
            let count = (BLOCK_SIZE - block_offset).min(to_read - bytes_read);
            buf[bytes_read..bytes_read + count].copy_from_slice(&block[block_offset..block_offset + count]);
            bytes_read += count;
        }
        Ok(bytes_read)
    }

    /// Check the V1 logical mapping representation without disk I/O or mutation.
    pub fn validate_write_range(offset: u64, length: usize) -> Result<u64, FsError> {
        let end = offset.checked_add(u64::try_from(length).map_err(|_| FsError::InvalidOffset)?)
            .ok_or(FsError::InvalidOffset)?;
        if end > u32::MAX as u64 * BLOCK_SIZE as u64 {
            return Err(FsError::InvalidOffset);
        }
        Ok(end)
    }

    /// Positional write preserves bytes outside the range and the old length.
    /// Extending past EOF zero-fills the gap; an empty write is a no-op.
    pub fn write_file(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, FsError> {
        self.ensure_mutable()?;
        let end = Self::validate_write_range(offset, data.len())?;
        let inode = self.lookup(path)?;
        self.write_content(inode, offset, end, data, false)
    }

    /// Exact whole-file replacement, including truncation to zero bytes.
    /// This is functional in-place mutation, not a crash-safe transaction.
    pub fn replace_file(&mut self, path: &str, data: &[u8]) -> Result<usize, FsError> {
        self.ensure_mutable()?;
        let end = Self::validate_write_range(0, data.len())?;
        let inode = self.lookup(path)?;
        self.write_content(inode, 0, end, data, true)
    }

    fn write_content(&mut self, mut inode: InodeRecord, offset: u64, end: u64,
        data: &[u8], replace: bool) -> Result<usize, FsError>
    {
        if !inode.is_file() {
            return Err(FsError::IsADirectory);
        }
        if !replace && data.is_empty() {
            return Ok(0);
        }
        let size = if replace { end } else { inode.size.max(end) };
        let needed = u32::try_from(size.div_ceil(BLOCK_SIZE as u64))
            .map_err(|_| FsError::InvalidOffset)?;
        if needed as u64 > self.active_sb.total_blocks {
            return Err(FsError::NoSpace);
        }
        let was_inline = inode.is_inline();
        let old_size = if replace { 0 } else { inode.size };
        let mut extents = if was_inline { Vec::new() } else { self.read_extents(&inode)? };
        if size <= INODE_PAYLOAD_SIZE as u64 && (replace || was_inline) {
            for ext in &extents {
                for b in 0..ext.block_count {
                    self.allocator.free_block(ext.physical_block + b as u64)?;
                }
            }
            if replace {
                inode.payload.fill(0);
            } else if offset > inode.size {
                inode.payload[inode.size as usize..offset as usize].fill(0);
            }
            inode.payload[offset as usize..end as usize].copy_from_slice(data);
            inode.payload[size as usize..].fill(0);
            inode.size = size;
            inode.inline_size = size as u32;
            inode.extent_count = 0;
            inode.flags |= INODE_FLAG_INLINE;
            self.put_inode(inode);
            return Ok(data.len());
        }
        let current = extents.last().map_or(0, |e| e.logical_block + e.block_count);
        if needed > current && (needed - current) as u64 > self.allocator.free_block_count() {
            return Err(FsError::NoSpace);
        }
        for logical in current..needed {
            let physical = self.allocator.allocate_block()?;
            if let Some(last) = extents.last_mut() {
                if last.physical_block + last.block_count as u64 == physical {
                    last.block_count = last.block_count.checked_add(1).ok_or(FsError::InvalidOffset)?;
                    continue;
                }
            }
            if extents.len() == INODE_PAYLOAD_SIZE / Extent::SIZE {
                return Err(FsError::NoSpace);
            }
            extents.push(Extent { logical_block: logical, block_count: 1, physical_block: physical });
        }
        // Include gap blocks and block zero during inline-to-extent conversion.
        let first = if was_inline { 0 } else { offset.min(old_size) / BLOCK_SIZE as u64 };
        for logical in first..end.div_ceil(BLOCK_SIZE as u64) {
            let start = logical * BLOCK_SIZE as u64;
            let stop = start + BLOCK_SIZE as u64;
            let mut block = [0u8; BLOCK_SIZE];
            let physical = Self::extent_block(&extents, logical as u32)?;
            let preserved_end = old_size.min(stop);
            if was_inline && logical == 0 && old_size != 0 {
                block[..old_size as usize].copy_from_slice(&inode.payload[..old_size as usize]);
            } else if !was_inline && start < old_size
                && (offset > start || end < preserved_end)
            {
                self.disk.read_block(physical, &mut block)?;
            }
            if end > old_size {
                let gap_start = old_size.max(start);
                let gap_end = offset.min(stop);
                if gap_start < gap_end {
                    block[(gap_start - start) as usize..(gap_end - start) as usize].fill(0);
                }
            }
            let copy_start = offset.max(start);
            let copy_end = end.min(stop);
            if copy_start < copy_end {
                let source = (copy_start - offset) as usize;
                let count = (copy_end - copy_start) as usize;
                block[(copy_start - start) as usize..(copy_end - start) as usize]
                    .copy_from_slice(&data[source..source + count]);
            }
            self.disk.write_block(physical, &block)?;
        }
        // Tail retirement is immediate; crash-preserving retirement is out of scope.
        if replace {
            for ext in &mut extents {
                let keep = needed.saturating_sub(ext.logical_block).min(ext.block_count);
                for b in keep..ext.block_count {
                    self.allocator.free_block(ext.physical_block + b as u64)?;
                }
                ext.block_count = keep;
            }
            extents.retain(|e| e.block_count != 0);
        }
        inode.flags &= !INODE_FLAG_INLINE;
        inode.inline_size = 0;
        inode.size = size;
        inode.extent_count = extents.len() as u32;
        inode.payload.fill(0);
        for (i, ext) in extents.iter().enumerate() {
            let off = i * Extent::SIZE;
            inode.payload[off..off + Extent::SIZE].copy_from_slice(&ext.to_bytes());
        }
        self.put_inode(inode);
        Ok(data.len())
    }

    fn extent_at(inode: &InodeRecord, index: usize) -> Extent {
        let off = index * Extent::SIZE;
        let mut bytes = [0u8; Extent::SIZE];
        bytes.copy_from_slice(&inode.payload[off..off + Extent::SIZE]);
        Extent::from_bytes(&bytes)
    }

    fn validate_extents(&self, inode: &InodeRecord) -> Result<(), FsError> {
        if inode.extent_count as usize > INODE_PAYLOAD_SIZE / Extent::SIZE {
            return Err(FsError::Corrupted);
        }
        let mut logical_end = 0u32;
        for i in 0..inode.extent_count as usize {
            let ext = Self::extent_at(inode, i);
            let end = ext.physical_block.checked_add(ext.block_count as u64).ok_or(FsError::Corrupted)?;
            if ext.block_count == 0 || ext.logical_block != logical_end
                || ext.physical_block < self.active_sb.free_bitmap_block + self.active_sb.free_bitmap_blocks as u64
                || end > self.active_sb.total_blocks
                || self.active_sb.inode_blocks.iter().any(|&b| b != 0 && b >= ext.physical_block && b < end)
            {
                return Err(FsError::Corrupted);
            }
            for previous in 0..i {
                let other = Self::extent_at(inode, previous);
                if ext.physical_block < other.physical_block + other.block_count as u64
                    && end > other.physical_block
                {
                    return Err(FsError::Corrupted);
                }
            }
            logical_end = logical_end.checked_add(ext.block_count).ok_or(FsError::Corrupted)?;
        }
        if inode.size.div_ceil(BLOCK_SIZE as u64) != logical_end as u64 {
            return Err(FsError::Corrupted);
        }
        Ok(())
    }

    fn read_extents(&self, inode: &InodeRecord) -> Result<Vec<Extent>, FsError> {
        self.validate_extents(inode)?;
        Ok((0..inode.extent_count as usize).map(|i| Self::extent_at(inode, i)).collect())
    }

    fn mapped_block(&self, inode: &InodeRecord, logical: u32) -> Result<u64, FsError> {
        for i in 0..inode.extent_count as usize {
            let ext = Self::extent_at(inode, i);
            if logical >= ext.logical_block && logical - ext.logical_block < ext.block_count {
                return Ok(ext.physical_block + (logical - ext.logical_block) as u64);
            }
        }
        Err(FsError::Corrupted)
    }

    fn extent_block(extents: &[Extent], logical: u32) -> Result<u64, FsError> {
        extents.iter().find(|e| logical >= e.logical_block && logical - e.logical_block < e.block_count)
            .map(|e| e.physical_block + (logical - e.logical_block) as u64)
            .ok_or(FsError::Corrupted)
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
            if is_dir != child_inode.is_dir() {
                return Err(FsError::Corrupted);
            }
            res.push((e.name_as_str().to_string(), is_dir, child_inode.size));
        }

        Ok(res)
    }

    /// Unlink (delete) a regular file.
    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> {
        self.ensure_mutable()?;
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
            let extents = self.read_extents(&child_inode)?;
            for ext in extents {
                for b in 0..ext.block_count {
                    self.allocator.free_block(ext.physical_block + b as u64)?;
                }
            }
        }

        self.put_inode(parent_inode);
        self.dirty_inodes.remove(&removed_inode_num);
        Ok(())
    }

    /// Remove an EMPTY directory at `path`.
    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        self.ensure_mutable()?;
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
        self.ensure_mutable()?;
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
        match self.lookup(new_trimmed) {
            Ok(_) => return Err(FsError::AlreadyExists),
            Err(FsError::NotFound) => {}
            Err(error) => return Err(error),
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
        let mut new_parent = self.lookup(new_parent_path)?;
        if !old_parent.is_dir() || !new_parent.is_dir() {
            return Err(FsError::NotADirectory);
        }
        // Insertion independently refuses duplicates, including equivalent path spellings.
        if DirPayload::find(&new_parent.payload, new_parent.extent_count as usize, new_leaf).is_some() {
            return Err(FsError::AlreadyExists);
        }
        let new_entry = DirEntryRecord::new(old_inode.inode_num, 1, new_leaf)
            .ok_or(FsError::InvalidName)?;
        let mut old_count = old_parent.extent_count as usize;
        DirPayload::remove(&mut old_parent.payload, &mut old_count, old_leaf)?;
        old_parent.extent_count = old_count as u32;
        if old_parent.inode_num == new_parent.inode_num {
            let mut count = old_parent.extent_count as usize;
            DirPayload::insert(&mut old_parent.payload, &mut count, &new_entry)?;
            old_parent.extent_count = count as u32;
            self.put_inode(old_parent);
        } else {
            let mut new_count = new_parent.extent_count as usize;
            DirPayload::insert(&mut new_parent.payload, &mut new_count, &new_entry)?;
            new_parent.extent_count = new_count as u32;
            self.put_inode(old_parent);
            self.put_inode(new_parent);
        }

        Ok(())
    }

    /// Publish metadata through the two existing barriers. This is not crash-preservation proof.
    pub fn sync(&mut self) -> Result<(), FsError> {
        self.ensure_mutable()?;
        let mut new_sb = self.active_sb;
        new_sb.sequence = new_sb.sequence.checked_add(1).ok_or(FsError::NoSpace)?;

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
                    self.disk.read_block(old_block, &mut block_buf)?;
                }

                for (idx, record) in records {
                    let offset = idx * INODE_SIZE;
                    block_buf[offset..offset + INODE_SIZE].copy_from_slice(&record.to_bytes());
                }

                self.disk.write_block(new_block, &block_buf)?;

                if old_block != 0 {
                    self.allocator.free_block(old_block)?;
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

/// The default-off host experiment preserves the existing library interface.
#[cfg(all(feature = "g2-cow-experiment", not(target_os = "none")))]
pub struct CellosFs<D: BlockDevice> {
    inner: crate::cow_experiment::Cow<D>,
}

#[cfg(all(feature = "g2-cow-experiment", not(target_os = "none")))]
impl<D: BlockDevice> CellosFs<D> {
    pub fn format(disk: D, total_blocks: u64) -> Result<Self, FsError> {
        crate::cow_experiment::Cow::format(disk, total_blocks).map(|inner| Self { inner })
    }
    pub fn open(disk: D) -> Result<Self, FsError> {
        crate::cow_experiment::Cow::open(disk).map(|inner| Self { inner })
    }
    pub fn superblock(&self) -> &Superblock { self.inner.superblock() }
    pub fn free_blocks(&self) -> u64 { self.inner.free_blocks() }
    pub fn into_disk(self) -> D { self.inner.into_disk() }
    pub fn lookup(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        self.inner.lookup(path)
    }
    pub fn create_file(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        self.inner.create(path, INODE_MODE_FILE)
    }
    pub fn create_dir(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        self.inner.create(path, INODE_MODE_DIR)
    }
    pub fn read_file(&mut self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        self.inner.read_file(path, offset, buf)
    }
    pub fn validate_write_range(offset: u64, length: usize) -> Result<u64, FsError> {
        let end = offset.checked_add(u64::try_from(length).map_err(|_| FsError::InvalidOffset)?)
            .ok_or(FsError::InvalidOffset)?;
        if end > u32::MAX as u64 * BLOCK_SIZE as u64 { return Err(FsError::InvalidOffset); }
        Ok(end)
    }
    pub fn write_file(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, FsError> {
        self.inner.write_file(path, offset, data, false)
    }
    pub fn replace_file(&mut self, path: &str, data: &[u8]) -> Result<usize, FsError> {
        self.inner.write_file(path, 0, data, true)
    }
    pub fn list_dir(&mut self, path: &str) -> Result<Vec<(String, bool, u64)>, FsError> {
        self.inner.list_dir(path)
    }
    pub fn unlink(&mut self, path: &str) -> Result<(), FsError> { self.inner.remove(path, false) }
    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> { self.inner.remove(path, true) }
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        self.inner.rename(old, new)
    }
    pub fn sync(&mut self) -> Result<(), FsError> { self.inner.sync() }
}
