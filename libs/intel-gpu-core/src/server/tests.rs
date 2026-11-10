use super::*;
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

/// Send one frame and return `(errno, value, arg)`.
fn call(
    server: &mut XeServer,
    caller: Caller,
    op: u8,
    file: u32,
    request: u32,
    arg: &[u8],
) -> (i32, u32, Vec<u8>) {
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let len = encode_request(&mut frame, op, file, request, arg, &[], &NoMem).unwrap();
    let mut reply = [0u8; MAX_REPLY_LEN];
    let reply_len = server.handle(caller, &frame[..len], &mut reply);
    let decoded = decode_reply(&reply[..reply_len], &[] as &[Segment]).unwrap();
    (decoded.errno, decoded.value, decoded.arg.to_vec())
}

fn open(server: &mut XeServer, caller: Caller) -> u32 {
    let (errno, file, _) = call(server, caller, OP_OPEN, 0, 0, &[]);
    assert_eq!(errno, 0);
    file
}

fn vm_create(server: &mut XeServer, caller: Caller, file: u32) -> u32 {
    let (errno, _, arg) = call(
        server,
        caller,
        OP_IOCTL,
        file,
        DRM_IOCTL_XE_VM_CREATE,
        &[0; 32],
    );
    assert_eq!(errno, 0);
    u32_at(&arg, 12)
}

fn vm_destroy(server: &mut XeServer, caller: Caller, file: u32, vm: u32) -> i32 {
    let mut arg = [0u8; 24];
    arg[..4].copy_from_slice(&vm.to_le_bytes());
    call(
        server,
        caller,
        OP_IOCTL,
        file,
        DRM_IOCTL_XE_VM_DESTROY,
        &arg,
    )
    .0
}

#[test]
fn clients_see_only_their_own_files_and_objects() {
    let mut server = XeServer::new(DEVICE, &[]);
    let file_a = open(&mut server, A);
    let file_b = open(&mut server, B);
    // Same ids on both sides; neither names the other's object.
    assert_eq!((file_a, file_b), (1, 1));
    assert_eq!(vm_create(&mut server, A, file_a), 1);
    assert_eq!(vm_create(&mut server, A, file_a), 2);
    assert_eq!(vm_destroy(&mut server, B, file_b, 2), ENOENT);
    assert_eq!(vm_create(&mut server, B, file_b), 1);
    assert_eq!(vm_destroy(&mut server, A, file_a, 2), 0);
    assert_eq!(vm_destroy(&mut server, B, file_b, 1), 0);
    assert_eq!(vm_destroy(&mut server, A, file_a, 1), 0);
    // A file id is only meaningful to the tid that opened it.
    let file_a2 = open(&mut server, A);
    assert_eq!(file_a2, 2);
    assert_eq!(call(&mut server, B, OP_CLOSE, file_a2, 0, &[]).0, EBADF);
    assert_eq!(call(&mut server, A, OP_CLOSE, file_a2, 0, &[]).0, 0);
}

#[test]
fn reused_tid_of_another_incarnation_inherits_nothing() {
    let mut server = XeServer::new(DEVICE, &[]);
    let file = open(&mut server, A);
    vm_create(&mut server, A, file);
    let reborn = Caller { generation: 2, ..A };
    assert_eq!(vm_destroy(&mut server, reborn, file, 1), EBADF);
    assert_eq!(server.client_count(), 0);
    let fresh = open(&mut server, reborn);
    assert_eq!(fresh, 1);
    assert_eq!(vm_destroy(&mut server, reborn, fresh, 1), ENOENT);
}

#[test]
fn release_and_close_drop_client_state() {
    let mut server = XeServer::new(DEVICE, &[]);
    let file = open(&mut server, A);
    open(&mut server, B);
    vm_create(&mut server, A, file);
    assert_eq!(server.release_client(A.tid), 1);
    assert_eq!(server.release_client(A.tid), 0);
    assert_eq!(vm_destroy(&mut server, A, file, 1), EBADF);
    assert_eq!(server.client_count(), 1);
    assert_eq!(call(&mut server, B, OP_CLOSE, 1, 0, &[]).0, 0);
    assert_eq!(server.client_count(), 0);
    assert_eq!(call(&mut server, B, OP_CLOSE, 1, 0, &[]).0, EBADF);
}

#[test]
fn unserved_requests_and_bad_ops_are_refused() {
    static UNSERVED: [u32; 1] = [DRM_IOCTL_XE_VM_CREATE];
    let mut server = XeServer::new(DEVICE, &UNSERVED);
    let file = open(&mut server, A);
    let (errno, _, arg) = call(
        &mut server,
        A,
        OP_IOCTL,
        file,
        DRM_IOCTL_XE_VM_CREATE,
        &[0; 32],
    );
    assert_eq!((errno, arg.len()), (EOPNOTSUPP, 0));
    assert_eq!(call(&mut server, A, 9, file, 0, &[]).0, EINVAL);
    assert_eq!(
        call(
            &mut server,
            A,
            OP_IOCTL,
            99,
            DRM_IOCTL_XE_VM_CREATE,
            &[0; 32]
        )
        .0,
        EBADF
    );
    // A wrong struct size is refused by the decoder, not misread.
    assert_eq!(
        call(
            &mut server,
            A,
            OP_IOCTL,
            file,
            DRM_IOCTL_XE_VM_DESTROY,
            &[0; 8]
        )
        .0,
        EINVAL
    );
    let mut reply = [0u8; MAX_REPLY_LEN];
    let len = server.handle(A, &[1, 0], &mut reply);
    assert_eq!(decode_reply(&reply[..len], &[]).unwrap().errno, EINVAL);
}

#[test]
fn file_and_client_tables_are_bounded() {
    let mut server = XeServer::new(DEVICE, &[]);
    for _ in 0..MAX_FILES_PER_CLIENT {
        open(&mut server, A);
    }
    assert_eq!(call(&mut server, A, OP_OPEN, 0, 0, &[]).0, EMFILE);
    for tid in 1..MAX_CLIENTS {
        let caller = Caller {
            tid: 1000 + tid,
            cell_id: 50 + tid as u64,
            generation: 1,
        };
        open(&mut server, caller);
    }
    assert_eq!(call(&mut server, B, OP_OPEN, 0, 0, &[]).0, ENFILE);
    // An existing client may still open within its own limit.
    assert_eq!(call(&mut server, A, OP_CLOSE, 1, 0, &[]).0, 0);
    assert_eq!(call(&mut server, A, OP_OPEN, 0, 0, &[]).0, 0);
}
