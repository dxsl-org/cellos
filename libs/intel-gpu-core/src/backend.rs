//! Hardware-independent xe backend objects and session state: GEM buffer objects,
//! GPU VMs, Exec Queues, and submission lifecycle (ADR-0033, G1/G2).
//!
//! Upstream reference: `drivers/gpu/drm/xe/{xe_bo,xe_vm,xe_exec_queue,xe_exec}.c` (MIT)
//! and `include/uapi/drm/xe_drm.h`.
//!
//! Under CP1 D-G1 (Option B), a [`GemObject`] wraps a client-owned export lease
//! (`ExportDmaRange = 437`). All GPU mappings are tracked per-[`GpuVm`] and
//! strictly bound to the 48-bit address space. On client death or hang, mappings
//! are revoked before client frames are released, and pending fences are signaled
//! with error.

use crate::syncobj::{FenceId, Syncobjs};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

pub const PAGE_SIZE_4K: usize = 4096;
pub const MAX_VA: u64 = (1u64 << 48) - 1;
pub const MAX_EXEC_SYNCS: usize = 1024;

// Engine classes from xe_drm.h
pub const XE_ENGINE_CLASS_RENDER: u16 = 0;
pub const XE_ENGINE_CLASS_COPY: u16 = 1;
pub const XE_ENGINE_CLASS_VIDEO: u16 = 2;
pub const XE_ENGINE_CLASS_VIDEO_ENHANCE: u16 = 3;
pub const XE_ENGINE_CLASS_COMPUTE: u16 = 4;
pub const XE_ENGINE_CLASS_VM_BIND: u16 = 5;

// Exec queue priorities from xe_drm.h
pub const XE_EXEC_QUEUE_PRIORITY_LOW: u32 = 0;
pub const XE_EXEC_QUEUE_PRIORITY_NORMAL: u32 = 1;
pub const XE_EXEC_QUEUE_PRIORITY_HIGH: u32 = 2;

// Exec queue ban properties from xe_drm.h
pub const XE_EXEC_QUEUE_PROPERTY_BAN: u32 = 0;
pub const XE_EXEC_QUEUE_BAN_FLAG_HANG: u32 = 1 << 0;
pub const XE_EXEC_QUEUE_BAN_FLAG_BAD_FAULT: u32 = 1 << 1;

// VM bind operations from xe_drm.h
pub const XE_VM_BIND_OP_MAP: u32 = 0;
pub const XE_VM_BIND_OP_UNMAP: u32 = 1;
pub const XE_VM_BIND_OP_MAP_USERPTR: u32 = 2;
pub const XE_VM_BIND_OP_UNMAP_ALL: u32 = 3;
pub const XE_VM_BIND_OP_PREFETCH: u32 = 4;

// VM bind flags from xe_drm.h
pub const XE_VM_BIND_FLAG_READONLY: u32 = 1 << 0;
pub const XE_VM_BIND_FLAG_IMMEDIATE: u32 = 1 << 1;
pub const XE_VM_BIND_FLAG_NULL: u32 = 1 << 2;
pub const XE_VM_BIND_FLAG_DUMPABLE: u32 = 1 << 3;

// CPU caching from xe_drm.h
pub const XE_GEM_CPU_CACHING_WB: u16 = 1;
pub const XE_GEM_CPU_CACHING_WC: u16 = 2;

/// Standard xe uAPI errors mapped to their Linux errno values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XeError {
    /// `-EINVAL` (22): bad arguments, misaligned sizes, oversize bounds, unknown flag.
    Invalid,
    /// `-ENOENT` (2): object, VM, or queue not found.
    NoEntry,
    /// `-ENOMEM` (12): capacity or quota exceeded.
    OutOfMemory,
    /// `-ECANCELED` (125): queue is banned or operation canceled.
    Canceled,
    /// `-EPERM` (1): permission denied / priority too high.
    PermissionDenied,
    /// `-EOPNOTSUPP` (95): operation not supported.
    NotSupported,
    /// `-EAGAIN` (11): queue busy / try again.
    TryAgain,
}

