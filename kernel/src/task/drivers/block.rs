use api::block::ViBlockDevice;
use super::mmc::MmcBlock;
use super::virtio_blk::viVirtIOBlk;
use types::ViResult;

static VIRTIO_ZST: viVirtIOBlk = viVirtIOBlk;
static MMC_ZST:    MmcBlock    = MmcBlock;

/// Return the active kernel block device.
///
/// Priority: VirtIO (QEMU para-virtual) → MMC (real board).
///
/// Note: PCIe NVMe is now a Driver Cell (`cells/drivers/nvme/`), not a
/// kernel driver. VFS acquires it via `sys_lookup_service(service::BLOCK_DRIVER)`.
pub fn block_device() -> &'static dyn ViBlockDevice {
    if super::virtio_blk::is_present() {
        &VIRTIO_ZST
    } else if super::mmc::is_present() {
        &MMC_ZST
    } else {
        &VIRTIO_ZST
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
