//! CellosFS On-Disk Binary Format Specifications.
//!
//! All on-disk integers are Little-Endian.
//! Standard block size is 4096 bytes (8 x 512-byte sectors).

use crate::crc32::crc32c;

/// Standard block size: 4 KiB.
pub const BLOCK_SIZE: usize = 4096;

/// Magic identifier: b"CELL_OSF" (0x46534F5F4C4C4543 in LE).
pub const MAGIC: [u8; 8] = *b"CELL_OSF";

/// Format Version 1.
pub const FORMAT_VERSION: u32 = 1;

/// Block index for Superblock A.
pub const SUPERBLOCK_A_BLOCK: u64 = 0;

/// Block index for Superblock B.
pub const SUPERBLOCK_B_BLOCK: u64 = 1;

/// First allocatable data/metadata block.
pub const FIRST_USABLE_BLOCK: u64 = 2;

/// Inode size: 512 bytes (8 inodes packed per 4 KiB block).
pub const INODE_SIZE: usize = 512;
pub const INODES_PER_BLOCK: usize = BLOCK_SIZE / INODE_SIZE;

/// Maximum payload size inside an Inode (for inline file data or extents).
pub const INODE_PAYLOAD_SIZE: usize = 428;

/// Inode Modes.
pub const INODE_MODE_UNUSED: u16 = 0;
pub const INODE_MODE_FILE: u16 = 1;
pub const INODE_MODE_DIR: u16 = 2;

/// Inode Flags.
pub const INODE_FLAG_INLINE: u16 = 1 << 0;

/// Maximum length of a filename in a compact DirEntry (56 bytes total).
pub const MAX_NAME_LEN: usize = 46;

/// Superblock structure (4096 bytes, stored at block 0 and block 1).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Superblock {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub sequence: u64,
    pub block_size: u32,
    pub total_blocks: u64,
    pub root_inode_block: u64,
    pub free_bitmap_block: u64,
    pub free_bitmap_blocks: u32,
    pub free_block_count: u64,
    pub next_inode_num: u64,
    pub created_at: u64,
    pub mounted_at: u64,
    pub inode_blocks: [u64; 32],
    pub reserved: [u8; 3748],
    pub checksum: u32,
}

impl Superblock {
    pub fn new(total_blocks: u64, free_bitmap_blocks: u32) -> Self {
        let mut sb = Self {
            magic: MAGIC,
            version: FORMAT_VERSION,
            flags: 0,
            sequence: 1,
            block_size: BLOCK_SIZE as u32,
            total_blocks,
            root_inode_block: FIRST_USABLE_BLOCK + free_bitmap_blocks as u64,
            free_bitmap_block: FIRST_USABLE_BLOCK,
            free_bitmap_blocks,
            free_block_count: total_blocks
                .saturating_sub(FIRST_USABLE_BLOCK + free_bitmap_blocks as u64 + 1),
            next_inode_num: 2, // 1 is root inode
            created_at: 0,
            mounted_at: 0,
            inode_blocks: {
                let mut arr = [0u64; 32];
                arr[0] = FIRST_USABLE_BLOCK + free_bitmap_blocks as u64;
                arr
            },
            reserved: [0u8; 3748],
            checksum: 0,
        };
        sb.update_checksum();
        sb
    }

