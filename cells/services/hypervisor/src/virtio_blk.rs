//! Shared virtio-blk device model (DeviceID=2).
//!
//! ARM uses VirtIO-MMIO slot 1/SPI17; x86 uses slot 0/IRQ5. The backend is an
//! optional persistent VFS file with a 4 MiB volatile fallback.
//!
//! Chain layout (virtio-blk spec §5.2.6.1):
//!   [0]      outhdr   16B device-readable: { type:u32, _:u32, sector:u64 }
//!   [1..n-1] data     device-readable (OUT) or device-writable (IN)
//!   [last]   status   1B  device-writable: 0=OK 1=IOERR 2=UNSUPP

extern crate alloc;
use crate::virtio_mmio::{QueueCfg, VirtioDevice};
use crate::virtqueue::{process_notify, DescBuf};
use alloc::vec;
use ostd::io::println;

// The fallback must stay below the fixed 8 MiB cell heap. Alpine boots from
// initramfs and only needs this scratch volume for bounded ad-hoc writes.
const DISK_SIZE: usize = 4 * 1024 * 1024; // 4 MiB
const SECTOR_SIZE: usize = 512;
const NUM_SECTORS: u64 = (DISK_SIZE / SECTOR_SIZE) as u64;

const BLK_T_IN: u32 = 0; // read  — device → driver
const BLK_T_OUT: u32 = 1; // write — driver → device
const BLK_T_FLUSH: u32 = 4;
const BACKEND_TIMEOUT_TICKS: u64 = 200;

pub enum Backend {
    Volatile(alloc::vec::Vec<u8>),
    Persistent {
        vfs_tid: usize,
        poisoned_tid: usize,
        file: api::vfs_file_handles::ViVfsFileHandle,
        size: u64,
    },
}

pub struct BlkDisk {
    backend: Backend,
    num_sectors: u64,
    last_avail: u16,
    used_idx: u16,
    irq: Option<u32>,
}

impl BlkDisk {
    pub fn new(
        file: Option<(usize, api::vfs_file_handles::ViVfsFileHandle, u64)>,
        irq: Option<u32>,
    ) -> Self {
        let (backend, num_sectors) = match file {
            Some((vfs_tid, file, size)) => (
                Backend::Persistent {
                    vfs_tid,
                    poisoned_tid: 0,
                    file,
                    size,
                },
                size / (SECTOR_SIZE as u64),
            ),
            None => (Backend::Volatile(vec![0u8; DISK_SIZE]), NUM_SECTORS),
        };
        Self {
            backend,
            num_sectors,
            last_avail: 0,
            used_idx: 0,
            irq,
        }
    }
}

impl VirtioDevice for BlkDisk {
    fn device_id(&self) -> u32 {
        2
    }
    fn device_features_lo(&self) -> u32 {
        1 << 9 // VIRTIO_BLK_F_FLUSH
    }

    /// virtio-blk config: capacity at bytes 0-7 (little-endian u64 of sectors).
    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            0 => (self.num_sectors & 0xFFFF_FFFF) as u32,
            4 => (self.num_sectors >> 32) as u32,
            _ => 0,
        }
    }

    fn notify(&mut self, q: usize, qcfg: &QueueCfg, vm_id: usize, vcpu_id: usize) -> bool {
        if q != 0 {
            return false;
        }
        // Disjoint field borrows: backend / last_avail / used_idx
        let backend = &mut self.backend;
        let published = process_notify(
            vm_id,
            qcfg,
            &mut self.last_avail,
            &mut self.used_idx,
            |bufs| handle_blk_request(backend, bufs, vm_id),
        );
        if published > 0 {
            if let Some(irq) = self.irq {
                crate::vmm::inject_irq(vm_id, vcpu_id, irq);
            }
            true
        } else {
            false
        }
    }

    fn reset(&mut self) {
        self.last_avail = 0;
        self.used_idx = 0;
    }

    #[cfg(feature = "hostile-backend-recovery")]
    fn hostile_backend_fault(&mut self, command: u32) {
        if command == 1
            && matches!(self.backend, Backend::Persistent { .. })
            && crate::backend_fault_control::disconnect(api::syscall::service::VFS)
        {
            force_persistent_unavailable_once(&mut self.backend);
        }
    }
}

