use crate::process::drivers::virtio_blk::BLOCK_DEVICE;
use fatfs::{Read, Write, Seek, SeekFrom};

#[derive(Clone)]
pub struct VirtIoDisk {
    pub position: u64,
}

impl VirtIoDisk {
    pub fn new() -> Self {
        Self { position: 0 }
    }
}

// Implement Read, Write, Seek traits from fatfs crate
impl Read for VirtIoDisk {
    type Error = (); 

    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut blk_guard = BLOCK_DEVICE.lock();
        let blk = blk_guard.as_mut().ok_or(())?;
        
        let start_pos = self.position;
        let end_pos = start_pos + buf.len() as u64;
        
        let start_sector = start_pos / 512;
        let end_sector = (end_pos + 511) / 512;
        
        let mut total_read = 0;
        let mut sector_buf = [0u8; 512];
        
        for sector in start_sector..end_sector {
            blk.read_block(sector as usize, &mut sector_buf).map_err(|_| ())?;
            
            let sector_start = sector * 512;
            let sector_end = sector_start + 512;
            
            let overlap_start = core::cmp::max(start_pos, sector_start);
            let overlap_end = core::cmp::min(end_pos, sector_end);
            let len = (overlap_end - overlap_start) as usize;
            
            let src_off = (overlap_start - sector_start) as usize;
            let dst_off = total_read;
            
            buf[dst_off..dst_off+len].copy_from_slice(&sector_buf[src_off..src_off+len]);
            
            total_read += len;
        }
        
        self.position += total_read as u64;
        Ok(total_read)
    }
}

impl Write for VirtIoDisk {
    type Error = ();

    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let mut blk_guard = BLOCK_DEVICE.lock();
        let blk = blk_guard.as_mut().ok_or(())?;
        
        let start_pos = self.position;
        let end_pos = start_pos + buf.len() as u64;
        
        let start_sector = start_pos / 512;
        let end_sector = (end_pos + 511) / 512;
        
        let mut total_written = 0;
        let mut sector_buf = [0u8; 512];
        
        for sector in start_sector..end_sector {
            let sector_start = sector * 512;
            let sector_end = sector_start + 512;
            
            let overlap_start = core::cmp::max(start_pos, sector_start);
            let overlap_end = core::cmp::min(end_pos, sector_end);
            let len = (overlap_end - overlap_start) as usize;
            
            let full_sector = (len == 512); // Optimized
            
            if !full_sector {
                // Read-Modify-Write: Load existing data
                blk.read_block(sector as usize, &mut sector_buf).map_err(|_| ())?;
            }
            
            let dst_off = (overlap_start - sector_start) as usize;
            let src_off = total_written;
            
            sector_buf[dst_off..dst_off+len].copy_from_slice(&buf[src_off..src_off+len]);
            
            blk.write_block(sector as usize, &sector_buf).map_err(|_| ())?;
            
            total_written += len;
        }
        
        self.position += total_written as u64;
        Ok(total_written)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl Seek for VirtIoDisk {
    type Error = ();

    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let new_pos = match pos {
            SeekFrom::Start(off) => off,
            SeekFrom::Current(off) => (self.position as i64 + off) as u64,
            SeekFrom::End(_) => return Err(()), // Requires knowing disk size
        };
        self.position = new_pos;
        Ok(new_pos)
    }
}
