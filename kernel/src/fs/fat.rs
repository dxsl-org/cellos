//! FAT32 Filesystem Implementation
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use core::cmp;
use core::cell::RefCell;

use api::block::ViBlockDevice;
use api::fs::{ViFileSystem, ViFile, OpenMode, FileHandle};
use types::{ViResult, ViError};
use crate::task::drivers::ramdisk::viRamDisk;  // Use RAM disk instead of VirtIO
use crate::sync::Spinlock; // Using Spinlock for kernel level sync

// Import io traits from fatfs (0.4)
use fatfs::{Read, Write, Seek, SeekFrom, IoBase};

/// Wrapper around the Block Device to provide a Read/Write/Seek stream for fatfs
pub struct BlockStream {
    device: viRamDisk,
    pos: u64,
}

impl BlockStream {
    pub fn new() -> Self {
        Self {
            device: viRamDisk,
            pos: 0,
        }
    }
}

// Implement fatfs IO traits
// fatfs 0.4 Read/Write/Seek traits inherit from IoBase

impl IoBase for BlockStream {
    type Error = (); // Use unit type for now to satisfy IoBase
}

impl Read for BlockStream {
    // type Error is in IoBase

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.len() == 0 {
            return Ok(0);
        }

        let sector_size = self.device.sector_size() as u64;
        let start_sector = self.pos / sector_size;
        let offset = (self.pos % sector_size) as usize;
        
        // Disable log to avoid recursion if necessary, but we need it now.
         // log::info!("BlockStream::read: Pos{}, Sec{}, Off{}, Len{}", self.pos, start_sector, offset, buf.len());

        let mut sector_buf = [0u8; 512]; 
        if self.device.read_sector(start_sector, &mut sector_buf).is_err() {
             log::error!("BlockStream: Read Error at Sector {}", start_sector);
             return Err(());
        }

        let available = 512 - offset;
        let to_copy = cmp::min(available, buf.len());
        
        buf[0..to_copy].copy_from_slice(&sector_buf[offset..offset+to_copy]);
        
        self.pos += to_copy as u64;
        Ok(to_copy)
    }
}

impl Write for BlockStream {
    fn write(&mut self, _buf: &[u8]) -> Result<usize, Self::Error> {
        Err(()) // Fail writes
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Seek for BlockStream {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let new_pos = match pos {
            SeekFrom::Start(off) => off,
            SeekFrom::Current(off) => (self.pos as i64 + off) as u64,
            SeekFrom::End(_) => return Err(()),
        };
        self.pos = new_pos;
        Ok(new_pos)
    }
}

// Type alias for the FS
type FatFS = fatfs::FileSystem<BlockStream, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;

pub struct ViFatFS {
    // Shared access to the filesystem via Spinlock to ensure Sync
    inner: Arc<Spinlock<FatFS>>,
}

unsafe impl Send for ViFatFS {}
unsafe impl Sync for ViFatFS {}

impl ViFatFS {
    pub fn new() -> ViResult<Self> {
        let mut stream = BlockStream::new();
        
        // Debug: Read and log boot sector
        {
            let mut boot_sector = [0u8; 512];
            if stream.read(&mut boot_sector).is_ok() {
                log::info!("Boot Sector Debug:");
                log::info!("  Signature: 0x{:02X}{:02X}", boot_sector[511], boot_sector[510]);
                log::info!("  Bytes/Sector: {}", u16::from_le_bytes([boot_sector[11], boot_sector[12]]));
                log::info!("  Sectors/Cluster: {}", boot_sector[13]);
                log::info!("  Reserved Sectors: {}", u16::from_le_bytes([boot_sector[14], boot_sector[15]]));
                log::info!("  FAT Count: {}", boot_sector[16]);
                log::info!("  Media: 0x{:02X}", boot_sector[21]);
                log::info!("  Total Sectors: {}", u32::from_le_bytes([boot_sector[32], boot_sector[33], boot_sector[34], boot_sector[35]]));
                log::info!("  Sectors/FAT: {}", u32::from_le_bytes([boot_sector[36], boot_sector[37], boot_sector[38], boot_sector[39]]));
                log::info!("  Root Cluster: {}", u32::from_le_bytes([boot_sector[44], boot_sector[45], boot_sector[46], boot_sector[47]]));
            }
            // Reset stream position
            stream.pos = 0;
        }
        
        let options = fatfs::FsOptions::new().update_accessed_date(false);
        match fatfs::FileSystem::new(stream, options) {
            Ok(fs) => Ok(Self { 
                inner: Arc::new(Spinlock::new(fs)) 
            }),
            Err(e) => {
                log::error!("ViFatFS: Mount failed: {:?}", e);
                Err(ViError::InvalidArgument)
            }
        }
    }
}

impl ViFileSystem for ViFatFS {
    fn open(&self, path: &str, _mode: OpenMode) -> ViResult<Box<dyn ViFile + Send + Sync>> {
        // Trim leading slash for fatfs
        let rel_path = path.trim_start_matches('/');
        let fs_lock = self.inner.lock();
        let root = fs_lock.root_dir();
        
        let mut is_dir = false;

        // Try opening as file first
        if root.open_file(rel_path).is_err() {
            // Try opening as directory
            if root.open_dir(rel_path).is_ok() {
                is_dir = true;
            } else {
                 return Err(ViError::NotFound);
            }
        }
        
        // Return a stateless handle
        Ok(Box::new(FatFile { 
            path: String::from(path), 
            pos: 0,
            fs: self.inner.clone(),
            is_dir,
        }))
    }
    
