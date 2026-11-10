//! The ioctl decoder against the uapi struct layouts, over a recording device.

use super::*;
use crate::hw::fake::{Event, RecordingHw};
use crate::hw::{HwQueue, HwVm};
use crate::uapi::query::DeviceInfo;

const DEVICE: DeviceInfo = DeviceInfo {
    name: "test",
    ip_ver_major: 30,
    ip_ver_minor: 0,
    ip_ver_rev: 0,
    reference_clock_hz: 24_000_000,
    dss_geometry: 0xff,
    dss_compute: 0xff,
    eu_per_dss: 8,
    sysmem_bytes: 1 << 38,
};
const ME: Sender = Sender {
    cell_id: 9,
    generation: 2,
};
const BASE: u64 = 0x1000_0000;

struct Mem(Vec<u8>);

impl UserMemory for Mem {
    fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), i32> {
        let at = addr.checked_sub(BASE).ok_or(EFAULT)? as usize;
        out.copy_from_slice(self.0.get(at..at + out.len()).ok_or(EFAULT)?);
        Ok(())
    }
    fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), i32> {
        let at = addr.checked_sub(BASE).ok_or(EFAULT)? as usize;
        self.0
            .get_mut(at..at + data.len())
            .ok_or(EFAULT)?
            .copy_from_slice(data);
        Ok(())
    }
}

struct Harness {
    file: DrmFile,
    hw: RecordingHw,
    mem: Mem,
}

impl Harness {
    fn new() -> Self {
        Self {
            file: DrmFile::new(),
            hw: RecordingHw::default(),
            mem: Mem(vec![0u8; 8192]),
        }
    }

    fn call(&mut self, request: u32, arg: &mut [u8], exports: &[u64]) -> Result<(), i32> {
        self.call_as(ME, request, arg, exports)
    }

    fn call_as(
        &mut self,
        sender: Sender,
        request: u32,
        arg: &mut [u8],
        exports: &[u64],
    ) -> Result<(), i32> {
        let mut ctx = IoctlContext {
            sender,
            exports,
            hw: &mut self.hw,
        };
        self.file
            .ioctl(&DEVICE, request, arg, &mut self.mem, &mut ctx)
    }

    fn vm(&mut self) -> u32 {
        let mut a = [0u8; 32];
        self.call(DRM_IOCTL_XE_VM_CREATE, &mut a, &[]).unwrap();
        u32_at(&a, 12)
    }

    fn gem(&mut self, size: u64, export: u64) -> Result<u32, i32> {
        let mut a = gem_create_arg(size);
        self.call(DRM_IOCTL_XE_GEM_CREATE, &mut a, &[export])
            .map(|()| u32_at(&a, 28))
    }

    fn queue(&mut self, vm: u32) -> u32 {
        // drm_xe_engine_class_instance { RENDER, 0, gt 0 } at BASE.
        self.mem.0[..8].fill(0);
        let mut q = [0u8; 48];
        q[8..10].copy_from_slice(&1u16.to_le_bytes());
        q[10..12].copy_from_slice(&1u16.to_le_bytes());
        q[12..16].copy_from_slice(&vm.to_le_bytes());
        q[24..32].copy_from_slice(&BASE.to_le_bytes());
        self.call(DRM_IOCTL_XE_EXEC_QUEUE_CREATE, &mut q, &[])
            .unwrap();
        u32_at(&q, 20)
    }
}

fn gem_create_arg(size: u64) -> [u8; 56] {
    let mut a = [0u8; 56];
    a[8..16].copy_from_slice(&size.to_le_bytes());
    a[16..20].copy_from_slice(&1u32.to_le_bytes()); // placement: sysmem
    a[32..34].copy_from_slice(&1u16.to_le_bytes()); // WB
    a
}

fn bind_op(obj: u32, offset: u64, addr: u64, range: u64, op: u32, flags: u32) -> [u8; 80] {
    let mut b = [0u8; 80];
    b[8..12].copy_from_slice(&obj.to_le_bytes());
    b[12..14].copy_from_slice(&2u16.to_le_bytes()); // pat_index
    b[16..24].copy_from_slice(&offset.to_le_bytes());
    b[24..32].copy_from_slice(&range.to_le_bytes());
    b[32..40].copy_from_slice(&addr.to_le_bytes());
    b[40..44].copy_from_slice(&op.to_le_bytes());
    b[44..48].copy_from_slice(&flags.to_le_bytes());
    b
}

fn vm_bind_arg(vm: u32, ops: &[[u8; 80]]) -> [u8; 136] {
    let mut a = [0u8; 136];
    a[8..12].copy_from_slice(&vm.to_le_bytes());
    a[20..24].copy_from_slice(&(ops.len() as u32).to_le_bytes());
    if ops.len() == 1 {
        a[24..104].copy_from_slice(&ops[0]);
    } else {
        a[24..32].copy_from_slice(&BASE.to_le_bytes());
    }
    a
}

