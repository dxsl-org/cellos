//! Inode table and Directory handling.

use crate::disk::{BlockDevice, FsError};
use crate::format::{
    DirEntryRecord, InodeRecord, BLOCK_SIZE, INODES_PER_BLOCK, INODE_PAYLOAD_SIZE, INODE_SIZE,
};
use alloc::vec::Vec;

/// Read an Inode from disk given its block and index within that block.
pub fn read_inode<D: BlockDevice>(
    disk: &mut D,
    block: u64,
    index: usize,
) -> Result<InodeRecord, FsError> {
    if index >= INODES_PER_BLOCK {
        return Err(FsError::Corrupted);
    }
    let mut block_buf = [0u8; BLOCK_SIZE];
    disk.read_block(block, &mut block_buf)?;

    let offset = index * INODE_SIZE;
    let mut inode_bytes = [0u8; INODE_SIZE];
    inode_bytes.copy_from_slice(&block_buf[offset..offset + INODE_SIZE]);

    InodeRecord::from_bytes(&inode_bytes).ok_or(FsError::Corrupted)
}

/// Write an Inode to disk given its block and index within that block.
pub fn write_inode<D: BlockDevice>(
    disk: &mut D,
    block: u64,
    index: usize,
    record: &mut InodeRecord,
) -> Result<(), FsError> {
    if index >= INODES_PER_BLOCK {
        return Err(FsError::Corrupted);
    }
    let mut block_buf = [0u8; BLOCK_SIZE];
    disk.read_block(block, &mut block_buf)?;

    record.update_checksum();
    let inode_bytes = record.to_bytes();

    let offset = index * INODE_SIZE;
    block_buf[offset..offset + INODE_SIZE].copy_from_slice(&inode_bytes);

    disk.write_block(block, &block_buf)
}

/// Helper for directory payload parsing (inline entries).
pub struct DirPayload;

impl DirPayload {
    pub fn list(payload: &[u8; INODE_PAYLOAD_SIZE], count: usize) -> Vec<DirEntryRecord> {
        let mut list = Vec::new();
        let entry_size = DirEntryRecord::SIZE;
        let max_entries = INODE_PAYLOAD_SIZE / entry_size;
        let limit = count.min(max_entries);

        for i in 0..limit {
            let offset = i * entry_size;
            let mut buf = [0u8; DirEntryRecord::SIZE];
            buf.copy_from_slice(&payload[offset..offset + entry_size]);
            if let Some(entry) = DirEntryRecord::from_bytes(&buf) {
                list.push(entry);
            }
        }
        list
    }

    pub fn find(
        payload: &[u8; INODE_PAYLOAD_SIZE],
        count: usize,
        name: &str,
    ) -> Option<DirEntryRecord> {
        let entries = Self::list(payload, count);
        entries.into_iter().find(|e| e.name_as_str() == name)
    }

    pub fn insert(
        payload: &mut [u8; INODE_PAYLOAD_SIZE],
        count: &mut usize,
        entry: &DirEntryRecord,
    ) -> Result<(), FsError> {
        let entry_size = DirEntryRecord::SIZE;
        let max_entries = INODE_PAYLOAD_SIZE / entry_size;
        if *count >= max_entries {
            return Err(FsError::NoSpace); // Need indirect block if exceeded
        }

        let offset = (*count) * entry_size;
        let bytes = entry.to_bytes();
        payload[offset..offset + entry_size].copy_from_slice(&bytes);
        *count += 1;
        Ok(())
    }

    pub fn remove(
        payload: &mut [u8; INODE_PAYLOAD_SIZE],
        count: &mut usize,
        name: &str,
    ) -> Result<u64, FsError> {
        let entry_size = DirEntryRecord::SIZE;
        let mut entries = Self::list(payload, *count);
        let pos = entries
            .iter()
            .position(|e| e.name_as_str() == name)
            .ok_or(FsError::NotFound)?;

        let removed_inode = entries[pos].inode_num;
        entries.swap_remove(pos);

        // Re-serialize
        *count = entries.len();
        payload.fill(0);
        for (i, entry) in entries.iter().enumerate() {
            let offset = i * entry_size;
            payload[offset..offset + entry_size].copy_from_slice(&entry.to_bytes());
        }

        Ok(removed_inode)
    }
}
