//! Host unit tests for the Mesa `anv` DRM transport shim, written against the uapi struct
//! layouts and full request codes rather than the shim's own decoding. Every request
//! crosses the real `XE_GPU` frames into the driver's request server.

use crate::memory::{ClientBuffer, ClientMemory, RevokeError};
use crate::shim::DrmShim;
use crate::transport::Transport;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use intel_gpu_core::backend::XeError;
use intel_gpu_core::hw::{ExportError, ExportMapper, ExportRecord, GpuHw, HwQueue, HwVm};
use intel_gpu_core::pte::PatIndex;
use intel_gpu_core::server::{Caller, XeServer};
use intel_gpu_core::uapi::ioctl::*;
use intel_gpu_core::uapi::query::*;
use intel_gpu_core::uapi::UserMemory;

/// The device the loopback server describes: one Xe3 GT.
const DEVICE: DeviceInfo = DeviceInfo {
    name: "Intel Xe3 (PTL/WCL)",
    ip_ver_major: 30,
    ip_ver_minor: 0,
    ip_ver_rev: 0,
    reference_clock_hz: 24_000_000,
    dss_geometry: 0x0FFF,
    dss_compute: 0x0FFF,
    eu_per_dss: 0xFF,
    sysmem_bytes: 1 << 38,
};

const CELL: u64 = 9;
const GENERATION: u64 = 1;

/// The kernel's export table, shared by the client's allocator and the driver's mapper.
#[derive(Default)]
struct Kernel {
    leases: BTreeMap<u64, ExportRecord>,
    next_id: u64,
    /// Revokes answer "drain pending" while set.
    drain_pending: bool,
    revoked: Vec<u64>,
    /// Live leaf mappings the driver wrote, by window address.
    leaves: BTreeMap<u64, u64>,
}

/// Client memory from a byte arena; exports land in the shared [`Kernel`].
struct FakeMemory {
    kernel: Rc<RefCell<Kernel>>,
    next_addr: u64,
    live: BTreeMap<usize, ClientBuffer>,
    /// Allocations beyond this many bytes fail, like a Cell that cannot back them.
    limit: usize,
    /// Exports are refused, as for a Tier-2 private-root Cell.
    refuse_export: bool,
}

impl ClientMemory for FakeMemory {
    fn alloc(&mut self, size: usize) -> Option<ClientBuffer> {
        if size > self.limit {
            return None;
        }
        let buf = ClientBuffer {
            id: self.next_addr as usize,
            addr: self.next_addr,
            size,
        };
        self.next_addr += size as u64 + 4096;
        self.live.insert(buf.id, buf);
        Some(buf)
    }

    fn export(&mut self, buf: &ClientBuffer, device_write: bool) -> Option<u64> {
        if self.refuse_export {
            return None;
        }
        let mut k = self.kernel.borrow_mut();
        k.next_id += 1;
        let id = k.next_id;
        k.leases.insert(
            id,
            ExportRecord {
                export_id: id,
                owner_cell_id: CELL,
                owner_generation: GENERATION,
                iova: (1 << 38) + (id << 24),
                len: buf.size as u64,
                device_write,
            },
        );
        Some(id)
    }

    fn revoke(&mut self, export_id: u64) -> Result<(), RevokeError> {
        let mut k = self.kernel.borrow_mut();
        if k.drain_pending {
            return Err(RevokeError::Pending);
        }
        k.leases.remove(&export_id);
        k.revoked.push(export_id);
        Ok(())
    }

    fn free(&mut self, buf: ClientBuffer) {
        assert!(self.live.remove(&buf.id).is_some(), "double free");
    }
}

/// The driver's device half, recording only what the shim tests look at.
struct FakeHw(Rc<RefCell<Kernel>>);

impl ExportMapper for FakeHw {
    fn map_export(&mut self, export_id: u64) -> Result<ExportRecord, ExportError> {
        self.0
            .borrow()
            .leases
            .get(&export_id)
            .copied()
            .ok_or(ExportError::NotFound)
    }
    fn revoke_export(&mut self, export_id: u64) -> Result<(), ExportError> {
        let mut k = self.0.borrow_mut();
        let record = k
            .leases
            .get(&export_id)
            .copied()
            .ok_or(ExportError::NotFound)?;
        // The driver revokes only after clearing every leaf into the window.
        assert!(!k
            .leaves
            .keys()
            .any(|&a| a >= record.iova && a < record.iova + record.len));
        k.leases.remove(&export_id);
        k.revoked.push(export_id);
        Ok(())
    }
}

