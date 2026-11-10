//! DRM transport shim for Mesa `anv` running on Cellos (ADR-0033 G1/G3).
//!
//! Intercepts `open("/dev/dri/renderD128")`, `ioctl`, `mmap`, `munmap` and `close` and
//! forwards each request, unchanged, to the Intel GPU driver Cell as one `XE_GPU` request
//! ([`intel_gpu_core::wire`]). The driver cannot follow the user pointers inside an ioctl
//! struct, so the shim declares the ranges each served request points at: bytes the driver
//! reads travel with the request, and the driver's writes come back in the reply and are
//! applied here, only inside the declared ranges.
//!
//! Buffer objects are this Cell's own memory (CP1 D-G1 option B): `GEM_CREATE` allocates
//! locally, exports the buffer to the driver and sends the unchanged `drm_xe_gem_create`
//! with the export id attached; `GEM_MMAP_OFFSET` + `mmap` resolve to the local pages;
//! `GEM_CLOSE` and `close` revoke the export after the driver dropped the object, and the
//! memory is freed once no CPU mapping of it remains (as a VMA keeps a GEM object alive).

use crate::memory::{ClientBuffer, ClientMemory};
use crate::transport::Transport;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use intel_gpu_core::uapi::ioctl::*;
use intel_gpu_core::uapi::{u16_at, u32_at, u64_at, UserMemory};
use intel_gpu_core::wire::{
    self, Segment, MAX_REPLY_LEN, MAX_REQUEST_LEN, OP_CLOSE, OP_IOCTL, OP_OPEN, SEG_IN, SEG_OUT,
};

/// One buffer object of an open file.
struct Bo {
    buf: ClientBuffer,
    export_id: u64,
    mmap_offset: Option<u64>,
}

/// Memory whose object is gone but which is still exported (revoke pending) or still
/// mapped by the CPU.
struct Orphan {
    buf: ClientBuffer,
    export_id: Option<u64>,
}

struct File {
    id: u32,
    bos: BTreeMap<u32, Bo>,
}

/// Open render nodes of one process, each backed by one driver-side DRM file.
pub struct DrmShim<T: Transport, M: ClientMemory> {
    transport: T,
    memory: M,
    next_fd: i32,
    files: BTreeMap<i32, File>,
    /// CPU mappings `(addr, len)` of buffers, by buffer id.
    maps: BTreeMap<usize, Vec<(u64, u64)>>,
    orphans: Vec<Orphan>,
}

impl<T: Transport, M: ClientMemory> DrmShim<T, M> {
    pub fn new(transport: T, memory: M) -> Self {
        Self {
            transport,
            memory,
            next_fd: 10, // Start above standard stdio descriptors
            files: BTreeMap::new(),
            maps: BTreeMap::new(),
            orphans: Vec::new(),
        }
    }

    /// The transport, e.g. to inspect a host-test server.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// The memory provider, e.g. to inspect a host-test fake.
    pub fn memory_mut(&mut self) -> &mut M {
        &mut self.memory
    }

    /// Buffers whose export revoke is still pending or that are still CPU-mapped.
    pub fn orphan_count(&self) -> usize {
        self.orphans.len()
    }

    pub fn open(&mut self, path: &str) -> Result<i32, i32> {
        self.settle();
        if path != "/dev/dri/renderD128" && path != "renderD128" {
            return Err(ENOENT);
        }
        let fd = self.next_fd;
        let next = fd.checked_add(1).ok_or(EMFILE)?;
        let id = self.call(OP_OPEN, 0, 0, &mut [], &[], &mut NoMemory)?;
        self.next_fd = next;
        self.files.insert(
            fd,
            File {
                id,
                bos: BTreeMap::new(),
            },
        );
        Ok(fd)
    }

    /// Like `close(2)`, the descriptor is gone even when the driver reports an error. The
    /// driver drops the file's GPU state first; then the shim revokes every export.
    pub fn close(&mut self, fd: i32) -> Result<(), i32> {
        let file = self.files.remove(&fd).ok_or(EBADF)?;
        let result = self
            .call(OP_CLOSE, file.id, 0, &mut [], &[], &mut NoMemory)
            .map(|_| ());
        for bo in file.bos.into_values() {
            self.retire(bo);
        }
        self.settle();
        result
    }

