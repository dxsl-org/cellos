//! The Linux `xe` uAPI subset Mesa `anv` issues for compute (ADR-0033 G1), decoded at the
//! uapi struct offsets over one open DRM file's [`XeSession`] and [`Syncobjs`].
//!
//! Upstream reference: `include/uapi/drm/drm.h`, `include/uapi/drm/xe_drm.h`,
//! `drivers/gpu/drm/drm_ioctl.c` and the `xe_*_ioctl` argument checks in
//! `drivers/gpu/drm/xe/{xe_bo,xe_vm,xe_exec_queue,xe_exec}.c` (MIT). Requests are matched
//! on the full Linux encoding (direction, type, number, struct size). The driver Cell's
//! IPC service and the host harnesses run this one decoder, so a request means the same
//! thing on both.

pub mod ioctl;
pub mod query;

use crate::backend::{
    BindOp, XeError, XeSession, MAX_EXEC_SYNCS, XE_ENGINE_CLASS_RENDER,
    XE_EXEC_QUEUE_PRIORITY_NORMAL, XE_EXEC_QUEUE_PROPERTY_BAN, XE_VM_BIND_OP_PREFETCH,
};
use crate::hw::{ExportError, GpuHw};
use crate::syncobj::Syncobjs;
use alloc::vec;
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

/// The kernel-attested Cell incarnation that sent a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sender {
    pub cell_id: u64,
    pub generation: u64,
}

