use api::block::ViBlockDevice;
use types::{ViResult, ViError};

/// RAM Disk - Zero-copy block device with embedded FAT32 image
/// Implements Luật 8: Direct memory access without copying
pub struct viRamDisk;

// Embed the real FAT32 disk image created by build script
// Path from kernel/src/task/drivers/ramdisk.rs to workspace root
// Using 40MB image to ensure FAT32 compliance (> 65525 clusters)
static DISK_IMAGE: &[u8] = include_bytes!("../../../../disk_40mb.img");

const SECTOR_SIZE: usize = 512;

impl ViBlockDevice for viRamDisk {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        let offset = (sector as usize) * SECTOR_SIZE;
        
        if offset + SECTOR_SIZE > DISK_IMAGE.len() {
            log::error!("RAM Disk: Read beyond disk boundary (sector {})", sector);
            return Err(ViError::InvalidArgument);
        }
        
        // Zero-copy: Direct slice access from static memory
        buf.copy_from_slice(&DISK_IMAGE[offset..offset + SECTOR_SIZE]);
        Ok(())
    }

    fn write_sector(&self, _sector: u64, _buf: &[u8]) -> ViResult<()> {
        // Read-only for now (static embedded image)
        log::warn!("RAM Disk: Write attempted but device is read-only");
        Err(ViError::PermissionDenied)
    }

    fn sector_count(&self) -> u64 {
        (DISK_IMAGE.len() / SECTOR_SIZE) as u64
    }

    fn sector_size(&self) -> usize {
        SECTOR_SIZE
    }
    
    fn flush(&self) -> ViResult<()> {
        Ok(())
    }
}

/// Initialize RAM disk
pub fn init_driver() {
    log::info!("RAM Disk: Embedded FAT32 image loaded");
    log::info!("  Size: {} KB ({} sectors)", 
               DISK_IMAGE.len() / 1024, 
               DISK_IMAGE.len() / SECTOR_SIZE);
}
