//! Private, default-off host experiment. V1 bytes and public errors are unchanged.
//! No recursion, dynamic traversal maps, or bitmap-authoritative allocation.
//! Mutations stage a candidate; only successful FS2 in `sync` acknowledges it.

use crate::disk::{BlockDevice, FsError};
use crate::format::{DirEntryRecord, Extent, InodeRecord, Superblock, BLOCK_SIZE,
    FIRST_USABLE_BLOCK, INODES_PER_BLOCK, INODE_FLAG_INLINE, INODE_MODE_DIR,
    INODE_PAYLOAD_SIZE, INODE_SIZE};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const MAX_BLOCKS: usize = 64 * 1024 * 1024 / BLOCK_SIZE;
const BIT_BYTES: usize = MAX_BLOCKS / 8;
const MAX_INODES: usize = 32 * INODES_PER_BLOCK;
const MAX_EXTENTS: usize = INODE_PAYLOAD_SIZE / Extent::SIZE;
const MAX_ENTRIES: usize = INODE_PAYLOAD_SIZE / DirEntryRecord::SIZE;
const RESERVED_END: u64 = FIRST_USABLE_BLOCK + 1;
type Bits = [u8; BIT_BYTES];

fn has(bits: &Bits, block: u64) -> bool {
    bits[block as usize / 8] & (1 << (block % 8)) != 0
}
fn set(bits: &mut Bits, block: u64) { bits[block as usize / 8] |= 1 << (block % 8); }

// One fixed allocation keeps the large image out of return/init stack frames.
// Its capacity does not depend on disk geometry or mutation count.
pub(crate) struct Cow<D: BlockDevice> {
    disk: D,
    state: Box<Storage>,
}

struct Storage {
    roots: [Option<Superblock>; 2],
    active: Superblock,
    slot: usize,
    closed: bool,
    reachable: [Bits; 2],
    reservations: Bits,
    scratch: Bits,
    // Cross-root interpretations: table slot, or (inode, logical data block).
    roles: [u32; MAX_BLOCKS],
    inodes: [Option<InodeRecord>; MAX_INODES],
    seen: [bool; MAX_INODES],
    queue: [u16; MAX_INODES],
    tables: [u64; 32],
    dirty: u32,
    cursor: u64,
    free_count: u64,
    buffer: [u8; BLOCK_SIZE],
}

impl<D: BlockDevice> Cow<D> {
    fn empty(disk: D, active: Superblock) -> Self {
        let mut allocation = Box::<Storage>::new_uninit();
        let pointer = allocation.as_mut_ptr();
        // SAFETY: each field is initialized exactly once before assume_init;
        // arrays are written element-by-element, so no whole-image temporary
        // or invalid zero-representation assumption is used. These writes are
        // infallible and no reference to uninitialized Storage is constructed.
        unsafe {
            unsafe fn array<T: Copy, const N: usize>(pointer: *mut [T; N], value: T) {
                let elements = pointer.cast::<T>();
                for index in 0..N { elements.add(index).write(value); }
            }
            array(core::ptr::addr_of_mut!((*pointer).roots), None);
            core::ptr::addr_of_mut!((*pointer).active).write(active);
            core::ptr::addr_of_mut!((*pointer).slot).write(0);
            core::ptr::addr_of_mut!((*pointer).closed).write(false);
            array(core::ptr::addr_of_mut!((*pointer).reachable), [0; BIT_BYTES]);
            array(core::ptr::addr_of_mut!((*pointer).reservations), 0);
            array(core::ptr::addr_of_mut!((*pointer).scratch), 0);
            array(core::ptr::addr_of_mut!((*pointer).roles), 0);
            array(core::ptr::addr_of_mut!((*pointer).inodes), None);
            array(core::ptr::addr_of_mut!((*pointer).seen), false);
            array(core::ptr::addr_of_mut!((*pointer).queue), 0);
            array(core::ptr::addr_of_mut!((*pointer).tables), 0);
            core::ptr::addr_of_mut!((*pointer).dirty).write(0);
            core::ptr::addr_of_mut!((*pointer).cursor).write(RESERVED_END);
            core::ptr::addr_of_mut!((*pointer).free_count).write(0);
            array(core::ptr::addr_of_mut!((*pointer).buffer), 0);
            Self { disk, state: allocation.assume_init() }
        }
    }