impl GpuHw for FakeHw {
    fn vm_create(&mut self) -> Result<HwVm, XeError> {
        Ok(HwVm(1))
    }
    fn vm_map(
        &mut self,
        _: HwVm,
        va: u64,
        dma: u64,
        len: u64,
        _: PatIndex,
        _: bool,
    ) -> Result<(), XeError> {
        for page in (0..len).step_by(4096) {
            self.0.borrow_mut().leaves.insert(dma + page, va + page);
        }
        Ok(())
    }
    fn vm_map_null(&mut self, _: HwVm, _: u64, _: u64, _: PatIndex) -> Result<(), XeError> {
        Ok(())
    }
    fn vm_unmap(&mut self, _: HwVm, va: u64, len: u64) {
        self.0
            .borrow_mut()
            .leaves
            .retain(|_, v| *v < va || *v >= va + len);
    }
    fn vm_destroy(&mut self, _: HwVm) {}
    fn queue_create(&mut self, _: HwVm, _: u16) -> Result<HwQueue, XeError> {
        Ok(HwQueue(1))
    }
    fn queue_submit(&mut self, _: HwQueue, _: u64) -> Result<(), XeError> {
        Ok(())
    }
    fn queue_ban(&mut self, _: HwQueue) -> u32 {
        0
    }
    fn queue_destroy(&mut self, _: HwQueue) {}
}

/// The driver's request server in-process, answering as one client tid.
struct Loopback(XeServer, FakeHw);

impl Transport for Loopback {
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, i32> {
        let caller = Caller {
            tid: 42,
            cell_id: CELL,
            generation: GENERATION,
        };
        Ok(self.0.handle(caller, request, reply, &mut self.1))
    }
}

type Shim = DrmShim<Loopback, FakeMemory>;

fn shim() -> Shim {
    let kernel = Rc::new(RefCell::new(Kernel::default()));
    let memory = FakeMemory {
        kernel: kernel.clone(),
        next_addr: 0x8000_0000,
        live: BTreeMap::new(),
        limit: 1 << 24,
        refuse_export: false,
    };
    DrmShim::new(Loopback(XeServer::new(DEVICE), FakeHw(kernel)), memory)
}

fn kernel(shim: &mut Shim) -> Rc<RefCell<Kernel>> {
    shim.memory_mut().kernel.clone()
}

/// User memory backed by one buffer starting at `BASE`.
struct FakeMem(Vec<u8>);

const BASE: u64 = 0x10_0000;

impl FakeMem {
    fn range(&self, addr: u64, len: usize) -> Result<core::ops::Range<usize>, i32> {
        let start = addr.checked_sub(BASE).ok_or(EFAULT)? as usize;
        let end = start.checked_add(len).ok_or(EFAULT)?;
        if end > self.0.len() {
            return Err(EFAULT);
        }
        Ok(start..end)
    }
}

impl UserMemory for FakeMem {
    fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), i32> {
        out.copy_from_slice(&self.0[self.range(addr, out.len())?]);
        Ok(())
    }

    fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), i32> {
        let range = self.range(addr, data.len())?;
        self.0[range].copy_from_slice(data);
        Ok(())
    }
}

fn arg(request: u32) -> Vec<u8> {
    vec![0u8; ioctl_size(request)]
}

fn put(buf: &mut [u8], offset: usize, bytes: &[u8]) {
    buf[offset..offset + bytes.len()].copy_from_slice(bytes);
}

fn u32_at(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(buf[offset..offset + 4].try_into().unwrap())
}

fn u64_at(buf: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap())
}

fn open() -> (Shim, i32, FakeMem) {
    let mut shim = shim();
    let fd = shim.open("/dev/dri/renderD128").unwrap();
    (shim, fd, FakeMem(vec![0u8; 4096]))
}