    /// Serve one ioctl. `arg` is the request's struct, exactly `ioctl_size(request)` bytes.
    pub fn ioctl(
        &mut self,
        fd: i32,
        request: u32,
        arg: &mut [u8],
        mem: &mut impl UserMemory,
    ) -> Result<(), i32> {
        self.settle();
        let file = self.files.get(&fd).ok_or(EBADF)?.id;
        if arg.len() != ioctl_size(request) {
            return Err(EINVAL);
        }
        match request {
            DRM_IOCTL_XE_GEM_CREATE => self.gem_create(fd, file, arg, mem),
            DRM_IOCTL_XE_GEM_MMAP_OFFSET => {
                self.call(OP_IOCTL, file, request, arg, &[], mem)?;
                let handle = u32_at(arg, 8);
                if let Some(bo) = self.bo_mut(fd, handle) {
                    bo.mmap_offset = Some(u64_at(arg, 16));
                }
                Ok(())
            }
            DRM_IOCTL_GEM_CLOSE => {
                self.call(OP_IOCTL, file, request, arg, &[], mem)?;
                let bo = self
                    .files
                    .get_mut(&fd)
                    .and_then(|f| f.bos.remove(&u32_at(arg, 0)));
                if let Some(bo) = bo {
                    self.retire(bo);
                }
                Ok(())
            }
            _ => {
                let segments = user_segments(request, arg)?;
                self.call(OP_IOCTL, file, request, arg, &segments, mem)
                    .map(|_| ())
            }
        }
    }

    /// `mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, offset)` of a GEM object's
    /// fake offset: the object's own pages in this Cell. Placed (`MAP_FIXED`) maps are not
    /// offered: the memory already has its address.
    pub fn mmap(&mut self, fd: i32, offset: u64, len: u64) -> Result<u64, i32> {
        let file = self.files.get(&fd).ok_or(EBADF)?;
        let bo = file
            .bos
            .values()
            .find(|bo| {
                bo.mmap_offset.is_some_and(|base| {
                    offset >= base
                        && len != 0
                        && offset
                            .checked_add(len)
                            .is_some_and(|end| end <= base + bo.buf.size as u64)
                })
            })
            .ok_or(EINVAL)?;
        let addr = bo.buf.addr + (offset - bo.mmap_offset.unwrap_or(offset));
        self.maps.entry(bo.buf.id).or_default().push((addr, len));
        Ok(addr)
    }

    /// `munmap` of (part of) a range [`Self::mmap`] returned.
    pub fn munmap(&mut self, addr: u64, len: u64) -> Result<(), i32> {
        let end = addr.checked_add(len).ok_or(EINVAL)?;
        let (&id, ranges) = self
            .maps
            .iter_mut()
            .find(|(_, ranges)| ranges.iter().any(|&(a, l)| a <= addr && end <= a + l))
            .ok_or(EINVAL)?;
        let mut kept = Vec::new();
        for (a, l) in ranges.drain(..) {
            let e = a + l;
            if e <= addr || a >= end {
                kept.push((a, l));
                continue;
            }
            if a < addr {
                kept.push((a, addr - a));
            }
            if e > end {
                kept.push((end, e - end));
            }
        }
        *ranges = kept;
        if ranges.is_empty() {
            self.maps.remove(&id);
        }
        self.settle();
        Ok(())
    }

    fn bo_mut(&mut self, fd: i32, handle: u32) -> Option<&mut Bo> {
        self.files.get_mut(&fd)?.bos.get_mut(&handle)
    }

    /// `GEM_CREATE` backed by this Cell's memory: allocate, export, attach. An allocation
    /// or export this Cell cannot make is `ENOMEM`, what `xe` returns when it cannot back
    /// an object.
    fn gem_create(
        &mut self,
        fd: i32,
        file: u32,
        arg: &mut [u8],
        mem: &mut impl UserMemory,
    ) -> Result<(), i32> {
        let size = usize::try_from(u64_at(arg, 8)).map_err(|_| EINVAL)?;
        if size == 0 || !size.is_multiple_of(4096) {
            return Err(EINVAL);
        }
        let buf = self.memory.alloc(size).ok_or(ENOMEM)?;
        // An xe object is always GPU-writable: read-only exists only per binding.
        let Some(export_id) = self.memory.export(&buf, true) else {
            self.memory.free(buf);
            return Err(ENOMEM);
        };
        let attachment = [Segment::export(export_id)];
        match self.call(
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_GEM_CREATE,
            arg,
            &attachment,
            mem,
        ) {
            Ok(_) => {
                let handle = u32_at(arg, 28);
                if let Some(f) = self.files.get_mut(&fd) {
                    f.bos.insert(
                        handle,
                        Bo {
                            buf,
                            export_id,
                            mmap_offset: None,
                        },
                    );
                }
                Ok(())
            }
            Err(errno) => {
                self.orphans.push(Orphan {
                    buf,
                    export_id: Some(export_id),
                });
                self.settle();
                Err(errno)
            }
        }
    }

    /// The driver no longer holds `bo`: revoke its export and free it when unmapped.
    fn retire(&mut self, bo: Bo) {
        self.orphans.push(Orphan {
            buf: bo.buf,
            export_id: Some(bo.export_id),
        });
        self.settle();
    }

    /// Finish what can be finished: pending revokes, then frees of unexported, unmapped
    /// buffers.
    fn settle(&mut self) {
        let mut i = 0;
        while i < self.orphans.len() {
            let orphan = &mut self.orphans[i];
            if let Some(id) = orphan.export_id {
                if self.memory.revoke(id).is_ok() {
                    orphan.export_id = None;
                }
            }
            if orphan.export_id.is_none() && !self.maps.contains_key(&orphan.buf.id) {
                let orphan = self.orphans.swap_remove(i);
                self.memory.free(orphan.buf);
            } else {
                i += 1;
            }
        }
    }