    fn bound(total: u64) -> Result<(), FsError> {
        if !(16..=MAX_BLOCKS as u64).contains(&total) { return Err(FsError::NoSpace); }
        Ok(())
    }

    fn geometry(sb: &Superblock, total: u64) -> Result<(), FsError> {
        Self::bound(total)?;
        if !sb.validate_geometry(total) || sb.free_bitmap_block != FIRST_USABLE_BLOCK
            || sb.free_bitmap_blocks != 1 || sb.sequence == 0
        { return Err(FsError::Corrupted); }
        Ok(())
    }

    pub(crate) fn format(disk: D, total: u64) -> Result<Self, FsError> {
        if disk.total_blocks() != total { return Err(FsError::PartitionOverflow); }
        Self::bound(total)?;
        let mut fs = Self::empty(disk, Superblock::new(total, 1));
        fs.state.inodes[0] = Some(InodeRecord::new(1, INODE_MODE_DIR));
        fs.state.buffer.fill(0);
        fs.state.buffer[..INODE_SIZE].copy_from_slice(&fs.state.inodes[0].unwrap().to_bytes());
        fs.disk.write_block(RESERVED_END, &fs.state.buffer)?;
        fs.state.buffer.fill(0);
        fs.disk.write_block(1, &fs.state.buffer)?;
        for block in 0..=RESERVED_END { set(&mut fs.state.reachable[0], block); }
        fs.state.free_count = total - RESERVED_END - 1;
        fs.write_bitmap()?;
        fs.disk.flush()?; // FS1: bootstrap payload and advisory bitmap.
        fs.state.active.update_checksum();
        fs.disk.write_block(0, &fs.state.active.to_bytes())?;
        fs.disk.flush()?; // FS2: bootstrap publication.
        fs.state.roots[0] = Some(fs.state.active);
        Ok(fs)
    }

    pub(crate) fn open(disk: D) -> Result<Self, FsError> {
        Self::bound(disk.total_blocks())?;
        let total = disk.total_blocks();
        let mut fs = Self::empty(disk, Superblock::new(total, 1));
        // A read error is not an absent slot. Only failed V1 checksum/magic is.
        for slot in 0..2 {
            fs.disk.read_block(slot as u64, &mut fs.state.buffer)?;
            fs.state.roots[slot] = Superblock::from_bytes(&fs.state.buffer);
            if let Some(sb) = fs.state.roots[slot].as_ref() { Self::geometry(sb, total)?; }
        }
        fs.state.slot = match (&fs.state.roots[0], &fs.state.roots[1]) {
            (Some(a), Some(b)) => if a.sequence >= b.sequence { 0 } else { 1 },
            (Some(_), None) => 0,
            (None, Some(_)) => 1,
            (None, None) => return Err(FsError::Corrupted),
        };
        // Every checksum-valid root is semantically checked, even the older one.
        for slot in 0..2 {
            if let Some(sb) = fs.state.roots[slot] {
                fs.load_inodes(&sb)?;
                fs.validate_graph(&sb, true)?;
                fs.state.reachable[slot].copy_from_slice(&fs.state.scratch);
            }
        }
        fs.state.active = fs.state.roots[fs.state.slot].unwrap();
        let active = fs.state.active;
        fs.load_inodes(&active)?;
        fs.state.free_count = fs.count_free();
        Ok(fs)
    }

    fn load_inodes(&mut self, sb: &Superblock) -> Result<(), FsError> {
        self.state.inodes.fill(None);
        for slot in 0..32 {
            let block = sb.inode_blocks[slot];
            if block == 0 { continue; }
            self.disk.read_block(block, &mut self.state.buffer)?;
            for index in 0..INODES_PER_BLOCK {
                let number = slot * INODES_PER_BLOCK + index + 1;
                let off = index * INODE_SIZE;
                let bytes: &[u8; INODE_SIZE] = self.state.buffer[off..off + INODE_SIZE]
                    .try_into().map_err(|_| FsError::Corrupted)?;
                if bytes.iter().all(|&byte| byte == 0) { continue; }
                let record = InodeRecord::from_bytes(bytes).ok_or(FsError::Corrupted)?;
                if record.inode_num != number as u64 || record.inode_num >= sb.next_inode_num {
                    return Err(FsError::Corrupted);
                }
                self.state.inodes[number - 1] = Some(record);
            }
        }
        Ok(())
    }

