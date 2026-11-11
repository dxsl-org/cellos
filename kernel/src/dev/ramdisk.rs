use api::block::ViBlockDevice;
use types::{ViError, ViResult};

// Embed the kernel-internal FAT16 image as a read-only static slice.
//
// The image is never written at runtime (VIFS1 serves reads only), so no
// heap copy is needed — reads go straight to the &[u8] embedded in .rodata.
// This avoids a 40+ MB heap allocation that would OOM the kernel at boot.
#[cfg(not(target_arch = "riscv32"))]
static DISK_IMAGE: &[u8] = include_bytes!(concat!(env!("EMBEDDED_OUT_DIR"), "/kernel_fs.img"));
#[cfg(target_arch = "riscv32")]
static DISK_IMAGE: &[u8] = &[];

const SECTOR_SIZE: usize = 512;

pub struct ViRamDisk;

impl ViBlockDevice for ViRamDisk {
    fn read_sector(&self, sector: u64, buf: &mut [u8]) -> ViResult<()> {
        let offset = (sector as usize) * SECTOR_SIZE;
        if offset + SECTOR_SIZE > DISK_IMAGE.len() {
            return Err(ViError::InvalidArgument);
        }
        buf.copy_from_slice(&DISK_IMAGE[offset..offset + SECTOR_SIZE]);
        Ok(())
    }

    fn write_sector(&self, _sector: u64, _buf: &[u8]) -> ViResult<()> {
        // VIFS1 is read-only — writes are never issued by the FAT driver on
        // an image that was already consistent when embedded.
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

/// Log the ramdisk size; no heap allocation needed (reads go to .rodata).
pub fn init_driver() {
    log::info!(
        "RAM Disk: read-only, {} KB ({} sectors)",
        DISK_IMAGE.len() / 1024,
        DISK_IMAGE.len() / SECTOR_SIZE
    );
}