fn ensure_persistent_connected(backend: &mut Backend) -> bool {
    if matches!(
        backend,
        Backend::Persistent {
            vfs_tid: usize::MAX,
            ..
        }
    ) {
        mark_persistent_unavailable(backend, false);
        println("[hv-backend-fault-host] block unavailable");
        return false;
    }
    let Backend::Persistent {
        vfs_tid,
        poisoned_tid,
        file,
        size,
    } = backend
    else {
        return true;
    };
    let Some(current_tid) = ostd::syscall::sys_lookup_service(api::syscall::service::VFS) else {
        *vfs_tid = 0;
        return false;
    };
    if current_tid == *poisoned_tid {
        return false;
    }
    if current_tid == *vfs_tid && current_tid != 0 {
        return true;
    }
    let expected_size = *size;
    let mut poisoned = false;
    let Some((new_tid, new_file, new_size)) =
        crate::persistent_disk::open_for(current_tid, &mut poisoned)
    else {
        *vfs_tid = 0;
        if poisoned {
            *poisoned_tid = current_tid;
        }
        return false;
    };
    if new_size != expected_size {
        *vfs_tid = 0;
        return false;
    }
    *vfs_tid = new_tid;
    *poisoned_tid = 0;
    *file = new_file;
    println(&alloc::format!(
        "[hv-backend-fault-host] recovered service=vfs new_tid={}",
        new_tid
    ));
    true
}
fn mark_persistent_unavailable(backend: &mut Backend, poison: bool) {
    if let Backend::Persistent {
        vfs_tid,
        poisoned_tid,
        ..
    } = backend
    {
        if poison {
            *poisoned_tid = *vfs_tid;
        }
        *vfs_tid = 0;
    }
}

#[cfg(feature = "hostile-backend-recovery")]
fn force_persistent_unavailable_once(backend: &mut Backend) {
    if let Backend::Persistent {
        vfs_tid,
        poisoned_tid,
        ..
    } = backend
    {
        *poisoned_tid = *vfs_tid;
        *vfs_tid = usize::MAX;
    }
}

fn handle_blk_request(backend: &mut Backend, bufs: &[DescBuf], vm_id: usize) -> u32 {
    if bufs.len() < 2 {
        println("[hv-blk] descriptor chain has fewer than two buffers");
        return 0;
    }
    let status_idx = bufs.len() - 1;

    if bufs[0].len != 16
        || bufs[0].writable
        || bufs[status_idx].len != 1
        || !bufs[status_idx].writable
    {
        println("[hv-blk] malformed descriptor chain");
        return 0; // Malformed chain structure
    }

    let mut hdr = [0u8; 16];
    if crate::vmm::read_guest_memory(vm_id, bufs[0].gpa, &mut hdr) != 16 {
        println("[hv-blk] request header read failed");
        write_status(vm_id, bufs[status_idx].gpa, 1);
        return 1;
    }
    let req_type = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let sector = u64::from_le_bytes(hdr[8..16].try_into().unwrap_or([0u8; 8]));

    let recovering = matches!(
        backend,
        Backend::Persistent {
            vfs_tid: 0 | usize::MAX,
            ..
        }
    );
    let data_bufs = &bufs[1..status_idx];
    let status = match req_type {
        BLK_T_IN if bufs.len() >= 3 => blk_read(backend, sector, data_bufs, vm_id),
        BLK_T_OUT if bufs.len() >= 3 => blk_write(backend, sector, data_bufs, vm_id),
        BLK_T_FLUSH if bufs.len() == 2 => blk_flush(backend),
        BLK_T_IN | BLK_T_OUT | BLK_T_FLUSH => 1,
        _ => 2u8, // VIRTIO_BLK_S_UNSUPP
    };
    if status != 0 && !recovering {
        println(&alloc::format!(
            "[hv-blk-host] request failed type={} sector={} buffers={} status={}",
            req_type,
            sector,
            bufs.len(),
            status
        ));
    }
    write_status(vm_id, bufs[status_idx].gpa, status);
    1 // bytes placed in used ring (status byte)
}
fn blk_flush(backend: &mut Backend) -> u8 {
    if !ensure_persistent_connected(backend) {
        return 1;
    }
    match backend {
        Backend::Volatile(_) => 0,
        Backend::Persistent {
            vfs_tid,
            poisoned_tid,
            file,
            ..
        } => {
            let req = api::ipc::VfsRequest::SyncHandle { file: *file };
            let mut send_buf = [0u8; 512];
            let mut resp_buf = [0u8; 512];
            let result = ostd::ipc::service_call_typed_bounded(
                *vfs_tid,
                &req,
                &mut send_buf,
                &mut resp_buf,
                BACKEND_TIMEOUT_TICKS,
            );
            if matches!(&result, Ok(api::ipc::VfsResponse::Ok)) {
                0
            } else {
                if matches!(&result, Err(ostd::ipc::IpcError::Recv)) {
                    *poisoned_tid = *vfs_tid;
                }
                *vfs_tid = 0;
                1 // VIRTIO_BLK_S_IOERR
            }
        }
    }
}