    fn mark(&mut self, block: u64, role: u32, total: u64, cross: bool) -> Result<(), FsError> {
        if block < RESERVED_END || block >= total || has(&self.state.scratch, block) {
            return Err(FsError::Corrupted);
        }
        if cross {
            let old = self.state.roles[block as usize];
            if old != 0 && old != role { return Err(FsError::Corrupted); }
            self.state.roles[block as usize] = role;
        }
        set(&mut self.state.scratch, block);
        Ok(())
    }

    fn entry(inode: &InodeRecord, index: usize) -> Result<DirEntryRecord, FsError> {
        if index >= MAX_ENTRIES { return Err(FsError::Corrupted); }
        let off = index * DirEntryRecord::SIZE;
        DirEntryRecord::from_bytes(inode.payload[off..off + DirEntryRecord::SIZE]
            .try_into().map_err(|_| FsError::Corrupted)?).ok_or(FsError::Corrupted)
    }

    fn extent(inode: &InodeRecord, index: usize) -> Result<Extent, FsError> {
        if index >= MAX_EXTENTS { return Err(FsError::Corrupted); }
        let off = index * Extent::SIZE;
        Ok(Extent::from_bytes(inode.payload[off..off + Extent::SIZE]
            .try_into().map_err(|_| FsError::Corrupted)?))
    }

    fn validate_graph(&mut self, sb: &Superblock, cross: bool) -> Result<(), FsError> {
        Self::geometry(sb, self.disk.total_blocks())?;
        self.state.scratch.fill(0);
        self.state.seen.fill(false);
        for block in 0..RESERVED_END { set(&mut self.state.scratch, block); }
        for slot in 0..32 {
            if sb.inode_blocks[slot] != 0 {
                self.mark(sb.inode_blocks[slot], slot as u32 + 1, sb.total_blocks, cross)?;
            }
        }
        // Queue visits each inode once. Multiple incoming edges (including root
        // edges) reject aliases and cycles without recursive stack growth.
        self.state.queue[0] = 1;
        self.state.seen[0] = true;
        let mut head = 0;
        let mut tail = 1;
        while head < tail {
            let number = self.state.queue[head] as usize;
            head += 1;
            let inode = self.state.inodes[number - 1].ok_or(FsError::Corrupted)?;
            if inode.inode_num != number as u64 || inode.inode_num >= sb.next_inode_num
                || inode.flags & !INODE_FLAG_INLINE != 0 || inode.nlink != 1
            { return Err(FsError::Corrupted); }
            if inode.is_dir() {
                if !inode.is_inline() || inode.extent_count as usize > MAX_ENTRIES
                    || inode.size != 0 || inode.inline_size != 0
                { return Err(FsError::Corrupted); }
                for index in 0..inode.extent_count as usize {
                    let entry = Self::entry(&inode, index)?;
                    let name = entry.name_as_str();
                    if entry.inode_num == 0 || entry.inode_num >= sb.next_inode_num
                        || entry.inode_num > MAX_INODES as u64 || entry.name_len == 0
                        || core::str::from_utf8(&entry.name[..entry.name_len as usize]).is_err()
                        || name.contains('/') || name.contains('\0') || name == "." || name == ".."
                        || !matches!(entry.file_type, 1 | 2)
                    { return Err(FsError::Corrupted); }
                    for previous in 0..index {
                        if Self::entry(&inode, previous)?.name_as_str() == name {
                            return Err(FsError::Corrupted);
                        }
                    }
                    let child = entry.inode_num as usize - 1;
                    let record = self.state.inodes[child].ok_or(FsError::Corrupted)?;
                    if self.state.seen[child] || (entry.file_type == 2) != record.is_dir() {
                        return Err(FsError::Corrupted);
                    }
                    if tail >= MAX_INODES { return Err(FsError::Corrupted); }
                    self.state.seen[child] = true;
                    self.state.queue[tail] = entry.inode_num as u16;
                    tail += 1;
                }
            } else if inode.is_file() {
                if inode.is_inline() {
                    if inode.size > INODE_PAYLOAD_SIZE as u64 || inode.inline_size as u64 != inode.size
                        || inode.extent_count != 0 { return Err(FsError::Corrupted); }
                } else {
                    if inode.extent_count as usize > MAX_EXTENTS || inode.inline_size != 0 {
                        return Err(FsError::Corrupted);
                    }
                    let mut logical = 0u32;
                    for index in 0..inode.extent_count as usize {
                        let extent = Self::extent(&inode, index)?;
                        let end = extent.physical_block.checked_add(extent.block_count as u64)
                            .ok_or(FsError::Corrupted)?;
                        if extent.block_count == 0 || extent.logical_block != logical
                            || end > sb.total_blocks { return Err(FsError::Corrupted); }
                        for offset in 0..extent.block_count {
                            let data_index = logical.checked_add(offset).ok_or(FsError::Corrupted)?;
                            if data_index >= MAX_BLOCKS as u32 { return Err(FsError::Corrupted); }
                            let role = ((number as u32) << 16) | (data_index + 1);
                            self.mark(extent.physical_block + offset as u64, role, sb.total_blocks, cross)?;
                        }
                        logical = logical.checked_add(extent.block_count).ok_or(FsError::Corrupted)?;
                    }
                    if inode.size.div_ceil(BLOCK_SIZE as u64) != logical as u64 {
                        return Err(FsError::Corrupted);
                    }
                }
            } else { return Err(FsError::Corrupted); }
        }
        if !self.state.inodes[0].ok_or(FsError::Corrupted)?.is_dir() { return Err(FsError::Corrupted); }
        for index in 0..MAX_INODES {
            if self.state.inodes[index].is_some() && !self.state.seen[index] { return Err(FsError::Corrupted); }
        }
        Ok(())
    }