/// Everything one ioctl may use besides its struct and user memory.
pub struct IoctlContext<'a, H: GpuHw> {
    /// Who sent the request: the owner every attached export must have.
    pub sender: Sender,
    /// Export ids attached to the request (`SEG_EXPORT`, [`crate::wire`]).
    pub exports: &'a [u64],
    pub hw: &'a mut H,
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

    /// `postclose`: stop the file's contexts, clear its GPU mappings and revoke its export
    /// leases, in that order. Returns the revoked export ids.
    pub fn release(&mut self, hw: &mut impl GpuHw) -> Vec<u64> {
        self.session.on_client_death(&mut self.syncobjs, hw)
    }

    /// Serve one ioctl. `arg` is the request's struct, exactly `ioctl_size(request)` bytes;
    /// like `drm_ioctl`, it may be updated even when an error is returned.
    pub fn ioctl<H: GpuHw>(
        &mut self,
        device: &DeviceInfo,
        request: u32,
        arg: &mut [u8],
        mem: &mut impl UserMemory,
        ctx: &mut IoctlContext<'_, H>,
    ) -> Result<(), i32> {
        if arg.len() != ioctl_size(request) {
            return Err(EINVAL);
        }
        // Only an object creation takes memory; an attachment anywhere else is a
        // malformed request, not something to ignore.
        if request != DRM_IOCTL_XE_GEM_CREATE && !ctx.exports.is_empty() {
            return Err(EINVAL);
        }
        let hw = &mut *ctx.hw;
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
            DRM_IOCTL_XE_GEM_CREATE => self.gem_create(arg, ctx),
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
            DRM_IOCTL_GEM_CLOSE => self
                .session
                .gem_close(u32_at(arg, 0), hw)
                .map_err(xe_to_errno),
            DRM_IOCTL_XE_VM_CREATE => {
                zeroed(arg, 0..8)?;
                zeroed(arg, 16..32)?;
                let flags = u32_at(arg, 8);
                if flags & !0xF != 0 {
                    return Err(EINVAL);
                }
                let vm_id = self.session.vm_create(flags, hw).map_err(xe_to_errno)?;
                put(arg, 12, &vm_id.to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_XE_VM_DESTROY => {
                zeroed(arg, 4..24)?;
                self.session
                    .vm_destroy(u32_at(arg, 0), hw)
                    .map_err(xe_to_errno)
            }
            DRM_IOCTL_XE_VM_BIND => self.vm_bind(arg, mem, hw),
            DRM_IOCTL_XE_EXEC_QUEUE_CREATE => self.exec_queue_create(arg, mem, hw),
            DRM_IOCTL_XE_EXEC_QUEUE_DESTROY => {
                zeroed(arg, 4..24)?;
                self.session
                    .exec_queue_destroy(u32_at(arg, 0), &mut self.syncobjs, hw)
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
                    .exec_queue_ban(u32_at(arg, 8), hw)
                    .map_err(xe_to_errno)?;
                put(arg, 16, &u64::from(ban).to_le_bytes());
                Ok(())
            }
            DRM_IOCTL_XE_EXEC => self.exec(arg, hw),
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

    /// `drm_xe_gem_create` over the request's one export attachment (CP1 D-G1 option B).
    ///
    /// Struct checks run before the export is mapped, so a malformed request has no
    /// kernel-side effect. Without an attachment there is no memory to place the object
    /// in, which `xe` reports for a placement it cannot serve: `-EINVAL`.
    fn gem_create<H: GpuHw>(
        &mut self,
        arg: &mut [u8],
        ctx: &mut IoctlContext<'_, H>,
    ) -> Result<(), i32> {
        zeroed(arg, 0..8)?;
        zeroed(arg, 34..56)?;
        // Bit 0 is the only region `MEM_REGIONS` reports (sysmem instance 0).
        if u32_at(arg, 16) != 1 || u32_at(arg, 20) & !0xF != 0 {
            return Err(EINVAL);
        }
        let size = usize::try_from(u64_at(arg, 8)).map_err(|_| EINVAL)?;
        let &[export_id] = ctx.exports else {
            return Err(EINVAL);
        };
        let record = ctx.hw.map_export(export_id).map_err(export_errno)?;
        if record.export_id != export_id {
            return Err(EIO);
        }
        // Confused-deputy check (CP1 §5.2): the lease must belong to the Cell incarnation
        // that sent this request, not to whichever client's id it carries.
        if (record.owner_cell_id, record.owner_generation)
            != (ctx.sender.cell_id, ctx.sender.generation)
        {
            return Err(EPERM);
        }
        let handle = self
            .session
            .gem_create(size, &record, u32_at(arg, 24), u16_at(arg, 32))
            .map_err(xe_to_errno)?;
        put(arg, 28, &handle.to_le_bytes());
        Ok(())
    }

    /// `drm_xe_vm_bind`: one inline op, or `num_binds` ops at `vector_of_binds`.
    fn vm_bind(
        &mut self,
        arg: &[u8],
        mem: &impl UserMemory,
        hw: &mut impl GpuHw,
    ) -> Result<(), i32> {
        zeroed(arg, 0..8)?; // extensions
        zeroed(arg, 16..20)?; // pad
        zeroed(arg, 104..108)?; // pad2
        zeroed(arg, 120..136)?; // reserved
        let num_syncs = u32_at(arg, 108) as usize;
        if num_syncs > MAX_EXEC_SYNCS {
            return Err(EINVAL);
        }
        let vm_id = u32_at(arg, 8);
        let num_binds = u32_at(arg, 20) as usize;
        let ops_bytes = match num_binds {
            0 => Vec::new(),
            1 => arg[24..24 + VM_BIND_OP_SIZE].to_vec(),
            n => {
                let len = n.checked_mul(VM_BIND_OP_SIZE).ok_or(EINVAL)?;
                let mut bytes = vec![0u8; len];
                mem.read(u64_at(arg, 24), &mut bytes)?;
                bytes
            }
        };
        let ops = ops_bytes
            .chunks_exact(VM_BIND_OP_SIZE)
            .map(decode_bind_op)
            .collect::<Result<Vec<_>, _>>()?;
        // Bind queues of class VM_BIND are not created by this driver: a named queue is
        // either unknown or not a bind queue (`xe_vm_bind_ioctl`).
        let queue = u32_at(arg, 12);
        if queue != 0 {
            return Err(if self.session.exec_queues.contains_key(&queue) {
                EINVAL
            } else {
                ENOENT
            });
        }
        // Binds are applied synchronously, but in-syncs cannot be waited on and out-syncs
        // are not wired to the device's TLB invalidation completion yet: refuse rather
        // than signal something that has not happened.
        if num_syncs != 0 {
            return Err(EOPNOTSUPP);
        }
        self.session.vm_bind(vm_id, &ops, hw).map_err(xe_to_errno)
    }

    fn exec_queue_create(
        &mut self,
        arg: &mut [u8],
        mem: &impl UserMemory,
        hw: &mut impl GpuHw,
    ) -> Result<(), i32> {
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
                hw,
            )
            .map_err(xe_to_errno)?;
        put(arg, 20, &q_id.to_le_bytes());
        Ok(())
    }

    /// `drm_xe_exec`, with the checks in `xe_exec_ioctl` order: arguments, queue lookup,
    /// width, ban, then the work.
    fn exec(&mut self, arg: &[u8], hw: &mut impl GpuHw) -> Result<(), i32> {
        zeroed(arg, 0..8)?;
        zeroed(arg, 34..56)?;
        let num_syncs = u32_at(arg, 12) as usize;
        if num_syncs > MAX_EXEC_SYNCS {
            return Err(EINVAL);
        }
        let queue = u32_at(arg, 8);
        let ban = self
            .session
            .exec_queue_ban(queue, hw)
            .map_err(xe_to_errno)?;
        let num_batch_buffer = u16_at(arg, 32);
        // Every queue is width 1.
        if num_batch_buffer > 1 {
            return Err(EINVAL);
        }
        if ban != 0 {
            return Err(ECANCELED);
        }
        // In/out syncobjs are not wired to job completion yet; accepting them would leave
        // Mesa waiting forever on fences nothing signals.
        if num_syncs != 0 {
            return Err(EOPNOTSUPP);
        }
        // Zero batches only signal syncs; without syncs there is nothing to do.
        if num_batch_buffer == 0 {
            return Ok(());
        }
        self.session
            .exec(queue, u64_at(arg, 24), None, hw)
            .map(|_| ())
            .map_err(xe_to_errno)
    }
}

/// One `drm_xe_vm_bind_op`, with the per-op struct checks of `vm_bind_ioctl_check_args`.
fn decode_bind_op(op: &[u8]) -> Result<BindOp, i32> {
    zeroed(op, 0..8)?; // extensions
    zeroed(op, 14..16)?; // pad
    zeroed(op, 52..80)?; // pad2, reserved
    let kind = u32_at(op, 40);
    // `prefetch_region && op != PREFETCH`.
    if u32_at(op, 48) != 0 && kind != XE_VM_BIND_OP_PREFETCH {
        return Err(EINVAL);
    }
    Ok(BindOp {
        op: kind,
        flags: u32_at(op, 44),
        gem_handle: u32_at(op, 8),
        obj_offset: u64_at(op, 16),
        addr: u64_at(op, 32),
        range: u64_at(op, 24),
        pat_index: u16_at(op, 12),
    })
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
        XeError::Io => EIO,
    }
}

/// What a refused 438 means to the client: an unknown or dead lease names no memory,
/// a lease held for another driver is not this client's to use.
fn export_errno(err: ExportError) -> i32 {
    match err {
        ExportError::NotFound => ENOENT,
        ExportError::PermissionDenied => EPERM,
        ExportError::InvalidInput => EINVAL,
        ExportError::NotSupported => EOPNOTSUPP,
        ExportError::OutOfMemory => ENOMEM,
        ExportError::TryAgain | ExportError::Unconfirmed | ExportError::Malformed => EIO,
    }
}

#[cfg(test)]
#[path = "uapi/tests.rs"]
mod tests;