fn blk_read(backend: &mut Backend, sector: u64, bufs: &[DescBuf], vm_id: usize) -> u8 {
    if !ensure_persistent_connected(backend) {
        return 1;
    }
    let capacity = match backend {
        Backend::Volatile(disk) => disk.len() as u64,
        Backend::Persistent { size, .. } => *size,
    };
    let mut off = sector.saturating_mul(SECTOR_SIZE as u64);

    let mut total_len = 0u64;
    for buf in bufs {
        if !buf.writable {
            return 1;
        }
        total_len = total_len.saturating_add(buf.len as u64);
    }
    if off.saturating_add(total_len) > capacity {
        return 1; // Out of bounds
    }

    for buf in bufs {
        match backend {
            Backend::Volatile(disk) => {
                let off_usize = off as usize;
                let n = buf.len as usize;
                if crate::vmm::write_guest_memory(vm_id, buf.gpa, &disk[off_usize..off_usize + n])
                    != n
                {
                    return 1;
                }
                off += n as u64;
            }
            Backend::Persistent {
                vfs_tid,
                poisoned_tid,
                file,
                ..
            } => {
                let mut n = buf.len as usize;
                let mut chunk_off = 0;
                while n > 0 {
                    let chunk = n.min(4096);
                    let grant_id = ostd::syscall::sys_grant_alloc(chunk).unwrap_or(0);
                    if grant_id == 0 {
                        return 1;
                    }
                    ostd::syscall::sys_grant_share(grant_id, *vfs_tid, 2 /* ReadWrite */);

                    let req = api::ipc::VfsRequest::ReadHandleGrant {
                        file: *file,
                        offset: off + chunk_off as u64,
                        size: chunk,
                        grant: grant_id,
                    };
                    let mut resp_buf = [0u8; 512];
                    let mut send_buf = [0u8; 512];
                    let result = ostd::ipc::service_call_typed_bounded(
                        *vfs_tid,
                        &req,
                        &mut send_buf,
                        &mut resp_buf,
                        BACKEND_TIMEOUT_TICKS,
                    );
                    let poison = matches!(&result, Err(ostd::ipc::IpcError::Recv));
                    let ok = if let Ok(api::ipc::VfsResponse::GrantDone { bytes }) = result {
                        let mut tmp = alloc::vec![0u8; chunk];
                        bytes == chunk
                            && ostd::syscall::sys_grant_copy_to_slice(grant_id, &mut tmp)
                                == Some(chunk)
                            && crate::vmm::write_guest_memory(
                                vm_id,
                                buf.gpa + chunk_off as u64,
                                &tmp,
                            ) == chunk
                    } else {
                        false
                    };

                    ostd::syscall::sys_grant_free(grant_id);
                    if !ok {
                        if poison {
                            *poisoned_tid = *vfs_tid;
                        }
                        *vfs_tid = 0;
                        return 1;
                    }
                    chunk_off += chunk;
                    n -= chunk;
                }
                off += buf.len as u64;
            }
        }
    }
    0
}

