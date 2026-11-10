//! The Linux `xe` uAPI subset Mesa `anv` issues for compute (ADR-0033 G1), decoded at the
//! uapi struct offsets over one open DRM file's [`XeSession`] and [`Syncobjs`].
//!
//! Upstream reference: `include/uapi/drm/drm.h`, `include/uapi/drm/xe_drm.h` and
//! `drivers/gpu/drm/drm_ioctl.c` (MIT). Requests are matched on the full Linux encoding
//! (direction, type, number, struct size). The driver Cell's IPC service and the host
//! harnesses run this one decoder, so a request means the same thing on both.

pub mod ioctl;
pub mod query;

use crate::backend::{
    XeError, XeSession, MAX_EXEC_SYNCS, XE_ENGINE_CLASS_RENDER, XE_EXEC_QUEUE_PRIORITY_NORMAL,
    XE_EXEC_QUEUE_PROPERTY_BAN,
};
use crate::syncobj::Syncobjs;
use alloc::vec::Vec;
use ioctl::*;
use query::{query_payload, DeviceInfo};

/// The calling process's memory, for the user pointers embedded in ioctl structs.
pub trait UserMemory {
    /// Copy `out.len()` bytes from user address `addr`; `Err(EFAULT)` if not readable.
    fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), i32>;
    /// Copy `data` to user address `addr`; `Err(EFAULT)` if not writable.
    fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), i32>;
}

/// One open DRM file (`struct drm_file`): its GPU objects and sync-object namespace.
#[derive(Default)]
pub struct DrmFile {
    pub session: XeSession,
    pub syncobjs: Syncobjs,
}

impl DrmFile {
    pub fn new() -> Self {
        Self::default()
    }

    /// `postclose`: revoke every GPU mapping, cancel queued work and signal its fences.
    /// Returns the export ids whose leases the GPU no longer reaches.
    pub fn release(&mut self) -> Vec<u64> {
        self.session.on_client_death(&mut self.syncobjs)
    }

    /// Serve one ioctl. `arg` is the request's struct, exactly `ioctl_size(request)` bytes;
    /// like `drm_ioctl`, it may be updated even when an error is returned.
    pub fn ioctl(
        &mut self,
        device: &DeviceInfo,
        request: u32,
        arg: &mut [u8],
        mem: &mut impl UserMemory,
    ) -> Result<(), i32> {
        if arg.len() != ioctl_size(request) {
            return Err(EINVAL);
        }
        match request {
            DRM_IOCTL_VERSION => version(device, arg, mem),
            DRM_IOCTL_GET_CAP => {
                match u64_at(arg, 0) {
                    DRM_CAP_SYNCOBJ | DRM_CAP_SYNCOBJ_TIMELINE => put(arg, 8, &1u64.to_le_bytes()),
                    _ => return Err(EINVAL),
                }
                Ok(())
            }
            DRM_IOCTL_XE_DEVICE_QUERY => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 24..40)?;
                let payload = query_payload(device, u32_at(arg, 8))?;
                let size = u32::try_from(payload.len()).map_err(|_| EINVAL)?;
                match u32_at(arg, 12) {
                    0 => put(arg, 12, &size.to_le_bytes()),
                    n if n != size => return Err(EINVAL),
                    _ => mem.write(u64_at(arg, 16), &payload)?,
                }
                Ok(())
            }
            DRM_IOCTL_XE_GEM_CREATE => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 34..56)?;
                // Bit 0 is the only region `MEM_REGIONS` reports (sysmem instance 0).
                if u32_at(arg, 16) != 1 || u32_at(arg, 20) & !0xF != 0 {
                    return Err(EINVAL);
                }
                let size = usize::try_from(u64_at(arg, 8)).map_err(|_| EINVAL)?;
                let handle = self
                    .session
                    .gem_create(size, 0, u32_at(arg, 24), u16_at(arg, 32))
                    .map_err(xe_to_errno)?;
                put(arg, 28, &handle.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_XE_GEM_MMAP_OFFSET => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 12..16)?; // PCI_BARRIER offsets are not served
                zeroed(arg, 24..40)?;
                let offset = self
                    .session
                    .gem_mmap_offset(u32_at(arg, 8))
                    .map_err(xe_to_errno)?;
                put(arg, 16, &offset.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_GEM_CLOSE => self.session.gem_close(u32_at(arg, 0)).map_err(xe_to_errno),
            DRM_IOCTL_XE_VM_CREATE => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 16..32)?;
                let flags = u32_at(arg, 8);
                if flags & !0xF != 0 {
                    return Err(EINVAL);
                }
                let vm_id = self.session.vm_create(flags).map_err(xe_to_errno)?;
                put(arg, 12, &vm_id.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_XE_VM_DESTROY => {
                zeroed(arg, 4..24)?;
                self.session.vm_destroy(u32_at(arg, 0)).map_err(xe_to_errno)
            }
            DRM_IOCTL_XE_EXEC_QUEUE_CREATE => self.exec_queue_create(arg, mem),
            DRM_IOCTL_XE_EXEC_QUEUE_DESTROY => {
                zeroed(arg, 4..24)?;
                self.session
                    .exec_queue_destroy(u32_at(arg, 0))
                    .map_err(xe_to_errno)
            }
            DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 24..40)?;
                if u32_at(arg, 12) != XE_EXEC_QUEUE_PROPERTY_BAN {
                    return Err(EINVAL);
                }
                let ban = self
                    .session
                    .exec_queue_get_property_ban(u32_at(arg, 8))
                    .map_err(xe_to_errno)?;
                put(arg, 16, &u64::from(ban).to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_XE_EXEC => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 34..56)?;
                let num_syncs = u32_at(arg, 12) as usize;
                if u16_at(arg, 32) != 1 || num_syncs > MAX_EXEC_SYNCS {
                    return Err(EINVAL);
                }
                // In/out syncobjs are not wired to job completion yet; accepting them would
                // leave Mesa waiting forever on fences nothing signals.
                if num_syncs != 0 {
                    return Err(EOPNOTSUPP);
                }
                self.session
                    .exec(u32_at(arg, 8), u64_at(arg, 24), 0, None)
                    .map_err(xe_to_errno)?;
                Ok(())
            }
            DRM_IOCTL_SYNCOBJ_CREATE => {
                let handle = self.syncobjs.create(u32_at(arg, 4)).map_err(|_| EINVAL)?;
                put(arg, 0, &handle.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_SYNCOBJ_DESTROY => {
                zeroed(arg, 4..8)?;
                self.syncobjs.destroy(u32_at(arg, 0)).map_err(|_| EINVAL)
            }
            _ => Err(EINVAL),
        }
    }

    fn exec_queue_create(&mut self, arg: &mut [u8], mem: &impl UserMemory) -> Result<(), i32> {
        zeroed(arg, 0..8)?;
        zeroed(arg, 32..48)?;
        // Queues are single-engine; `flags` may carry only LOW_LATENCY_HINT.
        if u16_at(arg, 8) != 1 || u16_at(arg, 10) != 1 || u32_at(arg, 16) & !1 != 0 {
            return Err(EINVAL);
        }
        let mut instance = [0u8; EXEC_QUEUE_INSTANCE_SIZE];
        mem.read(u64_at(arg, 24), &mut instance)?;
        // Only the engine `ENGINES` reports exists: RENDER instance 0 on GT 0.
        if u16_at(&instance, 0) != XE_ENGINE_CLASS_RENDER || instance[2..6] != [0; 4] {
            return Err(EINVAL);
        }
        let q_id = self
            .session
            .exec_queue_create(
                u32_at(arg, 12),
                XE_ENGINE_CLASS_RENDER,
                XE_EXEC_QUEUE_PRIORITY_NORMAL,
            )
            .map_err(xe_to_errno)?;
        put(arg, 20, &q_id.to_le_bytes());
        Ok(())
    }
}