    fn mkdir(&self, _path: &str) -> ViResult<()> {
        Err(ViError::NotSupported)
    }
    
    fn remove(&self, _path: &str) -> ViResult<()> {
        Err(ViError::NotSupported)
    }
}

/// Stateless File Handle
pub struct FatFile {
    path: String,
    pos: u64,
    fs: Arc<Spinlock<FatFS>>,
    is_dir: bool,
}

impl ViFile for FatFile {
    fn read(&mut self, buf: &mut [u8]) -> ViResult<usize> {
        if self.is_dir { return Err(ViError::IsADirectory); }

        let n = {
            let fs_lock = self.fs.lock();
            let root = fs_lock.root_dir();
             // Important: Strip leading slash, same as open()
            let rel_path = self.path.trim_start_matches('/');
            let res = match root.open_file(rel_path) {
                Ok(mut file) => {
                    file.seek(SeekFrom::Start(self.pos)).map_err(|_| ViError::IO)?;
                    file.read(buf).map_err(|_| ViError::IO)?
                },
                Err(_) => return Err(ViError::NotFound),
            };
            res
        };
        self.pos += n as u64;
        Ok(n)
    }
    
    fn write(&mut self, _buf: &[u8]) -> ViResult<usize> {
        Err(ViError::NotSupported)
    }
    
    fn seek(&mut self, pos: api::fs::SeekFrom) -> ViResult<u64> {
         let new_pos = match pos {
            api::fs::SeekFrom::Start(off) => off,
            api::fs::SeekFrom::Current(off) => (self.pos as i64 + off) as u64,
            api::fs::SeekFrom::End(off) => {
                // We need file size to seek from end
                let fs_lock = self.fs.lock();
                let root = fs_lock.root_dir();
                let rel_path = self.path.trim_start_matches('/');
                let mut f = root.open_file(rel_path).map_err(|_| ViError::NotFound)?;
                let size = f.seek(SeekFrom::End(0)).map_err(|_| ViError::IO)?;
                (size as i64 + off) as u64
            }, 
        };
        self.pos = new_pos;
        Ok(new_pos)
    }

    fn is_dir(&self) -> bool { self.is_dir }

    fn read_dir(&mut self) -> ViResult<Option<types::DirEntry>> {
        if !self.is_dir { return Err(ViError::NotADirectory); }

        let fs_lock = self.fs.lock();
        let root = fs_lock.root_dir();
        // Open directory relative to root (assuming path is absolute/full)
        // fatfs doesn't support absolute paths directly if they start with /, usually relative to dir.
        // We trim leading / if present.
        let p = self.path.trim_start_matches('/');
        let dir = if p.is_empty() {
            root
        } else {
            root.open_dir(p).map_err(|_| ViError::NotFound)?
        };

        // Skip 'pos' entries
        let mut skipped = 0;
        let iter = dir.iter();
        for entry_res in iter {
             if skipped < self.pos {
                 skipped += 1;
                 continue;
             }
             // Found our entry
             let entry = entry_res.map_err(|_| ViError::IO)?;
             let mut name = [0u8; 64];
             let name_str = entry.file_name();
             let name_bytes = name_str.as_bytes();
             let copy_len = core::cmp::min(name.len(), name_bytes.len());
             name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
             
             let file_type = if entry.is_dir() { types::FileType::Directory } else { types::FileType::File };
             
             self.pos += 1;
             return Ok(Some(types::DirEntry {
                 name,
                 file_type,
                 size: entry.len(),
             }));
        }
        
        Ok(None) // EOF
    }
}