fn exec_arg(queue: u32, addr: u64, batches: u16) -> [u8; 56] {
    let mut a = [0u8; 56];
    a[8..12].copy_from_slice(&queue.to_le_bytes());
    a[24..32].copy_from_slice(&addr.to_le_bytes());
    a[32..34].copy_from_slice(&batches.to_le_bytes());
    a
}

#[test]
fn request_codes_match_linux_headers() {
    assert_eq!(DRM_IOCTL_XE_VM_BIND, 0x4088_6445);
    assert_eq!(DRM_IOCTL_XE_GEM_CREATE, 0xC038_6441);
    assert_eq!(DRM_IOCTL_XE_EXEC, 0x4038_6449);
}

#[test]
fn gem_create_maps_the_attached_export_of_the_sender() {
    let mut h = Harness::new();
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x4000, true);
    let handle = h.gem(0x4000, 7).unwrap();
    assert_eq!(handle, 1);
    assert_eq!(h.hw.events, [Event::MapExport(7)]);
    let bo = &h.file.session.gem_objects[&handle];
    assert_eq!((bo.iova, bo.size, bo.writable), (1 << 38, 0x4000, true));
}

#[test]
fn gem_create_without_or_with_extra_attachments_is_refused() {
    let mut h = Harness::new();
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x4000, true);
    h.hw.add_export(8, ME.cell_id, ME.generation, 2 << 38, 0x4000, true);
    let mut a = gem_create_arg(0x4000);
    assert_eq!(h.call(DRM_IOCTL_XE_GEM_CREATE, &mut a, &[]), Err(EINVAL));
    assert_eq!(
        h.call(DRM_IOCTL_XE_GEM_CREATE, &mut a, &[7, 8]),
        Err(EINVAL)
    );
    // A malformed struct never reaches 438.
    let mut bad = gem_create_arg(0x4000);
    bad[40] = 1; // reserved
    assert_eq!(h.call(DRM_IOCTL_XE_GEM_CREATE, &mut bad, &[7]), Err(EINVAL));
    let mut vram = gem_create_arg(0x4000);
    vram[16..20].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        h.call(DRM_IOCTL_XE_GEM_CREATE, &mut vram, &[7]),
        Err(EINVAL)
    );
    assert!(h.hw.events.is_empty());
    // An attachment on any other request is malformed.
    let mut vm = [0u8; 32];
    assert_eq!(h.call(DRM_IOCTL_XE_VM_CREATE, &mut vm, &[7]), Err(EINVAL));
}

#[test]
fn confused_deputy_export_of_another_cell_is_refused() {
    let mut h = Harness::new();
    // Cell 10's lease, and cell 9's own lease of an older generation.
    h.hw.add_export(7, 10, ME.generation, 1 << 38, 0x4000, true);
    h.hw.add_export(8, ME.cell_id, ME.generation - 1, 2 << 38, 0x4000, true);
    assert_eq!(h.gem(0x4000, 7), Err(EPERM));
    assert_eq!(h.gem(0x4000, 8), Err(EPERM));
    assert!(h.file.session.gem_objects.is_empty());
    // The rightful owner can still use its lease: nothing was revoked.
    assert!(!h.hw.events.iter().any(|e| matches!(e, Event::Revoke(_))));
    let mut a = gem_create_arg(0x4000);
    let owner = Sender {
        cell_id: 10,
        generation: ME.generation,
    };
    assert_eq!(
        h.call_as(owner, DRM_IOCTL_XE_GEM_CREATE, &mut a, &[7]),
        Ok(())
    );
}

#[test]
fn gem_create_errors_follow_the_record() {
    let mut h = Harness::new();
    assert_eq!(h.gem(0x4000, 99), Err(ENOENT));
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x2000, true);
    // The window is smaller than the object.
    assert_eq!(h.gem(0x4000, 7), Err(EINVAL));
    assert_eq!(h.gem(0x2000, 7), Ok(1));
}

#[test]
fn vm_bind_inline_and_vector_forms() {
    let mut h = Harness::new();
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x4000, true);
    let vm = h.vm();
    let bo = h.gem(0x4000, 7).unwrap();
    let mut a = vm_bind_arg(vm, &[bind_op(bo, 0x1000, 0x10_0000, 0x1000, 0, 0)]);
    h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]).unwrap();
    assert_eq!(
        h.hw.events.last(),
        Some(&Event::Map {
            vm: HwVm(1),
            va: 0x10_0000,
            dma: (1 << 38) + 0x1000,
            len: 0x1000,
            writable: true
        })
    );
    // Two ops through `vector_of_binds`, the second read-only.
    let ops = [
        bind_op(bo, 0, 0x20_0000, 0x1000, 0, 0),
        bind_op(bo, 0x2000, 0x30_0000, 0x2000, 0, 1),
    ];
    h.mem.0[..80].copy_from_slice(&ops[0]);
    h.mem.0[80..160].copy_from_slice(&ops[1]);
    let mut a = vm_bind_arg(vm, &ops);
    h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]).unwrap();
    assert!(matches!(
        h.hw.events.last(),
        Some(Event::Map {
            va: 0x30_0000,
            writable: false,
            ..
        })
    ));
    assert_eq!(h.file.session.vms[&vm].mappings.len(), 3);
    // A vector outside the declared memory faults.
    let mut a = vm_bind_arg(vm, &ops);
    a[24..32].copy_from_slice(&0x20u64.to_le_bytes());
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EFAULT));
}