/// `drm_version`: numbers in place, strings through `drm_copy_field` (truncate to the
/// caller's buffer, always report the full length).
fn version(device: &DeviceInfo, arg: &mut [u8], mem: &mut impl UserMemory) -> Result<(), i32> {
    put(arg, 0, &1i32.to_le_bytes()); // version_major
    put(arg, 4, &0i32.to_le_bytes()); // version_minor
    put(arg, 8, &0i32.to_le_bytes()); // version_patchlevel
    for (len_at, value) in [(16, "xe"), (32, ""), (48, device.name)] {
        let cap = usize::try_from(u64_at(arg, len_at)).unwrap_or(usize::MAX);
        let ptr = u64_at(arg, len_at + 8);
        let n = value.len().min(cap);
        if n != 0 && ptr != 0 {
            mem.write(ptr, &value.as_bytes()[..n])?;
        }
        put(arg, len_at, &(value.len() as u64).to_le_bytes());
    }
    Ok(())
}

fn zeroed(arg: &[u8], range: core::ops::Range<usize>) -> Result<(), i32> {
    if arg[range].iter().all(|&b| b == 0) {
        Ok(())
    } else {
        Err(EINVAL)
    }
}

fn put(arg: &mut [u8], offset: usize, bytes: &[u8]) {
    arg[offset..offset + bytes.len()].copy_from_slice(bytes);
}

/// Little-endian `u16` at `offset` of a uapi struct.
pub fn u16_at(arg: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([arg[offset], arg[offset + 1]])
}

/// Little-endian `u32` at `offset` of a uapi struct.
pub fn u32_at(arg: &[u8], offset: usize) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&arg[offset..offset + 4]);
    u32::from_le_bytes(bytes)
}

/// Little-endian `u64` at `offset` of a uapi struct.
pub fn u64_at(arg: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&arg[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

fn xe_to_errno(err: XeError) -> i32 {
    match err {
        XeError::Invalid => EINVAL,
        XeError::NoEntry => ENOENT,
        XeError::OutOfMemory => ENOMEM,
        XeError::Canceled => ECANCELED,
        XeError::PermissionDenied => EPERM,
        XeError::NotSupported => EOPNOTSUPP,
        XeError::TryAgain => EAGAIN,
    }
}
