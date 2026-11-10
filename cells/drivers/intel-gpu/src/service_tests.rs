//! Host tests of the `XE_GPU` IPC service on a GPU brought up against the fake MMIO:
//! attested receive buffers carrying real request frames, two isolated clients, and
//! release of a client's files when the kernel reports its Cell dead.

use crate::gpu_tests::{test_gpu, TestGpu};
use crate::owner_watch::WatchSys;
use crate::service::{Outcome, Service};
use alloc::vec;
use alloc::vec::Vec;
use api::caller_identity::{CallerIdentity, CALLER_IDENTITY_LEN};
use intel_gpu_core::uapi::ioctl::*;
use intel_gpu_core::uapi::query::{DRM_XE_DEVICE_QUERY_GT_LIST, DRM_XE_DEVICE_QUERY_GT_TOPOLOGY};
use intel_gpu_core::uapi::{u16_at, u32_at, u64_at, UserMemory};
use intel_gpu_core::wire::{
    decode_reply, encode_request, Segment, MAX_REPLY_LEN, MAX_REQUEST_LEN, OP_CLOSE, OP_IOCTL,
    OP_OPEN, SEG_OUT,
};

/// Kernel stand-in: the root tid of cell `c` is `c * 1000`.
#[derive(Default)]
struct ScriptedWatch {
    refuse: bool,
    watches: Vec<(u64, u64)>,
    cancels: Vec<u64>,
}

impl WatchSys for ScriptedWatch {
    fn watch(&mut self, cell_id: u64, generation: u64) -> Option<(usize, u64)> {
        self.watches.push((cell_id, generation));
        (!self.refuse).then_some((cell_id as usize * 1000, self.watches.len() as u64))
    }

    fn cancel(&mut self, token: u64) {
        self.cancels.push(token);
    }
}

/// One client: a Cell incarnation sending from `tid`, with a little user memory.
struct Client {
    tid: usize,
    cell: u64,
    generation: u64,
    mem: Mem,
}

const BASE: u64 = 0x7000_0000;

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

fn client(tid: usize, cell: u64, generation: u64) -> Client {
    Client {
        tid,
        cell,
        generation,
        mem: Mem(vec![0u8; 4096]),
    }
}

/// A receive buffer as the kernel leaves it: request first, attested trailer last.
fn attested(frame: &[u8], sender: usize, cell: u64, generation: u64) -> Vec<u8> {
    let mut buf = vec![0u8; api::ipc::IPC_BUF_SIZE];
    buf[..frame.len()].copy_from_slice(frame);
    let identity = CallerIdentity {
        flags: 0,
        cell_id: cell,
        generation,
        sender_tid: sender as u64,
    };
    buf[api::ipc::IPC_BUF_SIZE - CALLER_IDENTITY_LEN..].copy_from_slice(&identity.to_trailer());
    buf
}

/// A GPU brought up on the fake Lunar Lake MMIO and GuC, serving `XE_GPU`.
fn service() -> Service<TestGpu> {
    let gpu = test_gpu();
    let device = intel_gpu_core::uapi::query::DeviceInfo {
        name: "Intel Xe2 (LNL)",
        ip_ver_major: 20,
        ip_ver_minor: 4,
        ip_ver_rev: 0,
        reference_clock_hz: 38_400_000,
        dss_geometry: 0xFF,
        dss_compute: 0x3F,
        eu_per_dss: 0xFF,
        sysmem_bytes: 1 << 38,
    };
    Service::new(device, gpu)
}

/// Send one operation as `c`; returns `(errno, value)` after applying user writes.
// One parameter per field of a request frame plus the session it goes through.
#[allow(clippy::too_many_arguments)]
fn call(
    svc: &mut Service<TestGpu>,
    watch: &mut ScriptedWatch,
    c: &mut Client,
    op: u8,
    file: u32,
    request: u32,
    arg: &mut [u8],
    segments: &[Segment],
) -> (i32, u32) {
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let len = encode_request(&mut frame, op, file, request, arg, segments, &c.mem).unwrap();
    let buf = attested(&frame[..len], c.tid, c.cell, c.generation);
    let mut reply = [0u8; MAX_REPLY_LEN];
    let Outcome::Reply { len, .. } = svc.handle(&buf, c.tid, &mut reply, watch) else {
        panic!("attested request not served");
    };
    let decoded = decode_reply(&reply[..len], segments).unwrap();
    if !decoded.arg.is_empty() {
        arg.copy_from_slice(decoded.arg);
    }
    decoded.apply_writes(&mut c.mem).unwrap();
    (decoded.errno, decoded.value)
}