/// A client GEM Buffer Object wrapping a DMA export lease (D-G1 Option B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GemObject {
    pub handle: u32,
    pub size: usize,
    pub export_id: u64,
    pub vm_id: u32,
    pub cpu_caching: u16,
    pub mmap_offset: u64,
}

/// An active virtual memory mapping in a GPU VM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmMapping {
    pub addr: u64,
    pub range: u64,
    pub gem_handle: u32,
    pub obj_offset: u64,
    pub pat_index: u16,
    pub flags: u32,
}

/// A per-client GPU address space (48-bit VA).
#[derive(Default)]
pub struct GpuVm {
    pub id: u32,
    pub flags: u32,
    pub mappings: Vec<VmMapping>,
}

impl GpuVm {
    pub fn new(id: u32, flags: u32) -> Self {
        Self {
            id,
            flags,
            mappings: Vec::new(),
        }
    }

    /// Check if a GPU virtual address is covered by an active mapping.
    pub fn is_mapped(&self, addr: u64, len: u64) -> bool {
        if len == 0 {
            return false;
        }
        let end = match addr.checked_add(len) {
            Some(e) => e,
            None => return false,
        };
        for m in &self.mappings {
            let m_end = m.addr.saturating_add(m.range);
            if addr >= m.addr && end <= m_end {
                return true;
            }
        }
        false
    }
}

/// An in-flight batch execution on an exec queue.
#[derive(Clone, Copy, Debug)]
pub struct InFlightJob {
    pub job_id: u64,
    pub batch_addr: u64,
    pub out_fence: Option<FenceId>,
}

/// An execution queue bound to an engine class and GPU VM.
pub struct ExecQueue {
    pub id: u32,
    pub vm_id: u32,
    pub engine_class: u16,
    pub priority: u32,
    pub banned: bool,
    pub ban_reason: u32,
    pub in_flight: Vec<InFlightJob>,
}

impl ExecQueue {
    pub fn new(id: u32, vm_id: u32, engine_class: u16, priority: u32) -> Self {
        Self {
            id,
            vm_id,
            engine_class,
            priority,
            banned: false,
            ban_reason: 0,
            in_flight: Vec::new(),
        }
    }
}

/// An operation descriptor for `VM_BIND`.
#[derive(Clone, Copy, Debug)]
pub struct BindOp {
    pub op: u32,
    pub flags: u32,
    pub gem_handle: u32,
    pub obj_offset: u64,
    pub addr: u64,
    pub range: u64,
    pub pat_index: u16,
}

/// A simulated xe backend session representing one client's connection.
pub struct XeSession {
    next_gem_handle: u32,
    next_vm_id: u32,
    next_queue_id: u32,
    next_job_id: u64,
    pub gem_objects: BTreeMap<u32, GemObject>,
    pub vms: BTreeMap<u32, GpuVm>,
    pub exec_queues: BTreeMap<u32, ExecQueue>,
}

// Not derived: ids start at 1 because 0 means "no object" in the uAPI (e.g. a NULL binding).
impl Default for XeSession {
    fn default() -> Self {
        Self::new()
    }
}

impl XeSession {
    pub fn new() -> Self {
        Self {
            next_gem_handle: 1,
            next_vm_id: 1,
            next_queue_id: 1,
            next_job_id: 1,
            gem_objects: BTreeMap::new(),
            vms: BTreeMap::new(),
            exec_queues: BTreeMap::new(),
        }
    }

    /// `DRM_IOCTL_XE_GEM_CREATE`
    pub fn gem_create(
        &mut self,
        size: usize,
        export_id: u64,
        vm_id: u32,
        cpu_caching: u16,
    ) -> Result<u32, XeError> {
        if size == 0 || !size.is_multiple_of(PAGE_SIZE_4K) {
            return Err(XeError::Invalid);
        }
        if vm_id != 0 && !self.vms.contains_key(&vm_id) {
            return Err(XeError::NoEntry);
        }
        if cpu_caching != XE_GEM_CPU_CACHING_WB && cpu_caching != XE_GEM_CPU_CACHING_WC {
            return Err(XeError::Invalid);
        }

        let handle = self.next_gem_handle;
        self.next_gem_handle = self
            .next_gem_handle
            .checked_add(1)
            .ok_or(XeError::OutOfMemory)?;

        let mmap_offset = (handle as u64) << 32;
        let bo = GemObject {
            handle,
            size,
            export_id,
            vm_id,
            cpu_caching,
            mmap_offset,
        };
        self.gem_objects.insert(handle, bo);
        Ok(handle)
    }