fn gem_create(shim: &mut Shim, fd: i32, mem: &mut FakeMem, size: u64) -> Result<u32, i32> {
    let mut create = arg(DRM_IOCTL_XE_GEM_CREATE);
    put(&mut create, 8, &size.to_le_bytes());
    put(&mut create, 16, &1u32.to_le_bytes()); // placement: sysmem
    put(&mut create, 32, &2u16.to_le_bytes()); // DRM_XE_GEM_CPU_CACHING_WC
    shim.ioctl(fd, DRM_IOCTL_XE_GEM_CREATE, &mut create, mem)
        .map(|()| u32_at(&create, 28))
}

#[test]
fn request_codes_match_linux_headers() {
    // Values libdrm and the kernel compute for these `_IOWR`/`_IOW` definitions.
    assert_eq!(DRM_IOCTL_VERSION, 0xC040_6400);
    assert_eq!(DRM_IOCTL_GET_CAP, 0xC010_640C);
    assert_eq!(DRM_IOCTL_GEM_CLOSE, 0x4008_6409);
    assert_eq!(DRM_IOCTL_SYNCOBJ_CREATE, 0xC008_64BF);
    assert_eq!(DRM_IOCTL_XE_DEVICE_QUERY, 0xC028_6440);
    assert_eq!(DRM_IOCTL_XE_GEM_CREATE, 0xC038_6441);
    assert_eq!(DRM_IOCTL_XE_EXEC, 0x4038_6449);
    assert_eq!(ioctl_type(DRM_IOCTL_XE_EXEC), DRM_IOCTL_BASE);
    assert_eq!(ioctl_dir(DRM_IOCTL_XE_EXEC), IOC_WRITE);
}

#[test]
fn shim_open_and_close() {
    let mut shim = shim();
    let fd = shim.open("/dev/dri/renderD128").unwrap();
    assert!(fd >= 10);

    // Invalid path rejected with -ENOENT
    assert_eq!(shim.open("/dev/dri/card0"), Err(ENOENT));

    // Close releases the driver file; repeated close returns -EBADF
    assert_eq!(shim.close(fd), Ok(()));
    assert_eq!(shim.transport_mut().0.client_count(), 0);
    assert_eq!(shim.close(fd), Err(EBADF));
}

#[test]
fn version_reports_xe_name_through_user_buffer() {
    let (mut shim, fd, mut mem) = open();
    let mut a = arg(DRM_IOCTL_VERSION);
    // First pass: lengths only, as libdrm's drmGetVersion does.
    assert_eq!(shim.ioctl(fd, DRM_IOCTL_VERSION, &mut a, &mut mem), Ok(()));
    assert_eq!(u32_at(&a, 0), 1);
    assert_eq!(u64_at(&a, 16), 2);

    put(&mut a, 24, &BASE.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_IOCTL_VERSION, &mut a, &mut mem), Ok(()));
    assert_eq!(&mem.0[..2], b"xe");
}

#[test]
fn get_cap_timeline() {
    let (mut shim, fd, mut mem) = open();
    let mut a = arg(DRM_IOCTL_GET_CAP);
    put(&mut a, 0, &DRM_CAP_SYNCOBJ_TIMELINE.to_le_bytes());
    assert_eq!(shim.ioctl(fd, DRM_IOCTL_GET_CAP, &mut a, &mut mem), Ok(()));
    assert_eq!(u64_at(&a, 8), 1);
}

#[test]
fn device_query_sizes_then_copies_uapi_payload() {
    let (mut shim, fd, mut mem) = open();
    let mut a = arg(DRM_IOCTL_XE_DEVICE_QUERY);
    put(&mut a, 8, &DRM_XE_DEVICE_QUERY_CONFIG.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_DEVICE_QUERY, &mut a, &mut mem),
        Ok(())
    );
    assert_eq!(u32_at(&a, 12), 8 + 5 * 8); // drm_xe_query_config with 5 params

    // A size that is not the exact payload size is refused.
    put(&mut a, 12, &8u32.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_DEVICE_QUERY, &mut a, &mut mem),
        Err(EINVAL)
    );

    put(&mut a, 12, &48u32.to_le_bytes());
    put(&mut a, 16, &BASE.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_DEVICE_QUERY, &mut a, &mut mem),
        Ok(())
    );
    assert_eq!(u32_at(&mem.0, 0), 5); // num_params
    assert_eq!(u64_at(&mem.0, 8 + 3 * 8), 48); // info[DRM_XE_QUERY_CONFIG_VA_BITS]
}