fn open(svc: &mut Service<TestGpu>, watch: &mut ScriptedWatch, c: &mut Client) -> u32 {
    let (errno, file) = call(svc, watch, c, OP_OPEN, 0, 0, &mut [], &[]);
    assert_eq!(errno, 0);
    file
}

fn vm_create(svc: &mut Service<TestGpu>, w: &mut ScriptedWatch, c: &mut Client, file: u32) -> u32 {
    let mut arg = [0u8; 32];
    assert_eq!(
        call(
            svc,
            w,
            c,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_VM_CREATE,
            &mut arg,
            &[]
        )
        .0,
        0
    );
    u32_at(&arg, 12)
}

fn vm_destroy(
    svc: &mut Service<TestGpu>,
    w: &mut ScriptedWatch,
    c: &mut Client,
    file: u32,
    vm: u32,
) -> i32 {
    let mut arg = [0u8; 24];
    arg[..4].copy_from_slice(&vm.to_le_bytes());
    call(
        svc,
        w,
        c,
        OP_IOCTL,
        file,
        DRM_IOCTL_XE_VM_DESTROY,
        &mut arg,
        &[],
    )
    .0
}

/// `DEVICE_QUERY` the way libdrm does it: size first, then copy into user memory.
fn query(
    svc: &mut Service<TestGpu>,
    w: &mut ScriptedWatch,
    c: &mut Client,
    file: u32,
    id: u32,
) -> Vec<u8> {
    let mut arg = [0u8; 40];
    arg[8..12].copy_from_slice(&id.to_le_bytes());
    assert_eq!(
        call(
            svc,
            w,
            c,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_DEVICE_QUERY,
            &mut arg,
            &[]
        )
        .0,
        0
    );
    let size = u32_at(&arg, 12);
    arg[16..24].copy_from_slice(&BASE.to_le_bytes());
    let out = [Segment {
        addr: BASE,
        len: size,
        flags: SEG_OUT,
    }];
    assert_eq!(
        call(
            svc,
            w,
            c,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_DEVICE_QUERY,
            &mut arg,
            &out
        )
        .0,
        0
    );
    c.mem.0[..size as usize].to_vec()
}

#[test]
fn device_query_reports_the_probed_gt() {
    let (mut svc, mut w) = (service(), ScriptedWatch::default());
    let mut a = client(101, 1, 1);
    let file = open(&mut svc, &mut w, &mut a);

    let gt = query(&mut svc, &mut w, &mut a, file, DRM_XE_DEVICE_QUERY_GT_LIST);
    assert_eq!(u32_at(&gt, 0), 1);
    assert_eq!(u32_at(&gt, 8 + 12), 38_400_000); // RPM_CONFIG0 crystal code 2
    assert_eq!((u16_at(&gt, 8 + 32), u16_at(&gt, 8 + 34)), (20, 4)); // GMD_ID 20.04

    let topo = query(
        &mut svc,
        &mut w,
        &mut a,
        file,
        DRM_XE_DEVICE_QUERY_GT_TOPOLOGY,
    );
    // Geometry then compute DSS (16-byte masks), each after an 8-byte header.
    assert_eq!(u64_at(&topo, 8), 0xFF);
    assert_eq!(u16_at(&topo, 24 + 2), 2);
    assert_eq!(u64_at(&topo, 24 + 8), 0x3F);
}

#[test]
fn two_clients_are_isolated() {
    let (mut svc, mut w) = (service(), ScriptedWatch::default());
    let mut a = client(101, 1, 1);
    let mut b = client(202, 2, 1);
    let file_a = open(&mut svc, &mut w, &mut a);
    let file_b = open(&mut svc, &mut w, &mut b);
    assert_eq!((file_a, file_b), (1, 1));

    assert_eq!(vm_create(&mut svc, &mut w, &mut a, file_a), 1);
    assert_eq!(vm_create(&mut svc, &mut w, &mut a, file_a), 2);
    // B names A's VM 2 through its own file 1: it does not exist there.
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut b, file_b, 2), ENOENT);
    assert_eq!(vm_create(&mut svc, &mut w, &mut b, file_b), 1);
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut a, file_a, 2), 0);
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut a, file_a, 1), 0);
    // A's destroys left B's VM 1 alone.
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut b, file_b, 1), 0);

    // A second file of A is A's only; B cannot close it.
    let file_a2 = open(&mut svc, &mut w, &mut a);
    assert_eq!(
        call(&mut svc, &mut w, &mut b, OP_CLOSE, file_a2, 0, &mut [], &[]).0,
        EBADF
    );
    assert_eq!(
        call(&mut svc, &mut w, &mut a, OP_CLOSE, file_a2, 0, &mut [], &[]).0,
        0
    );
}