    /// `DRM_IOCTL_XE_GEM_MMAP_OFFSET`
    pub fn gem_mmap_offset(&self, handle: u32) -> Result<u64, XeError> {
        let bo = self.gem_objects.get(&handle).ok_or(XeError::NoEntry)?;
        Ok(bo.mmap_offset)
    }

    /// `DRM_IOCTL_GEM_CLOSE`
    ///
    /// Nothing outside the handle table keeps the object (or its export lease) alive here, so
    /// its mappings are revoked with the handle; otherwise the GPU could keep reaching a lease
    /// that [`Self::on_client_death`] no longer reports.
    pub fn gem_close(&mut self, handle: u32) -> Result<(), XeError> {
        self.gem_objects.remove(&handle).ok_or(XeError::NoEntry)?;
        for vm in self.vms.values_mut() {
            vm.mappings.retain(|m| m.gem_handle != handle);
        }
        Ok(())
    }

    /// `DRM_IOCTL_XE_VM_CREATE`
    pub fn vm_create(&mut self, flags: u32) -> Result<u32, XeError> {
        let vm_id = self.next_vm_id;
        self.next_vm_id = self.next_vm_id.checked_add(1).ok_or(XeError::OutOfMemory)?;

        let vm = GpuVm::new(vm_id, flags);
        self.vms.insert(vm_id, vm);
        Ok(vm_id)
    }

    /// `DRM_IOCTL_XE_VM_DESTROY`
    pub fn vm_destroy(&mut self, vm_id: u32) -> Result<(), XeError> {
        self.vms.remove(&vm_id).ok_or(XeError::NoEntry)?;
        Ok(())
    }

    /// `DRM_IOCTL_XE_VM_BIND`
    ///
    /// Every op is validated before any is applied, so a refused array leaves the VM unchanged.
    pub fn vm_bind(&mut self, vm_id: u32, ops: &[BindOp]) -> Result<(), XeError> {
        if !self.vms.contains_key(&vm_id) {
            return Err(XeError::NoEntry);
        }
        for op in ops {
            self.validate_bind_op(vm_id, op)?;
        }
        let vm = self.vms.get_mut(&vm_id).ok_or(XeError::NoEntry)?;
        for op in ops {
            match op.op {
                XE_VM_BIND_OP_MAP => {
                    punch_hole(&mut vm.mappings, op.addr, op.addr + op.range);
                    vm.mappings.push(VmMapping {
                        addr: op.addr,
                        range: op.range,
                        gem_handle: op.gem_handle,
                        obj_offset: op.obj_offset,
                        pat_index: op.pat_index,
                        flags: op.flags,
                    });
                }
                XE_VM_BIND_OP_UNMAP => punch_hole(&mut vm.mappings, op.addr, op.addr + op.range),
                _ => vm.mappings.retain(|m| m.gem_handle != op.gem_handle),
            }
        }
        Ok(())
    }

