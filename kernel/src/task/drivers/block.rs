use api::block::ViBlockDevice;
use super::mmc::MmcBlock;
use types::{ViError, ViResult};

static MMC_ZST:  MmcBlock  = MmcBlock;
static NULL_ZST: NullBlock = NullBlock;

/// Placeholder block device used when no kernel-resident block driver is present.
///
/// Since the G2 loader redesign (phases 05/06) the kernel drives NO block hardware
/// on QEMU — VFS routes all sector I/O to the virtio-blk Driver Cell (`/bin/block`),
/// and NVMe/e1000 are Driver Cells too. The residual kernel callers of `block::` —
/// warm-boot snapshot save/restore, `verify_mbr`, and the `EarlyLoader` bootstrap
/// fallback — all handle a read error gracefully, so this device just fails reads.
/// MMC (SDHCI) still wins on real boards.
struct NullBlock;

impl ViBlockDevice for NullBlock {
    fn read_sector(&self, _sector: u64, _buf: &mut [u8]) -> ViResult<()> {
        Err(ViError::NotFound)
    }
    fn write_sector(&self, _sector: u64, _buf: &[u8]) -> ViResult<()> {
        Err(ViError::NotFound)
    }
    fn sector_count(&self) -> u64 {
        0
    }
    fn sector_size(&self) -> usize {
        512
    }
    fn flush(&self) -> ViResult<()> {
        Ok(())
    }
}

/// Return the active kernel block device.
///
/// MMC (SDHCI) on real boards; otherwise a null device — on QEMU the virtio-blk
/// Driver Cell owns the disk, not the kernel (G2 loader redesign). VFS reaches the
/// block driver via `sys_lookup_service(service::BLOCK_DRIVER)`, not this path.
pub fn block_device() -> &'static dyn ViBlockDevice {
    if super::mmc::is_present() {
        &MMC_ZST
    } else {
        &NULL_ZST
    }
}

/// Read one 512-byte sector. Convenience wrapper — no `ViBlockDevice` import required.
pub fn read_sector(sector: u64, buf: &mut [u8]) -> ViResult<()> {
    block_device().read_sector(sector, buf)
}

/// Write one 512-byte sector. Convenience wrapper — no `ViBlockDevice` import required.
pub fn write_sector(sector: u64, buf: &[u8]) -> ViResult<()> {
    block_device().write_sector(sector, buf)
}

/// Flush pending writes. Convenience wrapper — no `ViBlockDevice` import required.
pub fn flush() -> ViResult<()> {
    block_device().flush()
}
