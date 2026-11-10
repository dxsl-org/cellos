//! DRM transport shim for Mesa `anv` running on Cellos (ADR-0033 G1/G3).
//!
//! Intercepts `open("/dev/dri/renderD128")`, `ioctl` and `close` and forwards each, unchanged,
//! to the Intel GPU driver Cell as one `XE_GPU` request ([`intel_gpu_core::wire`]). The driver
//! cannot follow the user pointers inside an ioctl struct, so the shim declares the ranges each
//! served request points at: bytes the driver reads travel with the request, and the driver's
//! writes come back in the reply and are applied here, only inside the declared ranges.

use crate::transport::Transport;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use intel_gpu_core::uapi::ioctl::*;
use intel_gpu_core::uapi::{u16_at, u32_at, u64_at, UserMemory};
use intel_gpu_core::wire::{
    self, Segment, MAX_REPLY_LEN, MAX_REQUEST_LEN, OP_CLOSE, OP_IOCTL, OP_OPEN, SEG_IN, SEG_OUT,
};

/// Open render nodes of one process, each backed by one driver-side DRM file.
pub struct DrmShim<T: Transport> {
    transport: T,
    next_fd: i32,
    /// fd → driver file id.
    files: BTreeMap<i32, u32>,
}

impl<T: Transport> DrmShim<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            next_fd: 10, // Start above standard stdio descriptors
            files: BTreeMap::new(),
        }
    }

    /// The transport, e.g. to inspect a host-test server.
    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn open(&mut self, path: &str) -> Result<i32, i32> {
        if path != "/dev/dri/renderD128" && path != "renderD128" {
            return Err(ENOENT);
        }
        let fd = self.next_fd;
        let next = fd.checked_add(1).ok_or(EMFILE)?;
        let file = self.call(OP_OPEN, 0, 0, &mut [], &[], &mut NoMemory)?;
        self.next_fd = next;
        self.files.insert(fd, file);
        Ok(fd)
    }

    /// Like `close(2)`, the descriptor is gone even when the driver reports an error.
    pub fn close(&mut self, fd: i32) -> Result<(), i32> {
        let file = self.files.remove(&fd).ok_or(EBADF)?;
        self.call(OP_CLOSE, file, 0, &mut [], &[], &mut NoMemory)
            .map(|_| ())
    }

    /// Serve one ioctl. `arg` is the request's struct, exactly `ioctl_size(request)` bytes.
    pub fn ioctl(
        &mut self,
        fd: i32,
        request: u32,
        arg: &mut [u8],
        mem: &mut impl UserMemory,
    ) -> Result<(), i32> {
        let file = *self.files.get(&fd).ok_or(EBADF)?;
        if arg.len() != ioctl_size(request) {
            return Err(EINVAL);
        }
        let segments = user_segments(request, arg)?;
        self.call(OP_IOCTL, file, request, arg, &segments, mem)
            .map(|_| ())
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
            let len = count
                .checked_mul(EXEC_QUEUE_INSTANCE_SIZE)
                .filter(|&len| len <= MAX_REQUEST_LEN)
                .ok_or(EINVAL)?;
            segments.push(Segment {
                addr: u64_at(arg, 24),
                len: len as u32,
                flags: SEG_IN,
            });
        }
        _ => {}
    }
    Ok(segments)
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
