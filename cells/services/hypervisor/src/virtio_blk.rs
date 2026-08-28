//! virtio-blk device model (DeviceID=2, virtio-mmio slot 1 → SPI 17).
//!
//! 16 MiB in-memory backing; reads and writes are volatile (not persisted).
//!
//! Chain layout (virtio-blk spec §5.2.6.1):
//!   [0]      outhdr   16B device-readable: { type:u32, _:u32, sector:u64 }
//!   [1..n-1] data     device-readable (OUT) or device-writable (IN)
//!   [last]   status   1B  device-writable: 0=OK 1=IOERR 2=UNSUPP

extern crate alloc;
use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::{process_notify, DescBuf};
use alloc::vec;

// Guest scratch volume backed by the cell heap. MUST stay well under the
// fixed 8 MiB cell heap (ostd) — 16 MiB here OOM-killed the cell the moment
// the run loop constructed BlkDisk. Alpine netboot runs from initramfs and
// only needs this for ad-hoc writes.
const DISK_SIZE: usize = 4 * 1024 * 1024; // 4 MiB
const SECTOR_SIZE: usize = 512;
const NUM_SECTORS: u64 = (DISK_SIZE / SECTOR_SIZE) as u64;

const BLK_T_IN: u32 = 0; // read  — device → driver
const BLK_T_OUT: u32 = 1; // write — driver → device
const BLK_T_FLUSH: u32 = 4;

const DISK_SPI: u32 = 17; // SPI line for virtio-mmio slot 1

pub enum Backend {
    Volatile(alloc::vec::Vec<u8>),
    Persistent {
        file: ostd::fs::File,
        size: u64,
    },
}

pub struct BlkDisk {
    backend: Backend,
    num_sectors: u64,
    last_avail: u16,
    used_idx: u16,
}

impl BlkDisk {
    pub fn new(file: Option<ostd::fs::File>) -> Self {
        let (backend, num_sectors) = match file {
            Some(f) => {
                let size = f.size().unwrap_or(0);
                (Backend::Persistent { file: f, size }, size / (SECTOR_SIZE as u64))
            }
            None => {
                (Backend::Volatile(vec![0u8; DISK_SIZE]), NUM_SECTORS)
            }
        };
        Self {
            backend,
            num_sectors,
            last_avail: 0,
            used_idx: 0,
        }
    }
}

impl VirtioDevice for BlkDisk {
    fn device_id(&self) -> u32 {
        2
    }

    /// virtio-blk config: capacity at bytes 0-7 (little-endian u64 of sectors).
    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            0 => (self.num_sectors & 0xFFFF_FFFF) as u32,
            4 => (self.num_sectors >> 32) as u32,
            _ => 0,
        }
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) {
        if q != 0 {
            return;
        }
        // Disjoint field borrows: backend / last_avail / used_idx
        let backend = &mut self.backend;
        process_notify(
            vm_id,
            qcfg,
            &mut self.last_avail,
            &mut self.used_idx,
            |bufs| handle_blk_request(backend, bufs, vm_id),
        );
        crate::vmm::inject_irq(vm_id, vcpu_id, DISK_SPI);
    }
}

fn handle_blk_request(backend: &mut Backend, bufs: &[DescBuf], vm_id: usize) -> u32 {
    if bufs.len() < 3 {
        return 0;
    }
    let status_idx = bufs.len() - 1;

    let mut hdr = [0u8; 16];
    if crate::vmm::read_guest_memory(vm_id, bufs[0].gpa, &mut hdr) != 16 {
        write_status(vm_id, bufs[status_idx].gpa, 1);
        return 1;
    }
    let req_type = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let sector = u64::from_le_bytes(hdr[8..16].try_into().unwrap_or([0u8; 8]));

    let data_bufs = &bufs[1..status_idx];
    let status = match req_type {
        BLK_T_IN => blk_read(backend, sector, data_bufs, vm_id),
        BLK_T_OUT => blk_write(backend, sector, data_bufs, vm_id),
        BLK_T_FLUSH => blk_flush(backend),
        _ => 2u8, // VIRTIO_BLK_S_UNSUPP
    };
    write_status(vm_id, bufs[status_idx].gpa, status);
    1 // bytes placed in used ring (status byte)
}
fn blk_flush(backend: &mut Backend) -> u8 {
    match backend {
        Backend::Volatile(_) => 0,
        Backend::Persistent { file, .. } => {
            if file.sync_all().is_ok() {
                0
            } else {
                1 // VIRTIO_BLK_S_IOERR
            }
        }
    }
}

/// READ: copy disk sectors into driver-writable guest buffers.
fn blk_read(backend: &mut Backend, sector: u64, bufs: &[DescBuf], vm_id: usize) -> u8 {
    let mut lba = sector;
    for buf in bufs {
        let off = lba.saturating_mul(SECTOR_SIZE as u64);
        match backend {
            Backend::Volatile(disk) => {
                let off = off as usize;
                if off >= disk.len() { break; }
                let n = (buf.len as usize).min(disk.len() - off);
                crate::vmm::write_guest_memory(vm_id, buf.gpa, &disk[off..off + n]);
                lba += (n.div_ceil(SECTOR_SIZE)) as u64;
            }
            Backend::Persistent { file, size } => {
                if off >= *size { break; }
                let n = (buf.len as u64).min(*size - off) as usize;
                let mut tmp = vec![0u8; n];
                if file.read_at(off, &mut tmp).unwrap_or(0) != n {
                    return 1;
                }
                crate::vmm::write_guest_memory(vm_id, buf.gpa, &tmp);
                lba += (n.div_ceil(SECTOR_SIZE)) as u64;
            }
        }
    }
    0
}

/// WRITE: copy driver-readable guest buffers into disk sectors.
fn blk_write(backend: &mut Backend, sector: u64, bufs: &[DescBuf], vm_id: usize) -> u8 {
    let mut lba = sector;
    for buf in bufs {
        let off = lba.saturating_mul(SECTOR_SIZE as u64);
        match backend {
            Backend::Volatile(disk) => {
                let off = off as usize;
                if off >= disk.len() { break; }
                let n = (buf.len as usize).min(disk.len() - off);
                let mut tmp = vec![0u8; n];
                let got = crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut tmp);
                if got == 0 || got == usize::MAX { break; }
                disk[off..off + got].copy_from_slice(&tmp[..got]);
                lba += (got.div_ceil(SECTOR_SIZE)) as u64;
            }
            Backend::Persistent { file, size } => {
                if off >= *size { break; }
                let n = (buf.len as u64).min(*size - off) as usize;
                let mut tmp = vec![0u8; n];
                let got = crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut tmp);
                if got == 0 || got == usize::MAX { break; }
                if file.write_at(off, &tmp[..got]).is_err() {
                    return 1;
                }
                lba += (got.div_ceil(SECTOR_SIZE)) as u64;
            }
        }
    }
    0
}

fn write_status(vm_id: usize, gpa: u64, status: u8) {
    crate::vmm::write_guest_memory(vm_id, gpa, &[status]);
}
