//! Hardware-independent xe backend objects and session state: GEM buffer objects,
//! GPU VMs, exec queues, and submission lifecycle (ADR-0033, G1/G2).
//!
//! Upstream reference: `drivers/gpu/drm/xe/{xe_bo,xe_vm,xe_exec_queue,xe_exec}.c` (MIT)
//! and `include/uapi/drm/xe_drm.h`.
//!
//! Under CP1 D-G1 (option B) a [`GemObject`] is client memory the client exported to this
//! driver (`ExportDmaRange`, 437) and the driver mapped into its device domain
//! (`MapDmaExport`, 438): the object is the export's IOVA window. Every device effect goes
//! through a [`GpuHw`]. Teardown order is fixed: stop the contexts, clear the PPGTT leaves
//! (with TLB invalidation), and only then revoke the export (439), so the GPU never holds
//! a translation to a window the kernel is releasing.

use crate::hw::{ExportRecord, GpuHw, HwQueue, HwVm};
use crate::pte::PatIndex;
use crate::syncobj::{FenceId, Syncobjs};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

pub const PAGE_SIZE_4K: usize = 4096;
pub const MAX_VA: u64 = (1u64 << 48) - 1;
/// `DRM_XE_MAX_SYNCS`.
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
    /// `-EIO` (5): the device did not do what it was asked.
    Io,
}

/// A client buffer object: an export lease mapped into the driver's device domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GemObject {
    pub handle: u32,
    /// Bytes the uAPI object covers (`drm_xe_gem_create.size`), at most the window length.
    pub size: usize,
    /// The lease this object is; revoked as holder when the object goes away.
    pub export_id: u64,
    /// Device address of byte 0 (the export's IOVA window base).
    pub iova: u64,
    /// The remapper lets the GPU write the window. A device-read-only export forces every
    /// mapping of the object read-only in the PPGTT too.
    pub writable: bool,
    pub vm_id: u32,
    pub cpu_caching: u16,
    pub mmap_offset: u64,
}

/// An active virtual memory mapping in a GPU VM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmMapping {
    pub addr: u64,
    pub range: u64,
    /// 0 for a `NULL` mapping.
    pub gem_handle: u32,
    pub obj_offset: u64,
    pub pat_index: u16,
    pub flags: u32,
    /// Whether the PPGTT leaves carry the write bit.
    pub writable: bool,
}

/// A per-client GPU address space (48-bit VA).
pub struct GpuVm {
    pub id: u32,
    pub flags: u32,
    pub hw: HwVm,
    pub mappings: Vec<VmMapping>,
    /// `VM_DESTROY` ran while a queue still pointed its context at this PPGTT: the leaves
    /// are gone and the VM is invisible to the uAPI; the tables go with the last queue.
    pub closed: bool,
}

impl GpuVm {
    /// Check if a GPU virtual address is covered by an active mapping.
    pub fn is_mapped(&self, addr: u64, len: u64) -> bool {
        if len == 0 {
            return false;
        }
        let Some(end) = addr.checked_add(len) else {
            return false;
        };
        self.mappings
            .iter()
            .any(|m| addr >= m.addr && end <= m.addr.saturating_add(m.range))
    }

    fn overlaps(&self, start: u64, end: u64) -> bool {
        self.mappings
            .iter()
            .any(|m| m.addr < end && start < m.addr + m.range)
    }
}

/// An in-flight batch execution on an exec queue that a fence waits on.
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
    pub hw: HwQueue,
    pub banned: bool,
    pub ban_reason: u32,
    pub in_flight: Vec<InFlightJob>,
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