    fn call(
        &mut self,
        op: u8,
        file: u32,
        request: u32,
        arg: &mut [u8],
        segments: &[Segment],
        mem: &mut impl UserMemory,
    ) -> Result<u32, i32> {
        let mut frame = [0u8; MAX_REQUEST_LEN];
        let len = wire::encode_request(&mut frame, op, file, request, arg, segments, mem)?;
        let mut reply = [0u8; MAX_REPLY_LEN];
        let reply_len = self.transport.call(&frame[..len], &mut reply)?;
        let reply =
            wire::decode_reply(reply.get(..reply_len).ok_or(EIO)?, segments).map_err(|_| EIO)?;
        // `drm_ioctl` copies the struct out (for `_IOR`/`_IOWR`) and user writes land even
        // when the handler fails.
        if !reply.arg.is_empty() {
            if reply.arg.len() != arg.len() {
                return Err(EIO);
            }
            if ioctl_dir(request) & IOC_READ != 0 {
                arg.copy_from_slice(reply.arg);
            }
        }
        reply.apply_writes(mem)?;
        match reply.errno {
            0 => Ok(reply.value),
            errno => Err(errno),
        }
    }
}

/// The user ranges `request` points at, from its (already size-checked) struct.
fn user_segments(request: u32, arg: &[u8]) -> Result<Vec<Segment>, i32> {
    let mut segments = Vec::new();
    match request {
        // `drm_version`: name, date and desc buffers, each `{len u64, ptr u64}`.
        DRM_IOCTL_VERSION => {
            for len_at in [16, 32, 48] {
                let (len, ptr) = (u64_at(arg, len_at), u64_at(arg, len_at + 8));
                if len != 0 && ptr != 0 {
                    // The driver truncates to the buffer, so a huge buffer needs no more.
                    let len = u32::try_from(len).unwrap_or(u32::MAX);
                    segments.push(out(ptr, len));
                }
            }
        }
        // `drm_xe_device_query`: `size` bytes at `data` once the caller sized the query.
        DRM_IOCTL_XE_DEVICE_QUERY if u32_at(arg, 12) != 0 => {
            segments.push(out(u64_at(arg, 16), u32_at(arg, 12)));
        }
        // `drm_xe_exec_queue_create.instances`: `width * num_placements` engine instances.
        DRM_IOCTL_XE_EXEC_QUEUE_CREATE => {
            let count = usize::from(u16_at(arg, 8)) * usize::from(u16_at(arg, 10));
            segments.push(input(u64_at(arg, 24), count, EXEC_QUEUE_INSTANCE_SIZE)?);
        }
        // `drm_xe_vm_bind`: `vector_of_binds` when `num_binds > 1` (one op is inline), and
        // `num_syncs` `drm_xe_sync`s at `syncs`.
        DRM_IOCTL_XE_VM_BIND => {
            let num_binds = u32_at(arg, 20) as usize;
            if num_binds > 1 {
                segments.push(input(u64_at(arg, 24), num_binds, VM_BIND_OP_SIZE)?);
            }
            let num_syncs = u32_at(arg, 108) as usize;
            if num_syncs != 0 {
                segments.push(input(u64_at(arg, 112), num_syncs, XE_SYNC_SIZE)?);
            }
        }
        // `drm_xe_exec`: `num_syncs` syncs; `address` is the batch's GPU VA for one batch
        // buffer and a pointer to `num_batch_buffer` VAs for more.
        DRM_IOCTL_XE_EXEC => {
            let num_syncs = u32_at(arg, 12) as usize;
            if num_syncs != 0 {
                segments.push(input(u64_at(arg, 16), num_syncs, XE_SYNC_SIZE)?);
            }
            let batches = usize::from(u16_at(arg, 32));
            if batches > 1 {
                segments.push(input(u64_at(arg, 24), batches, 8)?);
            }
        }
        _ => {}
    }
    Ok(segments)
}

/// `count` elements of `size` bytes at `addr`, carried to the driver. An array that does
/// not fit one request frame cannot be sent: `ENOMEM`, as an allocation failure would be.
fn input(addr: u64, count: usize, size: usize) -> Result<Segment, i32> {
    let len = count
        .checked_mul(size)
        .filter(|&len| len <= MAX_REQUEST_LEN)
        .ok_or(ENOMEM)?;
    Ok(Segment {
        addr,
        len: len as u32,
        flags: SEG_IN,
    })
}

fn out(addr: u64, len: u32) -> Segment {
    Segment {
        addr,
        len,
        flags: SEG_OUT,
    }
}

/// Memory for operations that carry no user pointer.
struct NoMemory;

impl UserMemory for NoMemory {
    fn read(&self, _: u64, _: &mut [u8]) -> Result<(), i32> {
        Err(EFAULT)
    }

    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), i32> {
        Err(EFAULT)
    }
}