#[test]
fn topology_entries_use_uapi_type_ids() {
    let payload = query_payload(&DEVICE, DRM_XE_DEVICE_QUERY_GT_TOPOLOGY).unwrap();
    assert_eq!(u16::from_le_bytes([payload[2], payload[3]]), 1); // DSS_GEOMETRY
                                                                 // Packed entries: 16-byte DSS masks (`xe_dss_mask_t`), then the 8-byte EU mask.
    assert_eq!(payload.len(), 3 * 8 + 16 + 16 + 8);
    assert_eq!(u16::from_le_bytes([payload[26], payload[27]]), 2); // DSS_COMPUTE
    assert_eq!(
        query_payload(&DEVICE, DRM_XE_DEVICE_QUERY_ENGINES)
            .unwrap()
            .len(),
        8 + 32
    );
    let gt = query_payload(&DEVICE, DRM_XE_DEVICE_QUERY_GT_LIST).unwrap();
    assert_eq!(gt.len(), 8 + 96);
    assert_eq!(u32_at(&gt, 8 + 12), 24_000_000); // reference_clock
    assert_eq!(u16::from_le_bytes([gt[8 + 32], gt[8 + 33]]), 30); // ip_ver_major
}

#[test]
fn gem_create_exports_local_memory_and_mmap_resolves_to_it() {
    let (mut shim, fd, mut mem) = open();
    let handle = gem_create(&mut shim, fd, &mut mem, 8192).unwrap();
    assert_eq!(handle, 1);
    let k = kernel(&mut shim);
    assert_eq!(k.borrow().leases.len(), 1);
    assert!(k.borrow().leases[&1].device_write);

    let mut mmap = arg(DRM_IOCTL_XE_GEM_MMAP_OFFSET);
    put(&mut mmap, 8, &handle.to_le_bytes());
    shim.ioctl(fd, DRM_IOCTL_XE_GEM_MMAP_OFFSET, &mut mmap, &mut mem)
        .unwrap();
    let offset = u64_at(&mmap, 16);
    assert_eq!(offset, 1u64 << 32);
    // The object's own pages, at any offset inside it; nothing past it.
    let addr = shim.mmap(fd, offset, 8192).unwrap();
    assert_eq!(addr, shim.memory_mut().live.values().next().unwrap().addr);
    assert_eq!(shim.mmap(fd, offset + 4096, 4096), Ok(addr + 4096));
    assert_eq!(shim.mmap(fd, offset + 4096, 8192), Err(EINVAL));
    assert_eq!(shim.mmap(fd, 7 << 32, 4096), Err(EINVAL));

    // GEM_CLOSE: the driver drops the object, the export is revoked, but the memory stays
    // while a CPU mapping holds it (as a VMA keeps a GEM object alive).
    let mut close = arg(DRM_IOCTL_GEM_CLOSE);
    put(&mut close, 0, &handle.to_le_bytes());
    shim.ioctl(fd, DRM_IOCTL_GEM_CLOSE, &mut close, &mut mem)
        .unwrap();
    assert!(k.borrow().leases.is_empty());
    // The driver revoked as holder after unbinding, then the shim as owner (idempotent).
    assert_eq!(k.borrow().revoked, [1, 1]);
    assert_eq!(shim.orphan_count(), 1);
    // Both maps name the object's pages; unmapping the head leaves the tail mapped.
    assert_eq!(shim.munmap(addr, 4096), Ok(()));
    assert_eq!(shim.orphan_count(), 1);
    assert_eq!(shim.munmap(addr + 4096, 4096), Ok(()));
    assert_eq!(shim.orphan_count(), 0);
    assert!(shim.memory_mut().live.is_empty());
    assert_eq!(shim.munmap(addr, 4096), Err(EINVAL));
}

#[test]
fn allocations_this_cell_cannot_back_are_enomem_never_faked() {
    let (mut shim, fd, mut mem) = open();
    shim.memory_mut().limit = 4096;
    assert_eq!(gem_create(&mut shim, fd, &mut mem, 8192), Err(ENOMEM));
    // A refused export (Tier-2 private root, D-G0) frees the buffer and is ENOMEM too.
    shim.memory_mut().refuse_export = true;
    assert_eq!(gem_create(&mut shim, fd, &mut mem, 4096), Err(ENOMEM));
    assert!(shim.memory_mut().live.is_empty());
    // Invalid sizes are what xe answers for them.
    shim.memory_mut().refuse_export = false;
    assert_eq!(gem_create(&mut shim, fd, &mut mem, 0), Err(EINVAL));
    assert_eq!(gem_create(&mut shim, fd, &mut mem, 100), Err(EINVAL));
    // Nothing reached the driver's tables.
    assert!(kernel(&mut shim).borrow().leases.is_empty());
}