#[test]
fn client_death_releases_its_files() {
    let (mut svc, mut w) = (service(), ScriptedWatch::default());
    let mut root = client(3000, 3, 1);
    let mut worker = client(3001, 3, 1);
    let mut other = client(202, 2, 1);
    let file = open(&mut svc, &mut w, &mut root);
    let worker_file = open(&mut svc, &mut w, &mut worker);
    open(&mut svc, &mut w, &mut other);
    vm_create(&mut svc, &mut w, &mut root, file);
    assert_eq!(w.watches, [(3, 1), (2, 1)], "one watch per Cell");

    // An unattributed message from a tid that is not a watched root: ignored.
    let mut reply = [0u8; MAX_REPLY_LEN];
    let zero = vec![0u8; api::ipc::IPC_BUF_SIZE];
    assert_eq!(
        svc.handle(&zero, 3001, &mut reply, &mut w),
        Outcome::Dropped
    );
    // The kernel's death record for cell 3 (sender = root tid, no trailer).
    assert_eq!(
        svc.handle(&zero, 3000, &mut reply, &mut w),
        Outcome::Released { files: 2 }
    );
    assert_eq!(
        svc.handle(&zero, 3000, &mut reply, &mut w),
        Outcome::Dropped
    );

    // Both threads' files are gone; the other client is untouched.
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut root, file, 1), EBADF);
    assert_eq!(
        vm_destroy(&mut svc, &mut w, &mut worker, worker_file, 1),
        EBADF
    );
    assert_eq!(vm_create(&mut svc, &mut w, &mut other, 1), 1);
}

#[test]
fn restarted_cell_generation_finds_nothing_of_the_old_one() {
    let (mut svc, mut w) = (service(), ScriptedWatch::default());
    let mut old = client(4000, 4, 1);
    let file = open(&mut svc, &mut w, &mut old);
    vm_create(&mut svc, &mut w, &mut old, file);
    // Same CellId, new generation, a different tid: the old watch is cancelled and the
    // old generation's files released.
    let mut new = client(4100, 4, 2);
    open(&mut svc, &mut w, &mut new);
    assert_eq!(w.cancels, [1]);
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut old, file, 1), EBADF);
    // The same tid reused by the new generation inherits nothing either.
    let mut reused = client(4000, 4, 2);
    assert_eq!(vm_destroy(&mut svc, &mut w, &mut reused, file, 1), EBADF);
}

#[test]
fn spoofed_or_missing_identity_is_never_served() {
    let (mut svc, mut w) = (service(), ScriptedWatch::default());
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let len = encode_request(&mut frame, OP_OPEN, 0, 0, &[], &[], &Mem(Vec::new())).unwrap();
    // The trailer names tid 101 but the kernel reports sender 202.
    let buf = attested(&frame[..len], 101, 1, 1);
    let mut reply = [0u8; MAX_REPLY_LEN];
    assert_eq!(svc.handle(&buf, 202, &mut reply, &mut w), Outcome::Dropped);
    assert!(w.watches.is_empty());
}