    pub fn to_bytes(&self) -> [u8; BLOCK_SIZE] {
        let mut buf = [0u8; BLOCK_SIZE];
        buf[0..8].copy_from_slice(&self.magic);
        buf[8..12].copy_from_slice(&self.version.to_le_bytes());
        buf[12..16].copy_from_slice(&self.flags.to_le_bytes());
        buf[16..24].copy_from_slice(&self.sequence.to_le_bytes());
        buf[24..28].copy_from_slice(&self.block_size.to_le_bytes());
        buf[28..36].copy_from_slice(&self.total_blocks.to_le_bytes());
        buf[36..44].copy_from_slice(&self.root_inode_block.to_le_bytes());
        buf[44..52].copy_from_slice(&self.free_bitmap_block.to_le_bytes());
        buf[52..56].copy_from_slice(&self.free_bitmap_blocks.to_le_bytes());
        buf[56..64].copy_from_slice(&self.free_block_count.to_le_bytes());
        buf[64..72].copy_from_slice(&self.next_inode_num.to_le_bytes());
        buf[72..80].copy_from_slice(&self.created_at.to_le_bytes());
        buf[80..88].copy_from_slice(&self.mounted_at.to_le_bytes());
        for (i, &blk) in self.inode_blocks.iter().enumerate() {
            buf[88 + i * 8..88 + (i + 1) * 8].copy_from_slice(&blk.to_le_bytes());
        }
        buf[344..4092].copy_from_slice(&self.reserved);
        buf[4092..4096].copy_from_slice(&self.checksum.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; BLOCK_SIZE]) -> Option<Self> {
        let magic: [u8; 8] = buf[0..8].try_into().ok()?;
        if magic != MAGIC {
            return None;
        }
        let stored_checksum = u32::from_le_bytes(buf[4092..4096].try_into().ok()?);
        let computed_checksum = crc32c(&buf[0..4092]);
        if stored_checksum != computed_checksum {
            return None;
        }

        let mut inode_blocks = [0u64; 32];
        for (i, chunk) in buf[88..344].chunks_exact(8).enumerate() {
            inode_blocks[i] = u64::from_le_bytes(chunk.try_into().ok()?);
        }
        let mut reserved = [0u8; 3748];
        reserved.copy_from_slice(&buf[344..4092]);

        Some(Self {
            magic,
            version: u32::from_le_bytes(buf[8..12].try_into().ok()?),
            flags: u32::from_le_bytes(buf[12..16].try_into().ok()?),
            sequence: u64::from_le_bytes(buf[16..24].try_into().ok()?),
            block_size: u32::from_le_bytes(buf[24..28].try_into().ok()?),
            total_blocks: u64::from_le_bytes(buf[28..36].try_into().ok()?),
            root_inode_block: u64::from_le_bytes(buf[36..44].try_into().ok()?),
            free_bitmap_block: u64::from_le_bytes(buf[44..52].try_into().ok()?),
            free_bitmap_blocks: u32::from_le_bytes(buf[52..56].try_into().ok()?),
            free_block_count: u64::from_le_bytes(buf[56..64].try_into().ok()?),
            next_inode_num: u64::from_le_bytes(buf[64..72].try_into().ok()?),
            created_at: u64::from_le_bytes(buf[72..80].try_into().ok()?),
            mounted_at: u64::from_le_bytes(buf[80..88].try_into().ok()?),
            inode_blocks,
            reserved,
            checksum: stored_checksum,
        })
    }

    pub fn update_checksum(&mut self) {
        let bytes = self.to_bytes();
        self.checksum = crc32c(&bytes[0..4092]);
    }
}

/// Extent mapping contiguous logical file blocks to contiguous physical disk blocks.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Extent {
    pub logical_block: u32,
    pub block_count: u32,
    pub physical_block: u64,
}

impl Extent {
    pub const SIZE: usize = 16;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.logical_block.to_le_bytes());
        b[4..8].copy_from_slice(&self.block_count.to_le_bytes());
        b[8..16].copy_from_slice(&self.physical_block.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8; Self::SIZE]) -> Self {
        Self {
            logical_block: u32::from_le_bytes(b[0..4].try_into().unwrap()),
            block_count: u32::from_le_bytes(b[4..8].try_into().unwrap()),
            physical_block: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        }
    }
}

/// 512-byte packed Inode structure.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InodeRecord {
    pub inode_num: u64,
    pub mode: u16,
    pub flags: u16,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub extent_count: u32,
    pub inline_size: u32,
    pub payload: [u8; INODE_PAYLOAD_SIZE],
    pub checksum: u32,
}

impl InodeRecord {
    pub fn new(inode_num: u64, mode: u16) -> Self {
        let mut record = Self {
            inode_num,
            mode,
            flags: INODE_FLAG_INLINE,
            nlink: 1,
            uid: 0,
            gid: 0,
            size: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            extent_count: 0,
            inline_size: 0,
            payload: [0u8; INODE_PAYLOAD_SIZE],
            checksum: 0,
        };
        record.update_checksum();
        record
    }