#[test]
fn driver_refusal_revokes_and_frees_the_export() {
    let (mut shim, fd, mut mem) = open();
    let mut create = arg(DRM_IOCTL_XE_GEM_CREATE);
    put(&mut create, 8, &4096u64.to_le_bytes());
    put(&mut create, 16, &1u32.to_le_bytes());
    put(&mut create, 32, &99u16.to_le_bytes()); // unknown CPU caching
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_GEM_CREATE, &mut create, &mut mem),
        Err(EINVAL)
    );
    let k = kernel(&mut shim);
    assert_eq!(k.borrow().revoked, [1]);
    assert!(shim.memory_mut().live.is_empty());
}

#[test]
fn close_revokes_after_the_driver_zapped_and_waits_for_pending_drains() {
    let (mut shim, fd, mut mem) = open();
    let mut vm = arg(DRM_IOCTL_XE_VM_CREATE);
    shim.ioctl(fd, DRM_IOCTL_XE_VM_CREATE, &mut vm, &mut mem)
        .unwrap();
    let handle = gem_create(&mut shim, fd, &mut mem, 4096).unwrap();
    let mut bind = arg(DRM_IOCTL_XE_VM_BIND);
    put(&mut bind, 8, &u32_at(&vm, 12).to_le_bytes());
    put(&mut bind, 20, &1u32.to_le_bytes());
    put(&mut bind, 24 + 8, &handle.to_le_bytes());
    put(&mut bind, 24 + 12, &2u16.to_le_bytes());
    put(&mut bind, 24 + 24, &4096u64.to_le_bytes());
    put(&mut bind, 24 + 32, &0x10_0000u64.to_le_bytes());
    shim.ioctl(fd, DRM_IOCTL_XE_VM_BIND, &mut bind, &mut mem)
        .unwrap();
    let k = kernel(&mut shim);
    assert_eq!(k.borrow().leaves.len(), 1);

    // The kernel has not finished the drain when the shim revokes: memory stays locked.
    k.borrow_mut().drain_pending = true;
    assert_eq!(shim.close(fd), Ok(()));
    // The driver's own revoke (as holder, after clearing the leaf) ended the lease.
    assert!(k.borrow().leaves.is_empty());
    assert_eq!(shim.orphan_count(), 1);
    k.borrow_mut().drain_pending = false;
    shim.open("/dev/dri/renderD128").unwrap();
    assert_eq!(shim.orphan_count(), 0);
    assert!(shim.memory_mut().live.is_empty());
}

#[test]
fn vm_bind_vector_and_exec_syncs_are_declared() {
    let (mut shim, fd, mut mem) = open();
    let mut vm = arg(DRM_IOCTL_XE_VM_CREATE);
    shim.ioctl(fd, DRM_IOCTL_XE_VM_CREATE, &mut vm, &mut mem)
        .unwrap();
    let handle = gem_create(&mut shim, fd, &mut mem, 8192).unwrap();
    // Two ops in user memory through `vector_of_binds`.
    for (i, va) in [0x10_0000u64, 0x20_0000].iter().enumerate() {
        let op = &mut mem.0[i * 80..(i + 1) * 80];
        op[8..12].copy_from_slice(&handle.to_le_bytes());
        op[12..14].copy_from_slice(&2u16.to_le_bytes());
        op[16..24].copy_from_slice(&(i as u64 * 4096).to_le_bytes());
        op[24..32].copy_from_slice(&4096u64.to_le_bytes());
        op[32..40].copy_from_slice(&va.to_le_bytes());
    }
    let mut bind = arg(DRM_IOCTL_XE_VM_BIND);
    put(&mut bind, 8, &u32_at(&vm, 12).to_le_bytes());
    put(&mut bind, 20, &2u32.to_le_bytes());
    put(&mut bind, 24, &BASE.to_le_bytes());
    shim.ioctl(fd, DRM_IOCTL_XE_VM_BIND, &mut bind, &mut mem)
        .unwrap();
    assert_eq!(kernel(&mut shim).borrow().leaves.len(), 2);
    // With a sync array the driver reads it and refuses: completion is not wired.
    put(&mut bind, 108, &1u32.to_le_bytes());
    put(&mut bind, 112, &(BASE + 0x800).to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_VM_BIND, &mut bind, &mut mem),
        Err(EOPNOTSUPP)
    );
    // A sync array outside this process's memory faults on the client side.
    put(&mut bind, 112, &0x20u64.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_VM_BIND, &mut bind, &mut mem),
        Err(EFAULT)
    );
}