#[test]
fn vm_bind_refusals() {
    let mut h = Harness::new();
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x4000, true);
    let vm = h.vm();
    let bo = h.gem(0x4000, 7).unwrap();
    let op = bind_op(bo, 0, 0x10_0000, 0x1000, 0, 0);
    // Syncs are refused, not accepted and never signalled.
    let mut a = vm_bind_arg(vm, &[op]);
    a[108..112].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EOPNOTSUPP));
    a[108..112].copy_from_slice(&1025u32.to_le_bytes());
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EINVAL));
    // Unknown bind queue; a render queue is not a bind queue.
    let mut a = vm_bind_arg(vm, &[op]);
    a[12..16].copy_from_slice(&5u32.to_le_bytes());
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(ENOENT));
    let q = h.queue(vm);
    a[12..16].copy_from_slice(&q.to_le_bytes());
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EINVAL));
    // MAP_USERPTR and PREFETCH: what xe answers without GPUSVM / a placement.
    for kind in [2u32, 4] {
        let mut a = vm_bind_arg(vm, &[bind_op(0, 0, 0x10_0000, 0x1000, kind, 0)]);
        assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EINVAL));
    }
    // Reserved bytes, a prefetch region on a map.
    let mut bad = op;
    bad[60] = 1;
    let mut a = vm_bind_arg(vm, &[bad]);
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EINVAL));
    let mut bad = op;
    bad[48] = 1;
    let mut a = vm_bind_arg(vm, &[bad]);
    assert_eq!(h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]), Err(EINVAL));
    assert!(h.file.session.vms[&vm].mappings.is_empty());
}

#[test]
fn exec_queue_create_registers_a_context() {
    let mut h = Harness::new();
    let vm = h.vm();
    let q = h.queue(vm);
    assert_eq!(q, 1);
    assert_eq!(
        h.hw.events.last(),
        Some(&Event::QueueCreate(HwQueue(1), HwVm(1)))
    );
}

#[test]
fn exec_checks_in_xe_order() {
    let mut h = Harness::new();
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x1000, true);
    let vm = h.vm();
    let bo = h.gem(0x1000, 7).unwrap();
    let mut a = vm_bind_arg(vm, &[bind_op(bo, 0, 0x10_0000, 0x1000, 0, 0)]);
    h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]).unwrap();
    let q = h.queue(vm);

    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut exec_arg(9, 0x10_0000, 1), &[]),
        Err(ENOENT)
    );
    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut exec_arg(q, 0x10_0000, 2), &[]),
        Err(EINVAL)
    );
    let mut with_sync = exec_arg(q, 0x10_0000, 1);
    with_sync[12..16].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut with_sync, &[]),
        Err(EOPNOTSUPP)
    );
    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut exec_arg(q, 0x20_0000, 1), &[]),
        Err(EINVAL)
    );
    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut exec_arg(q, 0x10_0000, 0), &[]),
        Ok(())
    );
    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut exec_arg(q, 0x10_0000, 1), &[]),
        Ok(())
    );
    assert_eq!(
        h.hw.events.last(),
        Some(&Event::Submit(HwQueue(1), 0x10_0000))
    );

    // Banned by the GuC: canceled before the syncs check, and the ban is reported.
    h.hw.bans.insert(HwQueue(1), 1);
    assert_eq!(
        h.call(DRM_IOCTL_XE_EXEC, &mut with_sync, &[]),
        Err(ECANCELED)
    );
    let mut prop = [0u8; 40];
    prop[8..12].copy_from_slice(&q.to_le_bytes());
    h.call(DRM_IOCTL_XE_EXEC_QUEUE_GET_PROPERTY, &mut prop, &[])
        .unwrap();
    assert_eq!(u64_at(&prop, 16), 1);
}

#[test]
fn release_zaps_before_revoking() {
    let mut h = Harness::new();
    h.hw.add_export(7, ME.cell_id, ME.generation, 1 << 38, 0x1000, false);
    let vm = h.vm();
    let bo = h.gem(0x1000, 7).unwrap();
    let mut a = vm_bind_arg(vm, &[bind_op(bo, 0, 0x10_0000, 0x1000, 0, 0)]);
    h.call(DRM_IOCTL_XE_VM_BIND, &mut a, &[]).unwrap();
    // A device-read-only export maps read-only even without the READONLY flag.
    assert!(matches!(
        h.hw.events.last(),
        Some(Event::Map {
            writable: false,
            ..
        })
    ));
    let revoked = h.file.release(&mut h.hw);
    assert_eq!(revoked, [7]);
    let unmap = h.hw.position(|e| matches!(e, Event::Unmap { .. })).unwrap();
    let revoke = h.hw.position(|e| *e == Event::Revoke(7)).unwrap();
    assert!(unmap < revoke);
}