    pub fn to_bytes(&self) -> [u8; INODE_SIZE] {
        let mut buf = [0u8; INODE_SIZE];
        buf[0..8].copy_from_slice(&self.inode_num.to_le_bytes());
        buf[8..10].copy_from_slice(&self.mode.to_le_bytes());
        buf[10..12].copy_from_slice(&self.flags.to_le_bytes());
        buf[12..16].copy_from_slice(&self.nlink.to_le_bytes());
        buf[16..20].copy_from_slice(&self.uid.to_le_bytes());
        buf[20..24].copy_from_slice(&self.gid.to_le_bytes());
        buf[24..32].copy_from_slice(&self.size.to_le_bytes());
        buf[32..40].copy_from_slice(&self.atime.to_le_bytes());
        buf[40..48].copy_from_slice(&self.mtime.to_le_bytes());
        buf[48..56].copy_from_slice(&self.ctime.to_le_bytes());
        buf[56..60].copy_from_slice(&self.extent_count.to_le_bytes());
        buf[60..64].copy_from_slice(&self.inline_size.to_le_bytes());
        buf[64..64 + INODE_PAYLOAD_SIZE].copy_from_slice(&self.payload);
        buf[508..512].copy_from_slice(&self.checksum.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; INODE_SIZE]) -> Option<Self> {
        let stored_checksum = u32::from_le_bytes(buf[508..512].try_into().ok()?);
        let computed_checksum = crc32c(&buf[0..508]);
        if stored_checksum != computed_checksum {
            return None;
        }

        let mut payload = [0u8; INODE_PAYLOAD_SIZE];
        payload.copy_from_slice(&buf[64..64 + INODE_PAYLOAD_SIZE]);

        Some(Self {
            inode_num: u64::from_le_bytes(buf[0..8].try_into().ok()?),
            mode: u16::from_le_bytes(buf[8..10].try_into().ok()?),
            flags: u16::from_le_bytes(buf[10..12].try_into().ok()?),
            nlink: u32::from_le_bytes(buf[12..16].try_into().ok()?),
            uid: u32::from_le_bytes(buf[16..20].try_into().ok()?),
            gid: u32::from_le_bytes(buf[20..24].try_into().ok()?),
            size: u64::from_le_bytes(buf[24..32].try_into().ok()?),
            atime: u64::from_le_bytes(buf[32..40].try_into().ok()?),
            mtime: u64::from_le_bytes(buf[40..48].try_into().ok()?),
            ctime: u64::from_le_bytes(buf[48..56].try_into().ok()?),
            extent_count: u32::from_le_bytes(buf[56..60].try_into().ok()?),
            inline_size: u32::from_le_bytes(buf[60..64].try_into().ok()?),
            payload,
            checksum: stored_checksum,
        })
    }

    pub fn update_checksum(&mut self) {
        let bytes = self.to_bytes();
        self.checksum = crc32c(&bytes[0..508]);
    }

    pub fn is_inline(&self) -> bool {
        self.flags & INODE_FLAG_INLINE != 0
    }

    pub fn is_file(&self) -> bool {
        self.mode == INODE_MODE_FILE
    }

    pub fn is_dir(&self) -> bool {
        self.mode == INODE_MODE_DIR
    }
}

/// Directory entry for naming hierarchy.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DirEntryRecord {
    pub inode_num: u64,
    pub file_type: u8,
    pub name_len: u8,
    pub name: [u8; MAX_NAME_LEN],
}

impl DirEntryRecord {
    pub const SIZE: usize = 8 + 1 + 1 + MAX_NAME_LEN; // 258 bytes

    pub fn new(inode_num: u64, file_type: u8, name_str: &str) -> Option<Self> {
        let bytes = name_str.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_NAME_LEN {
            return None;
        }
        let mut name = [0u8; MAX_NAME_LEN];
        name[..bytes.len()].copy_from_slice(bytes);
        Some(Self {
            inode_num,
            file_type,
            name_len: bytes.len() as u8,
            name,
        })
    }

    pub fn name_as_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("")
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..8].copy_from_slice(&self.inode_num.to_le_bytes());
        b[8] = self.file_type;
        b[9] = self.name_len;
        b[10..10 + MAX_NAME_LEN].copy_from_slice(&self.name);
        b
    }

    pub fn from_bytes(b: &[u8; Self::SIZE]) -> Option<Self> {
        let name_len = b[9] as usize;
        if name_len > MAX_NAME_LEN {
            return None;
        }
        let mut name = [0u8; MAX_NAME_LEN];
        name.copy_from_slice(&b[10..10 + MAX_NAME_LEN]);
        Some(Self {
            inode_num: u64::from_le_bytes(b[0..8].try_into().ok()?),
            file_type: b[8],
            name_len: name_len as u8,
            name,
        })
    }
}