#[test]
fn vm_queue_and_exec_lifecycle() {
    let (mut shim, fd, mut mem) = open();
    let mut vm = arg(DRM_IOCTL_XE_VM_CREATE);
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_VM_CREATE, &mut vm, &mut mem),
        Ok(())
    );
    let vm_id = u32_at(&vm, 12);
    assert_eq!(vm_id, 1);

    // drm_xe_engine_class_instance { RENDER, 0, gt 0 } in user memory
    let mut q = arg(DRM_IOCTL_XE_EXEC_QUEUE_CREATE);
    put(&mut q, 8, &1u16.to_le_bytes()); // width
    put(&mut q, 10, &1u16.to_le_bytes()); // num_placements
    put(&mut q, 12, &vm_id.to_le_bytes());
    put(&mut q, 24, &BASE.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_EXEC_QUEUE_CREATE, &mut q, &mut mem),
        Ok(())
    );
    let q_id = u32_at(&q, 20);
    assert_eq!(q_id, 1);

    let mut prop = arg(DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY);
    put(&mut prop, 8, &q_id.to_le_bytes());
    assert_eq!(
        shim.ioctl(
            fd,
            DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY,
            &mut prop,
            &mut mem
        ),
        Ok(())
    );
    assert_eq!(u64_at(&prop, 16), 0); // Not banned

    // Exec of an unmapped batch address is refused, not silently queued.
    let mut exec = arg(DRM_IOCTL_XE_EXEC);
    put(&mut exec, 8, &q_id.to_le_bytes());
    put(&mut exec, 24, &0x10000u64.to_le_bytes());
    put(&mut exec, 32, &1u16.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_EXEC, &mut exec, &mut mem),
        Err(EINVAL)
    );

    let mut q_destroy = arg(DRM_IOCTL_XE_EXEC_QUEUE_DESTROY);
    put(&mut q_destroy, 0, &q_id.to_le_bytes());
    assert_eq!(
        shim.ioctl(
            fd,
            DRM_IOCTL_XE_EXEC_QUEUE_DESTROY,
            &mut q_destroy,
            &mut mem
        ),
        Ok(())
    );

    let mut vm_destroy = arg(DRM_IOCTL_XE_VM_DESTROY);
    put(&mut vm_destroy, 0, &vm_id.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_VM_DESTROY, &mut vm_destroy, &mut mem),
        Ok(())
    );
}

#[test]
fn syncobj_lifecycle() {
    let (mut shim, fd, mut mem) = open();
    let mut create = arg(DRM_IOCTL_SYNCOBJ_CREATE);
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_SYNCOBJ_CREATE, &mut create, &mut mem),
        Ok(())
    );
    let handle = u32_at(&create, 0);
    assert_eq!(handle, 1);

    let mut destroy = arg(DRM_IOCTL_SYNCOBJ_DESTROY);
    put(&mut destroy, 0, &handle.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_SYNCOBJ_DESTROY, &mut destroy, &mut mem),
        Ok(())
    );
}

#[test]
fn malformed_requests_are_refused() {
    let (mut shim, fd, mut mem) = open();
    // Bare ioctl number without direction/size, and a short argument buffer.
    let mut a = [0u8; 8];
    assert_eq!(
        shim.ioctl(fd, DRM_XE_VM_CREATE, &mut a, &mut mem),
        Err(EINVAL)
    );
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_VM_CREATE, &mut a, &mut mem),
        Err(EINVAL)
    );
    // Unsupported extension chain
    let mut vm = arg(DRM_IOCTL_XE_VM_CREATE);
    put(&mut vm, 0, &BASE.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_VM_CREATE, &mut vm, &mut mem),
        Err(EINVAL)
    );
}
