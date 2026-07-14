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

const DISK_SIZE: usize = 16 * 1024 * 1024; // 16 MiB
const SECTOR_SIZE: usize = 512;
const NUM_SECTORS: u64 = (DISK_SIZE / SECTOR_SIZE) as u64;

const BLK_T_IN: u32 = 0; // read  — device → driver
const BLK_T_OUT: u32 = 1; // write — driver → device
const BLK_T_FLUSH: u32 = 4;

const DISK_SPI: u32 = 17; // SPI line for virtio-mmio slot 1

pub struct BlkDisk {
    data: alloc::vec::Vec<u8>,
    last_avail: u16,
    used_idx: u16,
}

impl BlkDisk {
    pub fn new() -> Self {
        Self {
            data: vec![0u8; DISK_SIZE],
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
            0 => (NUM_SECTORS & 0xFFFF_FFFF) as u32,
            4 => (NUM_SECTORS >> 32) as u32,
            _ => 0,
        }
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) {
        if q != 0 {
            return;
        }
        // Disjoint field borrows: data / last_avail / used_idx
        let disk = self.data.as_mut_slice();
        process_notify(
            vm_id,
            qcfg,
            &mut self.last_avail,
            &mut self.used_idx,
            |bufs| handle_blk_request(disk, bufs, vm_id),
        );
        crate::vmm::inject_irq(vm_id, vcpu_id, DISK_SPI);
    }
}

fn handle_blk_request(disk: &mut [u8], bufs: &[DescBuf], vm_id: usize) -> u32 {
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
        BLK_T_IN => blk_read(disk, sector, data_bufs, vm_id),
        BLK_T_OUT => blk_write(disk, sector, data_bufs, vm_id),
        BLK_T_FLUSH => 0u8,
        _ => 2u8, // VIRTIO_BLK_S_UNSUPP
    };
    write_status(vm_id, bufs[status_idx].gpa, status);
    1 // bytes placed in used ring (status byte)
}

/// READ: copy disk sectors into driver-writable guest buffers.
fn blk_read(disk: &[u8], sector: u64, bufs: &[DescBuf], vm_id: usize) -> u8 {
    let mut lba = sector;
    for buf in bufs {
        let off = (lba as usize).saturating_mul(SECTOR_SIZE);
        if off >= disk.len() {
            break;
        }
        let n = (buf.len as usize).min(disk.len() - off);
        crate::vmm::write_guest_memory(vm_id, buf.gpa, &disk[off..off + n]);
        lba += (n.div_ceil(SECTOR_SIZE)) as u64;
    }
    0
}

/// WRITE: copy driver-readable guest buffers into disk sectors.
fn blk_write(disk: &mut [u8], sector: u64, bufs: &[DescBuf], vm_id: usize) -> u8 {
    let mut lba = sector;
    for buf in bufs {
        let off = (lba as usize).saturating_mul(SECTOR_SIZE);
        if off >= disk.len() {
            break;
        }
        let n = (buf.len as usize).min(disk.len() - off);
        let mut tmp = vec![0u8; n];
        let got = crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut tmp);
        if got == 0 || got == usize::MAX {
            break;
        }
        disk[off..off + got].copy_from_slice(&tmp[..got]);
        lba += (got.div_ceil(SECTOR_SIZE)) as u64;
    }
    0
}

fn write_status(vm_id: usize, gpa: u64, status: u8) {
    crate::vmm::write_guest_memory(vm_id, gpa, &[status]);
}