    fn admitted(&self) -> Result<(), FsError> {
        if self.state.closed { Err(FsError::Corrupted) } else { Ok(()) }
    }

    fn mutate<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, FsError>) -> Result<T, FsError> {
        self.admitted()?;
        let result = self.state.active.sequence.checked_add(1).ok_or(FsError::NoSpace)
            .and_then(|_| f(self));
        if result.is_err() { self.state.closed = true; }
        result
    }

    pub(crate) fn superblock(&self) -> &Superblock { &self.state.active }
    pub(crate) fn into_disk(self) -> D { self.disk }
    fn protected(&self, block: u64) -> bool {
        block < RESERVED_END || has(&self.state.reachable[0], block) || has(&self.state.reachable[1], block)
    }
    pub(crate) fn free_blocks(&self) -> u64 { self.state.free_count }
    fn count_free(&self) -> u64 {
        (RESERVED_END..self.state.active.total_blocks)
            .filter(|&block| !self.protected(block) && !has(&self.state.reservations, block)).count() as u64
    }

    // Explicit reservation events refuse aliases, including earlier unpublished
    // preparations. No freeing on failure or candidate supersession is allowed.
    fn reserve_at(&mut self, block: u64) -> Result<(), FsError> {
        if block >= self.state.active.total_blocks || self.protected(block)
            || has(&self.state.reservations, block) { return Err(FsError::Corrupted); }
        let remaining = self.state.free_count.checked_sub(1).ok_or(FsError::Corrupted)?;
        set(&mut self.state.reservations, block);
        self.state.free_count = remaining;
        Ok(())
    }
    fn reserve(&mut self) -> Result<u64, FsError> {
        while self.state.cursor < self.state.active.total_blocks {
            let block = self.state.cursor;
            self.state.cursor += 1;
            if !self.protected(block) && !has(&self.state.reservations, block) {
                self.reserve_at(block)?;
                return Ok(block);
            }
        }
        Err(FsError::NoSpace)
    }
    fn reserve_table(&mut self, number: u64) -> Result<(), FsError> {
        if number == 0 || number > MAX_INODES as u64 { return Err(FsError::NoSpace); }
        let slot = (number as usize - 1) / INODES_PER_BLOCK;
        if self.state.dirty & (1 << slot) == 0 {
            self.state.tables[slot] = self.reserve()?;
            self.state.dirty |= 1 << slot;
        }
        Ok(())
    }
    fn put(&mut self, mut record: InodeRecord) {
        record.update_checksum();
        self.state.inodes[record.inode_num as usize - 1] = Some(record);
    }

    fn find(inode: &InodeRecord, name: &str) -> Result<Option<DirEntryRecord>, FsError> {
        for index in 0..inode.extent_count as usize {
            let entry = Self::entry(inode, index)?;
            if entry.name_as_str() == name { return Ok(Some(entry)); }
        }
        Ok(None)
    }
    fn lookup_inner(&self, path: &str) -> Result<InodeRecord, FsError> {
        let mut inode = self.state.inodes[0].ok_or(FsError::Corrupted)?;
        for component in path.trim_matches('/').split('/').filter(|c| !c.is_empty()) {
            if !inode.is_dir() { return Err(FsError::NotADirectory); }
            let entry = Self::find(&inode, component)?.ok_or(FsError::NotFound)?;
            inode = self.state.inodes[entry.inode_num as usize - 1].ok_or(FsError::Corrupted)?;
        }
        Ok(inode)
    }
    pub(crate) fn lookup(&mut self, path: &str) -> Result<InodeRecord, FsError> {
        self.admitted()?;
        self.lookup_inner(path)
    }
    fn split(path: &str) -> Result<(&str, &str), FsError> {
        let path = path.trim_matches('/');
        if path.is_empty() { return Err(FsError::InvalidName); }
        let (parent, leaf) = match path.rfind('/') {
            Some(index) => (&path[..index], &path[index + 1..]),
            None => ("", path),
        };
        if leaf == "." || leaf == ".." || leaf.contains('\0') {
            return Err(FsError::InvalidName);
        }
        Ok((parent, leaf))
    }
    fn insert(parent: &mut InodeRecord, entry: DirEntryRecord) -> Result<(), FsError> {
        if Self::find(parent, entry.name_as_str())?.is_some() { return Err(FsError::AlreadyExists); }
        let index = parent.extent_count as usize;
        if index >= MAX_ENTRIES { return Err(FsError::NoSpace); }
        let off = index * DirEntryRecord::SIZE;
        parent.payload[off..off + DirEntryRecord::SIZE].copy_from_slice(&entry.to_bytes());
        parent.extent_count += 1;
        Ok(())
    }
    fn remove_entry(parent: &mut InodeRecord, name: &str) -> Result<u64, FsError> {
        let count = parent.extent_count as usize;
        for index in 0..count {
            let entry = Self::entry(parent, index)?;
            if entry.name_as_str() != name { continue; }
            let off = index * DirEntryRecord::SIZE;
            let last = (count - 1) * DirEntryRecord::SIZE;
            parent.payload.copy_within(last..last + DirEntryRecord::SIZE, off);
            parent.payload[last..last + DirEntryRecord::SIZE].fill(0);
            parent.extent_count -= 1;
            return Ok(entry.inode_num);
        }
        Err(FsError::NotFound)
    }

    pub(crate) fn create(&mut self, path: &str, mode: u16) -> Result<InodeRecord, FsError> {
        self.mutate(|fs| {
            if path.trim_matches('/').is_empty() { return Err(FsError::AlreadyExists); }
            let (parent_path, leaf) = Self::split(path)?;
            let mut parent = fs.lookup_inner(parent_path)?;
            if !parent.is_dir() { return Err(FsError::NotADirectory); }
            let number = fs.state.active.next_inode_num;
            let next = number.checked_add(1).ok_or(FsError::NoSpace)?;
            if number > MAX_INODES as u64 { return Err(FsError::NoSpace); }
            let entry = DirEntryRecord::new(number, if mode == INODE_MODE_DIR { 2 } else { 1 }, leaf)
                .ok_or(FsError::InvalidName)?;
            Self::insert(&mut parent, entry)?;
            fs.reserve_table(parent.inode_num)?;
            fs.reserve_table(number)?;
            let record = InodeRecord::new(number, mode);
            fs.put(parent);
            fs.put(record);
            fs.state.active.next_inode_num = next;
            Ok(record)
        })
    }

    fn mapped(inode: &InodeRecord, logical: u32) -> Result<u64, FsError> {
        for index in 0..inode.extent_count as usize {
            let extent = Self::extent(inode, index)?;
            if logical >= extent.logical_block && logical - extent.logical_block < extent.block_count {
                return Ok(extent.physical_block + (logical - extent.logical_block) as u64);
            }
        }
        Err(FsError::Corrupted)
    }
    pub(crate) fn read_file(&mut self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
        self.admitted()?;
        let inode = self.lookup_inner(path)?;
        if !inode.is_file() { return Err(FsError::IsADirectory); }
        if offset >= inode.size { return Ok(0); }
        let count = buf.len().min((inode.size - offset) as usize);
        if inode.is_inline() {
            buf[..count].copy_from_slice(&inode.payload[offset as usize..offset as usize + count]);
            return Ok(count);
        }
        let mut done = 0;
        while done < count {
            let position = offset + done as u64;
            let logical = (position / BLOCK_SIZE as u64) as u32;
            let block_off = (position % BLOCK_SIZE as u64) as usize;
            let block = Self::mapped(&inode, logical)?;
            if let Err(error) = self.disk.read_block(block, &mut self.state.buffer) {
                self.state.closed = true;
                return Err(error);
            }
            let part = (BLOCK_SIZE - block_off).min(count - done);
            buf[done..done + part].copy_from_slice(&self.state.buffer[block_off..block_off + part]);
            done += part;
        }
        Ok(count)
    }

    pub(crate) fn write_file(&mut self, path: &str, offset: u64, data: &[u8], replace: bool)
        -> Result<usize, FsError>
    {
        self.mutate(|fs| fs.write_inner(path, offset, data, replace))
    }
    fn write_inner(&mut self, path: &str, offset: u64, data: &[u8], replace: bool)
        -> Result<usize, FsError>
    {
        let end = crate::engine::CellosFs::<D>::validate_write_range(offset, data.len())?;
        let mut inode = self.lookup_inner(path)?;
        if !inode.is_file() { return Err(FsError::IsADirectory); }
        if !replace && data.is_empty() { return Ok(0); }
        let old = inode;
        let old_size = if replace { 0 } else { old.size };
        let size = if replace { end } else { old.size.max(end) };
        let needed = size.div_ceil(BLOCK_SIZE as u64);
        if needed > self.state.active.total_blocks { return Err(FsError::NoSpace); }
        self.reserve_table(inode.inode_num)?;
        if size <= INODE_PAYLOAD_SIZE as u64 {
            inode.payload.fill(0);
            if old_size != 0 {
                if old.is_inline() {
                    inode.payload[..old_size as usize].copy_from_slice(&old.payload[..old_size as usize]);
                } else {
                    self.disk.read_block(Self::mapped(&old, 0)?, &mut self.state.buffer)?;
                    inode.payload[..old_size as usize].copy_from_slice(&self.state.buffer[..old_size as usize]);
                }
            }
            inode.payload[offset as usize..end as usize].copy_from_slice(data);
            inode.flags = INODE_FLAG_INLINE;
            inode.inline_size = size as u32;
            inode.extent_count = 0;
            inode.size = size;
            self.put(inode);
            return Ok(data.len());
        }
        // Reserve the COMPLETE candidate before any data write. Prior candidates
        // stay reserved even if another staged write supersedes their mappings.
        if needed > self.free_blocks() { return Err(FsError::NoSpace); }
        let mut extents = [Extent::default(); MAX_EXTENTS];
        let mut count = 0usize;
        for logical in 0..needed as u32 {
            let block = self.reserve()?;
            if count != 0 && extents[count - 1].physical_block + extents[count - 1].block_count as u64 == block {
                extents[count - 1].block_count += 1;
            } else {
                if count == MAX_EXTENTS { return Err(FsError::NoSpace); }
                extents[count] = Extent { logical_block: logical, block_count: 1, physical_block: block };
                count += 1;
            }
        }
        inode.payload.fill(0);
        for index in 0..count {
            let off = index * Extent::SIZE;
            inode.payload[off..off + Extent::SIZE].copy_from_slice(&extents[index].to_bytes());
        }
        inode.flags = 0;
        inode.inline_size = 0;
        inode.extent_count = count as u32;
        inode.size = size;
        for logical in 0..needed as u32 {
            let start = logical as u64 * BLOCK_SIZE as u64;
            self.state.buffer.fill(0);
            if start < old_size {
                if old.is_inline() {
                    self.state.buffer[..old_size as usize].copy_from_slice(&old.payload[..old_size as usize]);
                } else {
                    self.disk.read_block(Self::mapped(&old, logical)?, &mut self.state.buffer)?;
                    let preserved = (old_size - start).min(BLOCK_SIZE as u64) as usize;
                    self.state.buffer[preserved..].fill(0);
                }
            }
            let copy_start = offset.max(start);
            let copy_end = end.min(start + BLOCK_SIZE as u64);
            if copy_start < copy_end {
                let source = (copy_start - offset) as usize;
                let len = (copy_end - copy_start) as usize;
                self.state.buffer[(copy_start - start) as usize..(copy_end - start) as usize]
                    .copy_from_slice(&data[source..source + len]);
            }
            self.disk.write_block(Self::mapped(&inode, logical)?, &self.state.buffer)?;
        }
        self.put(inode);
        Ok(data.len())
    }

    pub(crate) fn list_dir(&mut self, path: &str) -> Result<Vec<(String, bool, u64)>, FsError> {
        self.admitted()?;
        let inode = self.lookup_inner(path)?;
        if !inode.is_dir() { return Err(FsError::NotADirectory); }
        // This is the existing API's bounded output allocation, not traversal state.
        let mut result = Vec::with_capacity(inode.extent_count as usize);
        for index in 0..inode.extent_count as usize {
            let entry = Self::entry(&inode, index)?;
            let child = self.state.inodes[entry.inode_num as usize - 1].ok_or(FsError::Corrupted)?;
            result.push((entry.name_as_str().to_string(), child.is_dir(), child.size));
        }
        Ok(result)
    }
    pub(crate) fn remove(&mut self, path: &str, directory: bool) -> Result<(), FsError> {
        self.mutate(|fs| {
            let (parent_path, name) = Self::split(path)?;
            let child = fs.lookup_inner(path)?;
            if directory && !child.is_dir() { return Err(FsError::NotADirectory); }
            if !directory && child.is_dir() { return Err(FsError::IsADirectory); }
            if directory && child.extent_count != 0 { return Err(FsError::AlreadyExists); }
            let mut parent = fs.lookup_inner(parent_path)?;
            if !parent.is_dir() { return Err(FsError::NotADirectory); }
            if Self::remove_entry(&mut parent, name)? != child.inode_num { return Err(FsError::Corrupted); }
            fs.reserve_table(parent.inode_num)?;
            fs.reserve_table(child.inode_num)?;
            fs.put(parent);
            fs.state.inodes[child.inode_num as usize - 1] = None;
            Ok(())
        })
    }
    pub(crate) fn rename(&mut self, old: &str, new: &str) -> Result<(), FsError> {
        self.mutate(|fs| {
            let (old_parent, old_name) = Self::split(old)?;
            let (new_parent, new_name) = Self::split(new)?;
            let child = fs.lookup_inner(old)?;
            if child.is_dir() { return Err(FsError::IsADirectory); }
            if old.trim_matches('/') == new.trim_matches('/') { return Ok(()); }
            let mut source = fs.lookup_inner(old_parent)?;
            let mut destination = fs.lookup_inner(new_parent)?;
            if !source.is_dir() || !destination.is_dir() { return Err(FsError::NotADirectory); }
            if Self::find(&destination, new_name)?.is_some() { return Err(FsError::AlreadyExists); }
            let entry = DirEntryRecord::new(child.inode_num, 1, new_name).ok_or(FsError::InvalidName)?;
            Self::remove_entry(&mut source, old_name)?;
            if source.inode_num == destination.inode_num {
                Self::insert(&mut source, entry)?;
            } else { Self::insert(&mut destination, entry)?; }
            fs.reserve_table(source.inode_num)?;
            fs.reserve_table(destination.inode_num)?;
            fs.put(source);
            if source.inode_num != destination.inode_num { fs.put(destination); }
            Ok(())
        })
    }

    fn write_bitmap(&mut self) -> Result<(), FsError> {
        self.state.buffer.fill(0xff);
        for block in 0..self.state.active.total_blocks {
            if !self.protected(block) && !has(&self.state.reservations, block) {
                self.state.buffer[block as usize / 8] &= !(1 << (block % 8));
            }
        }
        self.disk.write_block(FIRST_USABLE_BLOCK, &self.state.buffer)
    }
    pub(crate) fn sync(&mut self) -> Result<(), FsError> {
        self.mutate(|fs| fs.sync_inner())
    }
    fn sync_inner(&mut self) -> Result<(), FsError> {
        let mut candidate = self.state.active;
        candidate.sequence = candidate.sequence.checked_add(1).ok_or(FsError::NoSpace)?;
        for slot in 0..32 {
            if self.state.dirty & (1 << slot) != 0 { candidate.inode_blocks[slot] = self.state.tables[slot]; }
        }
        candidate.root_inode_block = candidate.inode_blocks[0];
        self.validate_graph(&candidate, false)?;
        for slot in 0..32 {
            if self.state.dirty & (1 << slot) == 0 { continue; }
            let block = self.state.tables[slot];
            if !has(&self.state.reservations, block) || self.protected(block) {
                return Err(FsError::Corrupted);
            }
            self.state.buffer.fill(0);
            for index in 0..INODES_PER_BLOCK {
                if let Some(record) = self.state.inodes[slot * INODES_PER_BLOCK + index] {
                    let off = index * INODE_SIZE;
                    self.state.buffer[off..off + INODE_SIZE].copy_from_slice(&record.to_bytes());
                }
            }
            self.disk.write_block(block, &self.state.buffer)?;
        }
        self.write_bitmap()?;
        self.disk.flush()?; // FS1: all candidate data and table blocks are durable.
        // Encode the allocation state AFTER durable displacement, not the
        // larger pre-publication union. Superseded unpublished candidates are
        // omitted here, but remain reserved in memory until observed FS2.
        candidate.free_block_count = (RESERVED_END..candidate.total_blocks)
            .filter(|&block| !has(&self.state.reachable[self.state.slot], block)
                && !has(&self.state.scratch, block)).count() as u64;
        candidate.update_checksum();
        let target = 1 - self.state.slot;
        self.disk.write_block(target as u64, &candidate.to_bytes())?;
        self.disk.flush()?; // FS2: only observed success admits acknowledgement/reuse.
        self.state.roots[target] = Some(candidate);
        self.state.reachable[target].copy_from_slice(&self.state.scratch);
        self.state.reservations.fill(0);
        self.state.tables.fill(0);
        self.state.dirty = 0;
        self.state.cursor = RESERVED_END;
        self.state.active = candidate;
        self.state.slot = target;
        self.state.free_count = candidate.free_block_count;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::MemDisk;

    #[test]
    fn compiled_fixed_storage_profile() {
        use core::mem::size_of;
        std::println!("g2-cow fixed storage: allocated_storage={} Cow_handle={} total_fixed={} device_handle={} roots={} active={} reachability={} reservations={} scratch={} roles={} inodes={} seen={} queue={} tables={} io_buffer={} wrapper={}",
            size_of::<Storage>(), size_of::<Cow<MemDisk>>(),
            size_of::<Storage>() + size_of::<Cow<MemDisk>>(), size_of::<MemDisk>(),
            size_of::<[Option<Superblock>; 2]>(), size_of::<Superblock>(),
            size_of::<[Bits; 2]>(), size_of::<Bits>(), size_of::<Bits>(),
            size_of::<[u32; MAX_BLOCKS]>(), size_of::<[Option<InodeRecord>; MAX_INODES]>(),
            size_of::<[bool; MAX_INODES]>(), size_of::<[u16; MAX_INODES]>(),
            size_of::<[u64; 32]>(), size_of::<[u8; BLOCK_SIZE]>(),
            size_of::<crate::engine::CellosFs<MemDisk>>());
        std::println!("g2-cow stack evidence method: compiler-emitted LLVM .stack_sizes (RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Z emit-stack-sizes -C force-frame-pointers=yes' cargo test -p cellos-fs --features g2-cow-experiment --no-run); retain object/llvm-readobj --stack-sizes output and sum the nonrecursive call chains including BlockDevice frames. Include bounded Superblock values and encoding buffers, and report host device/test frames separately. Storage initializes in-place; traversal queue depth<=256, Rust recursion depth=0; extent planning={} bytes. Fixed storage excludes device backing, bounded list_dir API output, allocator bookkeeping and test diagnostics (no engine lock).", size_of::<[Extent; MAX_EXTENTS]>());
    }

    #[test]
    fn reservations_refuse_prior_unpublished_aliases() {
        let mut fs = Cow::format(MemDisk::new(32), 32).unwrap();
        let first = fs.reserve().unwrap();
        let next = fs.reserve().unwrap();
        assert_ne!(first, next);
        assert!(fs.mutate(|fs| fs.reserve_at(first)).is_err());
        assert!(fs.create("/closed", crate::format::INODE_MODE_FILE).is_err());
        assert!(fs.sync().is_err());
    }
}