#[test]
fn gem_create_needs_the_senders_export_and_death_zaps_before_revoking() {
    let (mut svc, mut w) = (service(), ScriptedWatch::default());
    let mut a = client(3000, 3, 1);
    let mut b = client(4000, 4, 1);
    svc.gpu.exports.add(7, 3, 1, 1 << 38, 0x2000, false);
    let file = open(&mut svc, &mut w, &mut a);
    let file_b = open(&mut svc, &mut w, &mut b);
    let mut gem = [0u8; 56];
    gem[8..16].copy_from_slice(&0x2000u64.to_le_bytes());
    gem[16..20].copy_from_slice(&1u32.to_le_bytes());
    gem[32..34].copy_from_slice(&1u16.to_le_bytes());
    let export = [Segment::export(7)];
    // No attachment: no memory to place the object in.
    let mut arg = gem;
    assert_eq!(
        call(
            &mut svc,
            &mut w,
            &mut a,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_GEM_CREATE,
            &mut arg,
            &[]
        )
        .0,
        EINVAL
    );
    // Another Cell carrying A's export id: the confused deputy is refused.
    let mut arg = gem;
    assert_eq!(
        call(
            &mut svc,
            &mut w,
            &mut b,
            OP_IOCTL,
            file_b,
            DRM_IOCTL_XE_GEM_CREATE,
            &mut arg,
            &export
        )
        .0,
        EPERM
    );
    let mut arg = gem;
    assert_eq!(
        call(
            &mut svc,
            &mut w,
            &mut a,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_GEM_CREATE,
            &mut arg,
            &export
        )
        .0,
        0
    );
    let handle = u32_at(&arg, 28);

    // Bind it (read-only export: read-only leaves) and run a queue on it.
    let vm = vm_create(&mut svc, &mut w, &mut a, file);
    let mut bind = [0u8; 136];
    bind[8..12].copy_from_slice(&vm.to_le_bytes());
    bind[20..24].copy_from_slice(&1u32.to_le_bytes());
    bind[24 + 8..24 + 12].copy_from_slice(&handle.to_le_bytes());
    bind[24 + 12..24 + 14].copy_from_slice(&2u16.to_le_bytes());
    bind[24 + 24..24 + 32].copy_from_slice(&0x2000u64.to_le_bytes());
    bind[24 + 32..24 + 40].copy_from_slice(&0x10_0000u64.to_le_bytes());
    assert_eq!(
        call(
            &mut svc,
            &mut w,
            &mut a,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_VM_BIND,
            &mut bind,
            &[]
        )
        .0,
        0
    );
    assert_eq!(
        svc.gpu
            .leaf(intel_gpu_core::hw::HwVm(1), 0x10_0000)
            .unwrap()
            & 2,
        0
    );
    a.mem.0[..8].fill(0);
    let mut q = [0u8; 48];
    q[8..10].copy_from_slice(&1u16.to_le_bytes());
    q[10..12].copy_from_slice(&1u16.to_le_bytes());
    q[12..16].copy_from_slice(&vm.to_le_bytes());
    q[24..32].copy_from_slice(&BASE.to_le_bytes());
    let instance = [Segment {
        addr: BASE,
        len: 8,
        flags: intel_gpu_core::wire::SEG_IN,
    }];
    assert_eq!(
        call(
            &mut svc,
            &mut w,
            &mut a,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_EXEC_QUEUE_CREATE,
            &mut q,
            &instance
        )
        .0,
        0
    );
    let mut exec = [0u8; 56];
    exec[8..12].copy_from_slice(&u32_at(&q, 20).to_le_bytes());
    exec[24..32].copy_from_slice(&0x10_0000u64.to_le_bytes());
    exec[32..34].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        call(
            &mut svc,
            &mut w,
            &mut a,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_EXEC,
            &mut exec,
            &[]
        )
        .0,
        0
    );

    // The kernel reports cell 3 dead: contexts stopped, leaves cleared, then the revoke,
    // which finds no leaf into its window.
    let mut reply = [0u8; MAX_REPLY_LEN];
    let zero = vec![0u8; api::ipc::IPC_BUF_SIZE];
    assert_eq!(
        svc.handle(&zero, 3000, &mut reply, &mut w),
        Outcome::Released { files: 1 }
    );
    assert_eq!(svc.gpu.exports.revoked, [7]);
    assert!(svc.gpu.exports.violations.is_empty());
    assert_eq!(svc.gpu.queue_count(), 0);
    assert_eq!(svc.gpu.vm_count(), 0);
}

#[test]
fn refused_watch_still_serves_and_reports_once() {
    let mut svc = service();
    let mut w = ScriptedWatch {
        refuse: true,
        ..ScriptedWatch::default()
    };
    let mut a = client(101, 1, 1);
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let len = encode_request(&mut frame, OP_OPEN, 0, 0, &[], &[], &a.mem).unwrap();
    let buf = attested(&frame[..len], a.tid, a.cell, a.generation);
    let mut reply = [0u8; MAX_REPLY_LEN];
    let first = svc.handle(&buf, a.tid, &mut reply, &mut w);
    assert!(matches!(
        first,
        Outcome::Reply {
            watch_unavailable: true,
            ..
        }
    ));
    let second = svc.handle(&buf, a.tid, &mut reply, &mut w);
    assert!(matches!(
        second,
        Outcome::Reply {
            watch_unavailable: false,
            ..
        }
    ));
    // Served: the client has two working files.
    assert_eq!(vm_create(&mut svc, &mut w, &mut a, 2), 1);
}