fn blk_write(backend: &mut Backend, sector: u64, bufs: &[DescBuf], vm_id: usize) -> u8 {
    if !ensure_persistent_connected(backend) {
        return 1;
    }
    let capacity = match backend {
        Backend::Volatile(disk) => disk.len() as u64,
        Backend::Persistent { size, .. } => *size,
    };
    let mut off = sector.saturating_mul(SECTOR_SIZE as u64);

    let mut total_len = 0u64;
    for buf in bufs {
        if buf.writable {
            println("[hv-blk] write data descriptor is device-writable");
            return 1;
        }
        total_len = total_len.saturating_add(buf.len as u64);
    }
    if off.saturating_add(total_len) > capacity {
        println("[hv-blk] write exceeds backend capacity");
        return 1; // Out of bounds
    }

    for buf in bufs {
        match backend {
            Backend::Volatile(disk) => {
                let off_usize = off as usize;
                let n = buf.len as usize;
                let mut tmp = alloc::vec![0u8; n];
                let got = crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut tmp);
                if got != n {
                    return 1;
                }
                disk[off_usize..off_usize + n].copy_from_slice(&tmp[..n]);
                off += n as u64;
            }
            Backend::Persistent {
                vfs_tid,
                poisoned_tid,
                file,
                ..
            } => {
                let mut n = buf.len as usize;
                let mut chunk_off = 0;
                while n > 0 {
                    let chunk = n.min(4096);
                    let mut tmp = alloc::vec![0u8; chunk];
                    let got =
                        crate::vmm::read_guest_memory(vm_id, buf.gpa + chunk_off as u64, &mut tmp);
                    if got != chunk {
                        println("[hv-blk] guest-memory read failed");
                        return 1;
                    }

                    let Some(grant_handle) =
                        ostd::grant::GrantHandle::<u8>::alloc_copy_from_slice(&tmp)
                    else {
                        println("[hv-blk] grant allocation/initialization failed");
                        return 1;
                    };
                    let grant_id = grant_handle.id();
                    if !ostd::syscall::sys_grant_share(grant_id, *vfs_tid, 1 /* WriteOnly */) {
                        println("[hv-blk] grant share failed");
                        return 1;
                    }

                    let req = api::ipc::VfsRequest::WriteHandleGrant {
                        file: *file,
                        offset: off + chunk_off as u64,
                        bytes: chunk,
                        grant: grant_id,
                    };
                    let mut resp_buf = [0u8; 512];
                    let mut send_buf = [0u8; 512];
                    let result = ostd::ipc::service_call_typed_bounded(
                        *vfs_tid,
                        &req,
                        &mut send_buf,
                        &mut resp_buf,
                        BACKEND_TIMEOUT_TICKS,
                    );
                    let poison = matches!(&result, Err(ostd::ipc::IpcError::Recv));
                    let ok = match result {
                        Ok(api::ipc::VfsResponse::GrantDone { bytes }) => bytes == chunk,
                        Ok(response) => {
                            println(&alloc::format!(
                                "[hv-blk] VFS write response: {:?}",
                                response
                            ));
                            false
                        }
                        Err(error) => {
                            println(&alloc::format!("[hv-blk] VFS write failed: {:?}", error));
                            false
                        }
                    };

                    drop(grant_handle);
                    if !ok {
                        if poison {
                            *poisoned_tid = *vfs_tid;
                        }
                        *vfs_tid = 0;
                        return 1;
                    }
                    chunk_off += chunk;
                    n -= chunk;
                }
                off += buf.len as u64;
            }
        }
    }
    0
}

fn write_status(vm_id: usize, gpa: u64, status: u8) {
    crate::vmm::write_guest_memory(vm_id, gpa, &[status]);
}