/// One client's open DRM file: its objects, VMs and queues.
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

    fn open_vm(&self, vm_id: u32) -> Result<&GpuVm, XeError> {
        self.vms
            .get(&vm_id)
            .filter(|vm| !vm.closed)
            .ok_or(XeError::NoEntry)
    }

    /// `DRM_IOCTL_XE_GEM_CREATE` over a mapped export. The caller has already checked that
    /// the export belongs to the requesting client.
    pub fn gem_create(
        &mut self,
        size: usize,
        export: &ExportRecord,
        vm_id: u32,
        cpu_caching: u16,
    ) -> Result<u32, XeError> {
        if size == 0 || !size.is_multiple_of(PAGE_SIZE_4K) {
            return Err(XeError::Invalid);
        }
        if vm_id != 0 {
            self.open_vm(vm_id)?;
        }
        if cpu_caching != XE_GEM_CPU_CACHING_WB && cpu_caching != XE_GEM_CPU_CACHING_WC {
            return Err(XeError::Invalid);
        }
        // The window must back every byte of the object, and one lease backs one object:
        // a second object over it would be revoked twice and outlive the first close.
        if export.len < size as u64
            || !export.iova.is_multiple_of(PAGE_SIZE_4K as u64)
            || self
                .gem_objects
                .values()
                .any(|bo| bo.export_id == export.export_id)
        {
            return Err(XeError::Invalid);
        }

        let handle = self.next_gem_handle;
        self.next_gem_handle = handle.checked_add(1).ok_or(XeError::OutOfMemory)?;
        self.gem_objects.insert(
            handle,
            GemObject {
                handle,
                size,
                export_id: export.export_id,
                iova: export.iova,
                writable: export.device_write,
                vm_id,
                cpu_caching,
                mmap_offset: u64::from(handle) << 32,
            },
        );
        Ok(handle)
    }

    /// `DRM_IOCTL_XE_GEM_MMAP_OFFSET`
    pub fn gem_mmap_offset(&self, handle: u32) -> Result<u64, XeError> {
        let bo = self.gem_objects.get(&handle).ok_or(XeError::NoEntry)?;
        Ok(bo.mmap_offset)
    }

    /// `DRM_IOCTL_GEM_CLOSE`: clear every leaf of the object, then revoke its export.
    ///
    /// Nothing outside the handle table keeps the object alive here (the client's CPU
    /// mappings are its own pages), so the GPU loses the object with the handle.
    pub fn gem_close(&mut self, handle: u32, hw: &mut impl GpuHw) -> Result<(), XeError> {
        let bo = self.gem_objects.remove(&handle).ok_or(XeError::NoEntry)?;
        for vm in self.vms.values_mut() {
            let (gone, kept): (Vec<_>, Vec<_>) =
                vm.mappings.drain(..).partition(|m| m.gem_handle == handle);
            vm.mappings = kept;
            for m in gone {
                hw.vm_unmap(vm.hw, m.addr, m.range);
            }
        }
        // The kernel finishes an unacknowledged drain on its own (`TryAgain`), and an
        // owner that already revoked leaves nothing to do (`NotFound`).
        let _ = hw.revoke_export(bo.export_id);
        Ok(())
    }

    /// `DRM_IOCTL_XE_VM_CREATE`
    pub fn vm_create(&mut self, flags: u32, hw: &mut impl GpuHw) -> Result<u32, XeError> {
        let vm_id = self.next_vm_id;
        let next = vm_id.checked_add(1).ok_or(XeError::OutOfMemory)?;
        let vm_hw = hw.vm_create()?;
        self.next_vm_id = next;
        self.vms.insert(
            vm_id,
            GpuVm {
                id: vm_id,
                flags,
                hw: vm_hw,
                mappings: Vec::new(),
                closed: false,
            },
        );
        Ok(vm_id)
    }

    /// `DRM_IOCTL_XE_VM_DESTROY`: every mapping is removed now; the page tables follow as
    /// soon as no exec queue's context points at them.
    pub fn vm_destroy(&mut self, vm_id: u32, hw: &mut impl GpuHw) -> Result<(), XeError> {
        self.open_vm(vm_id)?;
        let in_use = self.exec_queues.values().any(|q| q.vm_id == vm_id);
        let vm = self.vms.get_mut(&vm_id).ok_or(XeError::NoEntry)?;
        for m in vm.mappings.drain(..) {
            hw.vm_unmap(vm.hw, m.addr, m.range);
        }
        if in_use {
            vm.closed = true;
        } else if let Some(vm) = self.vms.remove(&vm_id) {
            hw.vm_destroy(vm.hw);
        }
        Ok(())
    }

    /// `DRM_IOCTL_XE_VM_BIND`
    ///
    /// Every op is validated before any is applied, so a refused array leaves the VM
    /// unchanged. A device failure while applying (no table memory) stops at that op; the
    /// ops before it stay bound, as their leaves are already live.
    pub fn vm_bind(
        &mut self,
        vm_id: u32,
        ops: &[BindOp],
        hw: &mut impl GpuHw,
    ) -> Result<(), XeError> {
        self.open_vm(vm_id)?;
        for op in ops {
            self.validate_bind_op(vm_id, op)?;
        }
        for op in ops {
            self.apply_bind_op(vm_id, op, hw)?;
        }
        Ok(())
    }

    fn apply_bind_op(
        &mut self,
        vm_id: u32,
        op: &BindOp,
        hw: &mut impl GpuHw,
    ) -> Result<(), XeError> {
        let bo_writable = self
            .gem_objects
            .get(&op.gem_handle)
            .map(|bo| (bo.iova, bo.writable));
        let vm = self.vms.get_mut(&vm_id).ok_or(XeError::NoEntry)?;
        match op.op {
            XE_VM_BIND_OP_MAP => {
                let end = op.addr + op.range;
                if vm.overlaps(op.addr, end) {
                    hw.vm_unmap(vm.hw, op.addr, op.range);
                    punch_hole(&mut vm.mappings, op.addr, end);
                }
                let pat = PatIndex::new(op.pat_index as u8).ok_or(XeError::Invalid)?;
                let writable = if op.flags & XE_VM_BIND_FLAG_NULL != 0 {
                    hw.vm_map_null(vm.hw, op.addr, op.range, pat)?;
                    true
                } else {
                    let (iova, bo_writable) = bo_writable.ok_or(XeError::NoEntry)?;
                    // READONLY is honoured, and forced when the remapper itself would
                    // refuse the GPU's writes: a write through a read-only window would
                    // fault at VT-d instead of failing in the PPGTT.
                    let writable = op.flags & XE_VM_BIND_FLAG_READONLY == 0 && bo_writable;
                    hw.vm_map(
                        vm.hw,
                        op.addr,
                        iova + op.obj_offset,
                        op.range,
                        pat,
                        writable,
                    )?;
                    writable
                };
                vm.mappings.push(VmMapping {
                    addr: op.addr,
                    range: op.range,
                    gem_handle: op.gem_handle,
                    obj_offset: op.obj_offset,
                    pat_index: op.pat_index,
                    flags: op.flags,
                    writable,
                });
            }
            XE_VM_BIND_OP_UNMAP => {
                let end = op.addr + op.range;
                if vm.overlaps(op.addr, end) {
                    hw.vm_unmap(vm.hw, op.addr, op.range);
                    punch_hole(&mut vm.mappings, op.addr, end);
                }
            }
            _ => {
                let (gone, kept): (Vec<_>, Vec<_>) = vm
                    .mappings
                    .drain(..)
                    .partition(|m| m.gem_handle == op.gem_handle);
                vm.mappings = kept;
                for m in gone {
                    hw.vm_unmap(vm.hw, m.addr, m.range);
                }
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
        // `pat_index >= xe->pat.n_entries` (32 on graphics 20+).
        if u8::try_from(op.pat_index)
            .ok()
            .and_then(PatIndex::new)
            .is_none()
        {
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
                // UNMAP and NULL name no object (`obj && op == UNMAP`, `obj && is_null`).
                if (is_null || op.op == XE_VM_BIND_OP_UNMAP)
                    && (op.gem_handle != 0 || op.obj_offset != 0)
                {
                    return Err(XeError::Invalid);
                }
                if op.op == XE_VM_BIND_OP_UNMAP || is_null {
                    return Ok(());
                }
                if op.gem_handle == 0 {
                    return Err(XeError::Invalid);
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
            // `MAP_USERPTR` needs `CONFIG_DRM_GPUSVM` upstream and `PREFETCH` a placement
            // to migrate to; neither exists here, and `xe` refuses such ops with `-EINVAL`
            // in `vm_bind_ioctl_check_args`.
            _ => Err(XeError::Invalid),
        }
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_CREATE`: a GuC context of `engine_class` in `vm_id`.
    pub fn exec_queue_create(
        &mut self,
        vm_id: u32,
        engine_class: u16,
        priority: u32,
        hw: &mut impl GpuHw,
    ) -> Result<u32, XeError> {
        let vm_hw = self.open_vm(vm_id)?.hw;
        if engine_class > XE_ENGINE_CLASS_VM_BIND || priority > XE_EXEC_QUEUE_PRIORITY_HIGH {
            return Err(XeError::Invalid);
        }
        let q_id = self.next_queue_id;
        let next = q_id.checked_add(1).ok_or(XeError::OutOfMemory)?;
        let queue_hw = hw.queue_create(vm_hw, engine_class)?;
        self.next_queue_id = next;
        self.exec_queues.insert(
            q_id,
            ExecQueue {
                id: q_id,
                vm_id,
                engine_class,
                priority,
                hw: queue_hw,
                banned: false,
                ban_reason: 0,
                in_flight: Vec::new(),
            },
        );
        Ok(q_id)
    }

    /// `DRM_IOCTL_XE_EXEC_QUEUE_DESTROY`: the context is stopped and deregistered, waiters
    /// on its jobs are woken, and a closed VM it kept alive is freed.
    pub fn exec_queue_destroy(
        &mut self,
        queue_id: u32,
        syncobjs: &mut Syncobjs,
        hw: &mut impl GpuHw,
    ) -> Result<(), XeError> {
        let q = self.exec_queues.remove(&queue_id).ok_or(XeError::NoEntry)?;
        hw.queue_destroy(q.hw);
        for job in q.in_flight {
            if let Some(fence) = job.out_fence {
                syncobjs.signal_fence(fence);
            }
        }
        let vm_unused = !self.exec_queues.values().any(|o| o.vm_id == q.vm_id);
        if vm_unused && self.vms.get(&q.vm_id).is_some_and(|vm| vm.closed) {
            if let Some(vm) = self.vms.remove(&q.vm_id) {
                hw.vm_destroy(vm.hw);
            }
        }
        Ok(())
    }

    /// Fold the bans the device reported for `queue_id` in; `Ok(reason)`, 0 if not banned.
    pub fn exec_queue_ban(&mut self, queue_id: u32, hw: &mut impl GpuHw) -> Result<u32, XeError> {
        let q = self
            .exec_queues
            .get_mut(&queue_id)
            .ok_or(XeError::NoEntry)?;
        let reported = hw.queue_ban(q.hw);
        if reported != 0 {
            q.banned = true;
            q.ban_reason |= reported;
        }
        Ok(if q.banned { q.ban_reason } else { 0 })
    }

    /// `DRM_IOCTL_XE_EXEC` of one batch at `batch_addr`, optionally signalling `out_fence`
    /// when [`Self::complete_job`] retires it.
    pub fn exec(
        &mut self,
        queue_id: u32,
        batch_addr: u64,
        out_fence: Option<FenceId>,
        hw: &mut impl GpuHw,
    ) -> Result<u64, XeError> {
        if self.exec_queue_ban(queue_id, hw)? != 0 {
            return Err(XeError::Canceled);
        }
        let q = self.exec_queues.get(&queue_id).ok_or(XeError::NoEntry)?;
        // Address 0 is a valid VA, so it gets no exemption from the mapping check.
        let vm = self.vms.get(&q.vm_id).ok_or(XeError::NoEntry)?;
        if !vm.is_mapped(batch_addr, PAGE_SIZE_4K as u64) {
            return Err(XeError::Invalid);
        }
        let job_id = self.next_job_id;
        let next = job_id.checked_add(1).ok_or(XeError::OutOfMemory)?;
        hw.queue_submit(q.hw, batch_addr)?;
        self.next_job_id = next;
        // Only a job somebody waits on needs remembering; the ring itself is the queue.
        if out_fence.is_some() {
            if let Some(q) = self.exec_queues.get_mut(&queue_id) {
                q.in_flight.push(InFlightJob {
                    job_id,
                    batch_addr,
                    out_fence,
                });
            }
        }
        Ok(job_id)
    }

    /// Retire job `job_id` of `queue_id`, signalling its out-fence.
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

    /// A hang on `queue_id` (GuC context reset): ban the queue and wake every waiter.
    pub fn inject_hang(&mut self, queue_id: u32, syncobjs: &mut Syncobjs) -> Result<(), XeError> {
        let q = self
            .exec_queues
            .get_mut(&queue_id)
            .ok_or(XeError::NoEntry)?;
        q.banned = true;
        q.ban_reason |= XE_EXEC_QUEUE_BAN_FLAG_HANG;
        for job in q.in_flight.drain(..) {
            if let Some(fence) = job.out_fence {
                syncobjs.signal_fence(fence);
            }
        }
        Ok(())
    }

    /// `postclose` / client death (`WatchCellOwner`): stop every context, clear every PPGTT
    /// leaf and free the tables, and only then revoke each export this file held. Returns
    /// the revoked export ids, in revocation order.
    pub fn on_client_death(&mut self, syncobjs: &mut Syncobjs, hw: &mut impl GpuHw) -> Vec<u64> {
        for (_, q) in core::mem::take(&mut self.exec_queues) {
            hw.queue_destroy(q.hw);
            for job in q.in_flight {
                if let Some(fence) = job.out_fence {
                    syncobjs.signal_fence(fence);
                }
            }
        }
        for (_, vm) in core::mem::take(&mut self.vms) {
            for m in &vm.mappings {
                hw.vm_unmap(vm.hw, m.addr, m.range);
            }
            hw.vm_destroy(vm.hw);
        }
        let mut revoked = Vec::new();
        for (_, bo) in core::mem::take(&mut self.gem_objects) {
            let _ = hw.revoke_export(bo.export_id);
            revoked.push(bo.export_id);
        }
        revoked
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
