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
/// IPC budget for a single VFS round trip, in scheduler ticks.
///
/// Sized for a 4 KiB block request — the unit this device used to move. A
/// batched chunk moves up to `VFS_GRANT_CHUNK` in one call, and the VFS side of
/// that call performs one FAT sector operation per 512 bytes, so the budget
/// scales with the chunk (see `chunk_timeout_ticks`). A budget that only covers
/// the round trip turns a slow-but-correct VFS into a poisoned connection.
const BACKEND_TIMEOUT_TICKS: u64 = 200;
/// Extra IPC budget per 4 KiB of a batched chunk.
const BACKEND_TICKS_PER_4K: u64 = 200;

/// IPC budget for a chunk of `bytes`, never below the base request budget.
fn chunk_timeout_ticks(bytes: usize) -> u64 {
    BACKEND_TIMEOUT_TICKS + (bytes.div_ceil(4096) as u64) * BACKEND_TICKS_PER_4K
}

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
    ///
    /// `seg_max` (byte 12, `VIRTIO_BLK_F_SEG_MAX`) is deliberately NOT advertised
    /// yet. **Any** multi-segment request — one bio split across two or more data
    /// descriptors — breaks the guest's view of the disk on this device model:
    /// the next read of an affected sector comes back as zeros (or as a plain
    /// I/O error), while the device itself reported success. The isolated
    /// experiments, each a full two-boot lane run:
    ///
    /// - retired batching code + `seg_max` advertised → run 1 fails at the first
    ///   block read, so this is not an artefact of the batching in this file;
    /// - this code + `seg_max: 2` → same failure, so it is not chain *length*;
    /// - either code with the feature off → both boots pass.
    ///
    /// What is already ruled out: the scatter writes the right bytes into the
    /// descriptor's frame (read back through `ReadGuestMemory` immediately after
    /// the write), no request ends in `VIRTIO_BLK_S_IOERR`, the chain is never
    /// rejected by the guard (`[hv-virtio-host] reject descriptor-chain` never
    /// fires), `VIRTIO_RING_F_INDIRECT_DESC` is not advertised so Linux cannot
    /// be hiding segments in an indirect table, the host image still holds the
    /// data after the failure, and reporting the spec's used-ring length
    /// (payload + status byte, see below) changes nothing.
    ///
    /// The remaining difference is *when* the guest reads: with the feature on,
    /// Linux probes the disk earlier in boot, and the failing boot logs an I/O
    /// error on logical block 0 before any request could have been malformed.
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

    // Used-ring length: the bytes the device wrote into the chain's
    // device-writable part. A successful read writes the data *and* the status
    // byte; every other outcome (write request, flush, failure) writes only the
    // status byte. Reported as 1 for reads too until now, which is short by the
    // payload.
    let payload: u32 = if req_type == BLK_T_IN && status == 0 {
        data_bufs.iter().map(|buf| buf.len).sum()
    } else {
        0
    };
    1 + payload
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
                // One grant (and one VFS round trip) per chunk of the whole
                // request, scattered across the chain's guest buffers. The
                // guest does not care whether a request arrived as one
                // descriptor or sixty-four.
                let mut done = 0usize;
                let mut chunks = 0usize;
                while done < total_len as usize {
                    let want = total_len as usize - done;
                    let Some((mut grant, chunk)) = alloc_chunk_grant(want) else {
                        println("[hv-blk] grant allocation failed");
                        return 1;
                    };
                    if !ostd::syscall::sys_grant_share(
                        grant.id(),
                        *vfs_tid,
                        2, // ReadWrite — the VFS fills the grant
                    ) {
                        println("[hv-blk] grant share failed");
                        return 1;
                    }

                    let req = api::ipc::VfsRequest::ReadHandleGrant {
                        file: *file,
                        offset: off + done as u64,
                        size: chunk,
                        grant: grant.id(),
                    };
                    let mut resp_buf = [0u8; 512];
                    let mut send_buf = [0u8; 512];
                    let result = ostd::ipc::service_call_typed_bounded(
                        *vfs_tid,
                        &req,
                        &mut send_buf,
                        &mut resp_buf,
                        chunk_timeout_ticks(chunk),
                    );
                    let poison = matches!(&result, Err(ostd::ipc::IpcError::Recv));
                    let ok = match result {
                        Ok(api::ipc::VfsResponse::GrantDone { bytes }) if bytes == chunk => {
                            // The safe accessor carries the exclusivity proof: the
                            // handle is the region's only owner wrapper.
                            let scattered = grant.with_bytes(|data| {
                                scatter_to_guest(vm_id, bufs, done, &data[..chunk])
                            });
                            if !scattered {
                                println(&alloc::format!(
                                    "[hv-blk] read scatter failed off={} bytes={}",
                                    off + done as u64,
                                    chunk
                                ));
                            }
                            scattered
                        }
                        Ok(response) => {
                            println(&alloc::format!(
                                "[hv-blk] VFS read response: {:?} off={} bytes={}",
                                response,
                                off + done as u64,
                                chunk
                            ));
                            false
                        }
                        Err(error) => {
                            println(&alloc::format!(
                                "[hv-blk] VFS read failed: {:?} off={} bytes={}",
                                error,
                                off + done as u64,
                                chunk
                            ));
                            false
                        }
                    };

                    drop(grant);
                    if !ok {
                        if poison {
                            *poisoned_tid = *vfs_tid;
                        }
                        *vfs_tid = 0;
                        return 1;
                    }
                    done += chunk;
                    chunks += 1;
                }
                if total_len >= 65536 {
                    println(&alloc::format!(
                        "[hv-blk] read bytes={} chunks={}",
                        total_len,
                        chunks
                    ));
                }
                off += total_len;
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
                let got = crate::vmm::read_guest_memory(vm_id, buf.gpa, &mut disk[off_usize..off_usize + n]);
                if got != n {
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
                // Gather the request's guest buffers into one grant per chunk,
                // then hand the VFS that chunk in a single round trip.
                let mut done = 0usize;
                let mut chunks = 0usize;
                while done < total_len as usize {
                    let want = total_len as usize - done;
                    let Some((mut grant, chunk)) = alloc_chunk_grant(want) else {
                        println("[hv-blk] grant allocation failed");
                        return 1;
                    };
                    // The safe accessor carries the exclusivity proof; the grant
                    // is shared only after the request's bytes are in place.
                    let filled = grant
                        .with_bytes_mut(|data| gather_from_guest(vm_id, bufs, done, &mut data[..chunk]));
                    if !filled {
                        println("[hv-blk] guest-memory read failed");
                        return 1;
                    }
                    let grant_id = grant.id();
                    if !ostd::syscall::sys_grant_share(grant_id, *vfs_tid, 1 /* WriteOnly */) {
                        println("[hv-blk] grant share failed");
                        return 1;
                    }

                    let req = api::ipc::VfsRequest::WriteHandleGrant {
                        file: *file,
                        offset: off + done as u64,
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
                        chunk_timeout_ticks(chunk),
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

                    drop(grant);
                    if !ok {
                        if poison {
                            *poisoned_tid = *vfs_tid;
                        }
                        *vfs_tid = 0;
                        return 1;
                    }
                    done += chunk;
                    chunks += 1;
                }
                if total_len >= 65536 {
                    println(&alloc::format!(
                        "[hv-blk] write bytes={} chunks={}",
                        total_len,
                        chunks
                    ));
                }
                off += total_len;
            }
        }
    }
    0
}

