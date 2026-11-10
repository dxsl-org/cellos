use super::*;
use crate::hw::fake::{Event, RecordingHw};
use crate::uapi::ioctl::*;
use crate::uapi::u32_at;
use crate::uapi::UserMemory;
use crate::wire::{decode_reply, encode_request, Segment, MAX_REPLY_LEN, MAX_REQUEST_LEN};
use alloc::vec::Vec;

const DEVICE: DeviceInfo = DeviceInfo {
    name: "test",
    ip_ver_major: 20,
    ip_ver_minor: 4,
    ip_ver_rev: 0,
    reference_clock_hz: 38_400_000,
    dss_geometry: 0xFF,
    dss_compute: 0xFF,
    eu_per_dss: 0xFF,
    sysmem_bytes: 0,
};

const A: Caller = Caller {
    tid: 100,
    cell_id: 7,
    generation: 1,
};
const B: Caller = Caller {
    tid: 200,
    cell_id: 8,
    generation: 1,
};

/// A client with no user memory: only pointer-free requests are sent.
struct NoMem;

impl UserMemory for NoMem {
    fn read(&self, _: u64, _: &mut [u8]) -> Result<(), i32> {
        Err(EFAULT)
    }

    fn write(&mut self, _: u64, _: &[u8]) -> Result<(), i32> {
        Err(EFAULT)
    }
}

struct Rig {
    server: XeServer,
    hw: RecordingHw,
}

impl Rig {
    fn new() -> Self {
        Self {
            server: XeServer::new(DEVICE),
            hw: RecordingHw::default(),
        }
    }

    /// Send one frame and return `(errno, value, arg)`.
    fn call(
        &mut self,
        caller: Caller,
        op: u8,
        file: u32,
        request: u32,
        arg: &[u8],
    ) -> (i32, u32, Vec<u8>) {
        self.call_with(caller, op, file, request, arg, &[])
    }

    fn call_with(
        &mut self,
        caller: Caller,
        op: u8,
        file: u32,
        request: u32,
        arg: &[u8],
        segments: &[Segment],
    ) -> (i32, u32, Vec<u8>) {
        let mut frame = [0u8; MAX_REQUEST_LEN];
        let len = encode_request(&mut frame, op, file, request, arg, segments, &NoMem).unwrap();
        let mut reply = [0u8; MAX_REPLY_LEN];
        let reply_len = self
            .server
            .handle(caller, &frame[..len], &mut reply, &mut self.hw);
        let decoded = decode_reply(&reply[..reply_len], segments).unwrap();
        (decoded.errno, decoded.value, decoded.arg.to_vec())
    }

    fn open(&mut self, caller: Caller) -> u32 {
        let (errno, file, _) = self.call(caller, OP_OPEN, 0, 0, &[]);
        assert_eq!(errno, 0);
        file
    }

    fn vm_create(&mut self, caller: Caller, file: u32) -> u32 {
        let (errno, _, arg) = self.call(caller, OP_IOCTL, file, DRM_IOCTL_XE_VM_CREATE, &[0; 32]);
        assert_eq!(errno, 0);
        u32_at(&arg, 12)
    }

    fn vm_destroy(&mut self, caller: Caller, file: u32, vm: u32) -> i32 {
        let mut arg = [0u8; 24];
        arg[..4].copy_from_slice(&vm.to_le_bytes());
        self.call(caller, OP_IOCTL, file, DRM_IOCTL_XE_VM_DESTROY, &arg)
            .0
    }

    fn gem_create(&mut self, caller: Caller, file: u32, export: u64) -> (i32, u32) {
        let mut arg = [0u8; 56];
        arg[8..16].copy_from_slice(&4096u64.to_le_bytes());
        arg[16..20].copy_from_slice(&1u32.to_le_bytes());
        arg[32..34].copy_from_slice(&1u16.to_le_bytes());
        let (errno, _, arg) = self.call_with(
            caller,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_GEM_CREATE,
            &arg,
            &[Segment::export(export)],
        );
        (errno, u32_at(&arg, 28))
    }
}

#[test]
fn clients_see_only_their_own_files_and_objects() {
    let mut rig = Rig::new();
    let file_a = rig.open(A);
    let file_b = rig.open(B);
    // Same ids on both sides; neither names the other's object.
    assert_eq!((file_a, file_b), (1, 1));
    assert_eq!(rig.vm_create(A, file_a), 1);
    assert_eq!(rig.vm_create(A, file_a), 2);
    assert_eq!(rig.vm_destroy(B, file_b, 2), ENOENT);
    assert_eq!(rig.vm_create(B, file_b), 1);
    assert_eq!(rig.vm_destroy(A, file_a, 2), 0);
    assert_eq!(rig.vm_destroy(B, file_b, 1), 0);
    assert_eq!(rig.vm_destroy(A, file_a, 1), 0);
    // A file id is only meaningful to the tid that opened it.
    let file_a2 = rig.open(A);
    assert_eq!(file_a2, 2);
    assert_eq!(rig.call(B, OP_CLOSE, file_a2, 0, &[]).0, EBADF);
    assert_eq!(rig.call(A, OP_CLOSE, file_a2, 0, &[]).0, 0);
}