    /// Argument rules of `vm_bind_ioctl_check_args` in `xe_vm.c` for the supported ops.
    fn validate_bind_op(&self, vm_id: u32, op: &BindOp) -> Result<(), XeError> {
        const SUPPORTED_FLAGS: u32 = XE_VM_BIND_FLAG_READONLY
            | XE_VM_BIND_FLAG_IMMEDIATE
            | XE_VM_BIND_FLAG_NULL
            | XE_VM_BIND_FLAG_DUMPABLE;
        let is_null = op.flags & XE_VM_BIND_FLAG_NULL != 0;
        if op.flags & !SUPPORTED_FLAGS != 0 || (is_null && op.op != XE_VM_BIND_OP_MAP) {
            return Err(XeError::Invalid);
        }
        match op.op {
            XE_VM_BIND_OP_MAP | XE_VM_BIND_OP_UNMAP => {
                let page = PAGE_SIZE_4K as u64;
                if op.range == 0
                    || !op.range.is_multiple_of(page)
                    || !op.addr.is_multiple_of(page)
                    || !op.obj_offset.is_multiple_of(page)
                {
                    return Err(XeError::Invalid);
                }
                // `end` is exclusive: the last page below 2^48 is mappable.
                let end = op.addr.checked_add(op.range).ok_or(XeError::Invalid)?;
                if end - 1 > MAX_VA {
                    return Err(XeError::Invalid);
                }
                if is_null && (op.gem_handle != 0 || op.obj_offset != 0) {
                    return Err(XeError::Invalid);
                }
                if op.op == XE_VM_BIND_OP_UNMAP || is_null {
                    return Ok(());
                }
                let bo = self
                    .gem_objects
                    .get(&op.gem_handle)
                    .ok_or(XeError::NoEntry)?;
                if bo.vm_id != 0 && bo.vm_id != vm_id {
                    return Err(XeError::Invalid);
                }
                let bo_end = op
                    .obj_offset
                    .checked_add(op.range)
                    .ok_or(XeError::Invalid)?;
                if bo_end > bo.size as u64 {
                    return Err(XeError::Invalid);
                }
                Ok(())
            }
            XE_VM_BIND_OP_UNMAP_ALL => {
                if op.gem_handle == 0 || op.addr != 0 || op.range != 0 {
                    return Err(XeError::Invalid);
                }
                if !self.gem_objects.contains_key(&op.gem_handle) {
                    return Err(XeError::NoEntry);
                }
                Ok(())
            }
            _ => Err(XeError::NotSupported),
        }
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_CREATE`
    pub fn exec_queue_create(
        &mut self,
        vm_id: u32,
        engine_class: u16,
        priority: u32,
    ) -> Result<u32, XeError> {
        if !self.vms.contains_key(&vm_id) {
            return Err(XeError::NoEntry);
        }
        if engine_class > XE_ENGINE_CLASS_VM_BIND {
            return Err(XeError::Invalid);
        }
        if priority > XE_EXEC_QUEUE_PRIORITY_HIGH {
            return Err(XeError::Invalid);
        }

        let q_id = self.next_queue_id;
        self.next_queue_id = self
            .next_queue_id
            .checked_add(1)
            .ok_or(XeError::OutOfMemory)?;

        let q = ExecQueue::new(q_id, vm_id, engine_class, priority);
        self.exec_queues.insert(q_id, q);
        Ok(q_id)
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_DESTROY`
    pub fn exec_queue_destroy(&mut self, queue_id: u32) -> Result<(), XeError> {
        self.exec_queues.remove(&queue_id).ok_or(XeError::NoEntry)?;
        Ok(())
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY`
    pub fn exec_queue_get_property_ban(&self, queue_id: u32) -> Result<u32, XeError> {
        let q = self.exec_queues.get(&queue_id).ok_or(XeError::NoEntry)?;
        Ok(if q.banned { q.ban_reason } else { 0 })
    }

    /// `DRM_IOCTL_XE_EXEC`
    pub fn exec(
        &mut self,
        queue_id: u32,
        batch_addr: u64,
        num_syncs: usize,
        out_fence: Option<FenceId>,
    ) -> Result<u64, XeError> {
        if num_syncs > MAX_EXEC_SYNCS {
            return Err(XeError::Invalid);
        }

        let q = self
            .exec_queues
            .get_mut(&queue_id)
            .ok_or(XeError::NoEntry)?;
        if q.banned {
            return Err(XeError::Canceled);
        }

        // Address 0 is a valid VA, so it gets no exemption from the mapping check.
        let vm = self.vms.get(&q.vm_id).ok_or(XeError::NoEntry)?;
        if !vm.is_mapped(batch_addr, PAGE_SIZE_4K as u64) {
            return Err(XeError::Invalid);
        }

        let job_id = self.next_job_id;
        self.next_job_id = self
            .next_job_id
            .checked_add(1)
            .ok_or(XeError::OutOfMemory)?;

        q.in_flight.push(InFlightJob {
            job_id,
            batch_addr,
            out_fence,
        });

        Ok(job_id)
    }

    /// Simulate completing an in-flight job, signaling its out-fence.
    pub fn complete_job(
        &mut self,
        queue_id: u32,
        job_id: u64,
        syncobjs: &mut Syncobjs,
    ) -> Result<(), XeError> {
        let q = self
            .exec_queues
            .get_mut(&queue_id)
            .ok_or(XeError::NoEntry)?;
        let idx = q
            .in_flight
            .iter()
            .position(|j| j.job_id == job_id)
            .ok_or(XeError::NoEntry)?;

        let job = q.in_flight.remove(idx);
        if let Some(fence) = job.out_fence {
            syncobjs.signal_fence(fence);
        }
        Ok(())
    }

    /// Inject GPU hang: bans queue and signals all in-flight fences with completion/error.
    pub fn inject_hang(&mut self, queue_id: u32, syncobjs: &mut Syncobjs) -> Result<(), XeError> {
        let q = self
            .exec_queues
            .get_mut(&queue_id)
            .ok_or(XeError::NoEntry)?;
        q.banned = true;
        q.ban_reason = XE_EXEC_QUEUE_BAN_FLAG_HANG;

        for job in q.in_flight.drain(..) {
            if let Some(fence) = job.out_fence {
                // Signal fence so waiters wake up and detect the error state
                syncobjs.signal_fence(fence);
            }
        }
        Ok(())
    }

    /// Teardown on client exit / death (`WatchCellOwner` trigger).
    ///
    /// Revokes all active GPU mappings, bans queues, signals pending fences with error,
    /// and returns the list of revoked export IDs before client memory frames can be freed.
    pub fn on_client_death(&mut self, syncobjs: &mut Syncobjs) -> Vec<u64> {
        let mut revoked_exports = Vec::new();
        for bo in self.gem_objects.values() {
            if bo.export_id != 0 {
                revoked_exports.push(bo.export_id);
            }
        }

        // Cancel all queues and wake up pending syncobjs
        for q in self.exec_queues.values_mut() {
            q.banned = true;
            q.ban_reason = XE_EXEC_QUEUE_BAN_FLAG_HANG;
            for job in q.in_flight.drain(..) {
                if let Some(fence) = job.out_fence {
                    syncobjs.signal_fence(fence);
                }
            }
        }

        // Unmap all GPU VM mappings immediately
        for vm in self.vms.values_mut() {
            vm.mappings.clear();
        }

        self.gem_objects.clear();
        revoked_exports
    }
}

/// Remove `[start, end)` from `mappings`, trimming or splitting partially covered ones the way
/// GPUVA remap does, so a partial unmap never leaves the covered pages reachable.
fn punch_hole(mappings: &mut Vec<VmMapping>, start: u64, end: u64) {
    let mut i = 0;
    while i < mappings.len() {
        let m = &mut mappings[i];
        let m_end = m.addr + m.range;
        if m_end <= start || m.addr >= end {
            i += 1;
        } else if m.addr < start {
            let tail = (m_end > end).then(|| VmMapping {
                addr: end,
                range: m_end - end,
                obj_offset: m.obj_offset + (end - m.addr),
                ..m.clone()
            });
            m.range = start - m.addr;
            // A tail lies past `end`, so the scan skips it when it reaches it.
            mappings.extend(tail);
            i += 1;
        } else if m_end > end {
            m.obj_offset += end - m.addr;
            m.range = m_end - end;
            m.addr = end;
            i += 1;
        } else {
            mappings.swap_remove(i);
        }
    }
}

#[cfg(test)]
#[path = "backend/tests.rs"]
mod tests;