/// Allocate the largest usable grant chunk for `want` bytes.
///
/// Grants come from contiguous frames, so a large chunk can fail once the
/// allocator is fragmented. Halving down to one page keeps bulk I/O working
/// (more round trips) instead of failing the request.
fn alloc_chunk_grant(want: usize) -> Option<(ostd::grant::GrantHandle<u8>, usize)> {
    // A request can be smaller than the chunk cap — a single sector, or the
    // tail of a chain — so the halving floor is one byte: refusing a small
    // request outright would fail writes the device is supposed to serve.
    let mut chunk = want.min(api::ipc::VFS_GRANT_CHUNK);
    loop {
        if let Some(handle) = ostd::grant::GrantHandle::<u8>::alloc(chunk) {
            return Some((handle, chunk));
        }
        if chunk <= 1 {
            return None;
        }
        chunk = (chunk / 2).max(1);
    }
}

/// Copy `src` into the guest buffers of `bufs`, starting at request offset `start`.
///
/// `bufs` are the request's data descriptors in chain order; their lengths
/// concatenate into the request's byte range.
fn scatter_to_guest(vm_id: usize, bufs: &[DescBuf], start: usize, src: &[u8]) -> bool {
    let mut written = 0usize;
    let mut cursor = 0usize;
    for buf in bufs {
        let len = buf.len as usize;
        if cursor + len > start {
            let local = start.saturating_sub(cursor);
            let take = (len - local).min(src.len() - written);
            if crate::vmm::write_guest_memory(
                vm_id,
                buf.gpa + local as u64,
                &src[written..written + take],
            ) != take
            {
                return false;
            }
            written += take;
            if written == src.len() {
                break;
            }
        }
        cursor += len;
    }
    written == src.len()
}

/// Copy the guest bytes of `bufs` at request offset `start` into `dst`.
fn gather_from_guest(vm_id: usize, bufs: &[DescBuf], start: usize, dst: &mut [u8]) -> bool {
    let mut read = 0usize;
    let mut cursor = 0usize;
    for buf in bufs {
        let len = buf.len as usize;
        if cursor + len > start {
            let local = start.saturating_sub(cursor);
            let take = (len - local).min(dst.len() - read);
            if crate::vmm::read_guest_memory(
                vm_id,
                buf.gpa + local as u64,
                &mut dst[read..read + take],
            ) != take
            {
                return false;
            }
            read += take;
            if read == dst.len() {
                break;
            }
        }
        cursor += len;
    }
    read == dst.len()
}

fn write_status(vm_id: usize, gpa: u64, status: u8) {
    crate::vmm::write_guest_memory(vm_id, gpa, &[status]);
}
