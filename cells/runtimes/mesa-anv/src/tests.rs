//! Host unit tests for the Mesa `anv` DRM transport shim, written against the uapi struct
//! layouts and full request codes rather than the shim's own decoding. Every request
//! crosses the real `XE_GPU` frames into the driver's request server.

use crate::shim::DrmShim;
use crate::transport::Transport;
use alloc::vec;
use alloc::vec::Vec;
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
    sysmem_bytes: 16 << 30,
};

/// The driver's request server in-process, answering as one client tid.
struct Loopback(XeServer);

impl Transport for Loopback {
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, i32> {
        let caller = Caller {
            tid: 42,
            cell_id: 9,
            generation: 1,
        };
        Ok(self.0.handle(caller, request, reply))
    }
}

fn shim() -> DrmShim<Loopback> {
    DrmShim::new(Loopback(XeServer::new(DEVICE, &[])))
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

fn open() -> (DrmShim<Loopback>, i32, FakeMem) {
    let mut shim = shim();
    let fd = shim.open("/dev/dri/renderD128").unwrap();
    (shim, fd, FakeMem(vec![0u8; 4096]))
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
fn gem_lifecycle() {
    let (mut shim, fd, mut mem) = open();
    let mut create = arg(DRM_IOCTL_XE_GEM_CREATE);
    put(&mut create, 8, &8192u64.to_le_bytes());
    put(&mut create, 16, &1u32.to_le_bytes()); // placement: sysmem
    put(&mut create, 32, &2u16.to_le_bytes()); // DRM_XE_GEM_CPU_CACHING_WC
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_GEM_CREATE, &mut create, &mut mem),
        Ok(())
    );
    let handle = u32_at(&create, 28);
    assert_eq!(handle, 1);

    let mut mmap = arg(DRM_IOCTL_XE_GEM_MMAP_OFFSET);
    put(&mut mmap, 8, &handle.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_XE_GEM_MMAP_OFFSET, &mut mmap, &mut mem),
        Ok(())
    );
    assert_eq!(u64_at(&mmap, 16), 1u64 << 32);

    let mut close = arg(DRM_IOCTL_GEM_CLOSE);
    put(&mut close, 0, &handle.to_le_bytes());
    assert_eq!(
        shim.ioctl(fd, DRM_IOCTL_GEM_CLOSE, &mut close, &mut mem),
        Ok(())
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