#[test]
fn export_attachment_is_checked_against_the_attested_sender() {
    let mut rig = Rig::new();
    rig.hw
        .add_export(41, A.cell_id, A.generation, 1 << 38, 4096, true);
    let file_a = rig.open(A);
    let file_b = rig.open(B);
    // B carries A's export id: refused, the confused-deputy case.
    assert_eq!(rig.gem_create(B, file_b, 41).0, EPERM);
    let (errno, handle) = rig.gem_create(A, file_a, 41);
    assert_eq!((errno, handle), (0, 1));
    // Attachments ride only on ioctls.
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let len = encode_request(
        &mut frame,
        OP_OPEN,
        0,
        0,
        &[],
        &[Segment::export(41)],
        &NoMem,
    )
    .unwrap();
    let mut reply = [0u8; MAX_REPLY_LEN];
    let n = rig.server.handle(A, &frame[..len], &mut reply, &mut rig.hw);
    assert_eq!(decode_reply(&reply[..n], &[]).unwrap().errno, EINVAL);
}

#[test]
fn reused_tid_of_another_incarnation_inherits_nothing() {
    let mut rig = Rig::new();
    rig.hw
        .add_export(41, A.cell_id, A.generation, 1 << 38, 4096, true);
    let file = rig.open(A);
    rig.vm_create(A, file);
    rig.gem_create(A, file, 41);
    let reborn = Caller { generation: 2, ..A };
    assert_eq!(rig.vm_destroy(reborn, file, 1), EBADF);
    assert_eq!(rig.server.client_count(), 0);
    // The old incarnation's lease went with its files.
    assert!(rig.hw.events.contains(&Event::Revoke(41)));
    let fresh = rig.open(reborn);
    assert_eq!(fresh, 1);
    assert_eq!(rig.vm_destroy(reborn, fresh, 1), ENOENT);
}

#[test]
fn release_and_close_drop_client_state() {
    let mut rig = Rig::new();
    rig.hw
        .add_export(41, A.cell_id, A.generation, 1 << 38, 4096, true);
    rig.hw
        .add_export(42, B.cell_id, B.generation, 2 << 38, 4096, true);
    let file = rig.open(A);
    let file_b = rig.open(B);
    rig.vm_create(A, file);
    assert_eq!(rig.gem_create(A, file, 41).0, 0);
    assert_eq!(rig.gem_create(B, file_b, 42).0, 0);
    assert_eq!(rig.server.release_client(A.tid, &mut rig.hw), 1);
    assert_eq!(rig.hw.events.last(), Some(&Event::Revoke(41)));
    assert_eq!(rig.server.release_client(A.tid, &mut rig.hw), 0);
    assert_eq!(rig.vm_destroy(A, file, 1), EBADF);
    assert_eq!(rig.server.client_count(), 1);
    assert_eq!(rig.call(B, OP_CLOSE, 1, 0, &[]).0, 0);
    assert_eq!(rig.hw.events.last(), Some(&Event::Revoke(42)));
    assert_eq!(rig.server.client_count(), 0);
    assert_eq!(rig.call(B, OP_CLOSE, 1, 0, &[]).0, EBADF);
}

#[test]
fn bad_ops_files_and_sizes_are_refused() {
    let mut rig = Rig::new();
    let file = rig.open(A);
    assert_eq!(rig.call(A, 9, file, 0, &[]).0, EINVAL);
    assert_eq!(
        rig.call(A, OP_IOCTL, 99, DRM_IOCTL_XE_VM_CREATE, &[0; 32])
            .0,
        EBADF
    );
    // A wrong struct size is refused by the decoder, not misread.
    assert_eq!(
        rig.call(A, OP_IOCTL, file, DRM_IOCTL_XE_VM_DESTROY, &[0; 8])
            .0,
        EINVAL
    );
    let mut reply = [0u8; MAX_REPLY_LEN];
    let len = rig.server.handle(A, &[1, 0], &mut reply, &mut rig.hw);
    assert_eq!(decode_reply(&reply[..len], &[]).unwrap().errno, EINVAL);
}

#[test]
fn file_and_client_tables_are_bounded() {
    let mut rig = Rig::new();
    for _ in 0..MAX_FILES_PER_CLIENT {
        rig.open(A);
    }
    assert_eq!(rig.call(A, OP_OPEN, 0, 0, &[]).0, EMFILE);
    for tid in 1..MAX_CLIENTS {
        let caller = Caller {
            tid: 1000 + tid,
            cell_id: 50 + tid as u64,
            generation: 1,
        };
        rig.open(caller);
    }
    assert_eq!(rig.call(B, OP_OPEN, 0, 0, &[]).0, ENFILE);
    // An existing client may still open within its own limit.
    assert_eq!(rig.call(A, OP_CLOSE, 1, 0, &[]).0, 0);
    assert_eq!(rig.call(A, OP_OPEN, 0, 0, &[]).0, 0);
}
