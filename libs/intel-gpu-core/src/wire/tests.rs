use super::*;
use crate::uapi::ioctl::{DRM_IOCTL_VERSION, EFAULT, EINVAL};
use crate::uapi::UserMemory;
use alloc::vec;
use alloc::vec::Vec;

/// Client memory: one buffer at `BASE`.
struct Mem(Vec<u8>);

const BASE: u64 = 0x4000;

impl UserMemory for Mem {
    fn read(&self, addr: u64, out: &mut [u8]) -> Result<(), i32> {
        let start = addr.checked_sub(BASE).ok_or(EFAULT)? as usize;
        let bytes = self.0.get(start..start + out.len()).ok_or(EFAULT)?;
        out.copy_from_slice(bytes);
        Ok(())
    }

    fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), i32> {
        let start = addr.checked_sub(BASE).ok_or(EFAULT)? as usize;
        let bytes = self.0.get_mut(start..start + data.len()).ok_or(EFAULT)?;
        bytes.copy_from_slice(data);
        Ok(())
    }
}

fn segments() -> [Segment; 2] {
    [
        Segment {
            addr: BASE,
            len: 8,
            flags: SEG_IN,
        },
        Segment {
            addr: BASE + 0x100,
            len: 16,
            flags: SEG_OUT,
        },
    ]
}

#[test]
fn request_round_trip_exposes_only_declared_segments() {
    let mut client = Mem(vec![0u8; 0x200]);
    client.0[..8].copy_from_slice(b"instance");
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let arg = [7u8; 64];
    let len = encode_request(
        &mut frame,
        OP_IOCTL,
        3,
        DRM_IOCTL_VERSION,
        &arg,
        &segments(),
        &client,
    )
    .unwrap();
    assert_eq!(len, HEADER_LEN + 2 * DESCRIPTOR_LEN + 64 + 8);

    let request = decode_request(&frame[..len]).unwrap();
    assert_eq!(
        (request.op, request.file, request.request),
        (OP_IOCTL, 3, DRM_IOCTL_VERSION)
    );
    assert_eq!(request.arg, &arg[..]);
    let mut mem = request.memory();
    let mut out = [0u8; 4];
    assert_eq!(mem.read(BASE + 4, &mut out), Ok(()));
    assert_eq!(&out, b"ance");
    // Past the IN segment, or inside the OUT-only one: a bad user pointer.
    assert_eq!(mem.read(BASE + 6, &mut out), Err(EFAULT));
    assert_eq!(mem.read(BASE + 0x100, &mut out), Err(EFAULT));
    assert_eq!(mem.write(BASE, b"x"), Err(EFAULT));
    assert_eq!(mem.write(BASE + 0x10c, b"tail"), Ok(()));
    assert_eq!(mem.write(BASE + 0x10d, b"tail"), Err(EFAULT));

    let mut reply = [0u8; MAX_REPLY_LEN];
    let reply_len = encode_reply(&mut reply, 0, 9, &arg, Some(&mem));
    let decoded = decode_reply(&reply[..reply_len], &segments()).unwrap();
    assert_eq!(
        (decoded.errno, decoded.value, decoded.arg),
        (0, 9, &arg[..])
    );
    decoded.apply_writes(&mut client).unwrap();
    assert_eq!(&client.0[0x10c..0x110], b"tail");
}

#[test]
fn malformed_requests_are_refused() {
    let client = Mem(vec![0u8; 0x200]);
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let len = encode_request(&mut frame, OP_IOCTL, 1, 0, &[1, 2], &segments(), &client).unwrap();
    // A receive buffer may hold more than the frame; it may not hold less.
    assert_eq!(decode_request(&frame[..len + 1]).unwrap().arg, &[1, 2]);
    assert_eq!(decode_request(&frame[..len - 1]).unwrap_err(), EINVAL);
    // Reserved header word.
    let mut bad = frame;
    bad[12] = 1;
    assert_eq!(decode_request(&bad[..len]).unwrap_err(), EINVAL);
    // Unknown segment flag, and a segment with no direction.
    let mut bad = frame;
    bad[HEADER_LEN + 12] = 8;
    assert_eq!(decode_request(&bad[..len]).unwrap_err(), EINVAL);
    let mut bad = frame;
    bad[HEADER_LEN + DESCRIPTOR_LEN + 12] = 0;
    assert_eq!(decode_request(&bad[..len]).unwrap_err(), EINVAL);
    // Too many segments.
    let mut bad = frame;
    bad[1] = MAX_SEGMENTS as u8 + 1;
    assert_eq!(decode_request(&bad[..len]).unwrap_err(), EINVAL);
    assert_eq!(decode_request(&[]).unwrap_err(), EINVAL);
    // An unreadable IN range fails on the client, before anything is sent.
    let far = [Segment {
        addr: BASE + 0x1000,
        len: 4,
        flags: SEG_IN,
    }];
    assert_eq!(
        encode_request(&mut frame, OP_IOCTL, 1, 0, &[], &far, &client),
        Err(EFAULT)
    );
}

#[test]
fn export_attachments_round_trip_and_malformed_ones_are_refused() {
    let client = Mem(vec![0u8; 0x200]);
    let mut frame = [0u8; MAX_REQUEST_LEN];
    let segments = [segments()[0], Segment::export(0x1234)];
    let len = encode_request(&mut frame, OP_IOCTL, 1, 7, &[9; 8], &segments, &client).unwrap();
    // The descriptor is `{addr = id, len = 0, flags = SEG_EXPORT}` and carries no bytes.
    let at = HEADER_LEN + DESCRIPTOR_LEN;
    assert_eq!(&frame[at..at + 8], &0x1234u64.to_le_bytes());
    assert_eq!(&frame[at + 8..at + 16], &[0, 0, 0, 0, 4, 0, 0, 0]);
    let request = decode_request(&frame[..len]).unwrap();
    assert_eq!(request.exports(), [0x1234]);
    assert_eq!(request.arg, &[9; 8]);

    // A length, an extra direction flag, id 0, or an id above isize::MAX.
    for (offset, byte) in [(8, 1u8), (12, 4 | SEG_IN as u8)] {
        let mut bad = frame;
        bad[at + offset] = byte;
        assert_eq!(
            decode_request(&bad[..len]).unwrap_err(),
            EINVAL,
            "byte {offset}"
        );
    }
    for id in [0u64, 1 << 63] {
        let mut bad = frame;
        bad[at..at + 8].copy_from_slice(&id.to_le_bytes());
        assert_eq!(
            decode_request(&bad[..len]).unwrap_err(),
            EINVAL,
            "id {id:#x}"
        );
    }
    // The client side refuses to build one either.
    let bad = [Segment {
        addr: 5,
        len: 4,
        flags: SEG_EXPORT,
    }];
    assert_eq!(
        encode_request(&mut frame, OP_IOCTL, 1, 0, &[], &bad, &client),
        Err(EINVAL)
    );
}

#[test]
fn reply_write_outside_declared_out_range_is_refused_whole() {
    let declared = segments();
    let undeclared = [Segment {
        addr: BASE,
        len: 8,
        flags: SEG_OUT,
    }];
    // An empty success reply: header only.
    let frame = [0u8; HEADER_LEN];
    // A request granting the driver a write to `BASE`, answered against a client that
    // declared only `BASE + 0x100`.
    let request_frame = {
        let mut out = [0u8; 64];
        let client = Mem(vec![0u8; 0x200]);
        let len = encode_request(&mut out, OP_IOCTL, 1, 0, &[], &undeclared, &client).unwrap();
        out[..len].to_vec()
    };
    let request = decode_request(&request_frame).unwrap();
    let mut mem = request.memory();
    mem.write(BASE, b"evil").unwrap();
    let mut reply = [0u8; MAX_REPLY_LEN];
    let len = encode_reply(&mut reply, 0, 0, &[], Some(&mem));
    assert_eq!(decode_reply(&reply[..len], &declared).unwrap_err(), EFAULT);
    assert!(decode_reply(&frame, &declared).is_ok());
    // More write descriptors than any reply may carry, and a non-zero reserved byte.
    let mut bad = frame;
    bad[10] = MAX_WRITES as u8 + 1;
    assert_eq!(decode_reply(&bad, &declared).unwrap_err(), EINVAL);
    let mut bad = frame;
    bad[11] = 1;
    assert_eq!(decode_reply(&bad, &declared).unwrap_err(), EINVAL);
    assert_eq!(
        decode_reply(&reply[..len - 1], &undeclared).unwrap_err(),
        EINVAL
    );
}

#[test]
fn reply_budget_bounds_driver_writes() {
    let big = [Segment {
        addr: BASE,
        len: 8192,
        flags: SEG_OUT,
    }];
    let client = Mem(vec![0u8; 0x200]);
    let mut out = [0u8; 64];
    let len = encode_request(&mut out, OP_IOCTL, 1, 0, &[], &big, &client).unwrap();
    let request = decode_request(&out[..len]).unwrap();
    let mut mem = request.memory();
    let room = MAX_REPLY_LEN - HEADER_LEN - DESCRIPTOR_LEN;
    assert_eq!(
        mem.write(BASE, &vec![1u8; room + 1]),
        Err(crate::uapi::ioctl::ENOMEM)
    );
    assert_eq!(mem.write(BASE, &vec![1u8; room]), Ok(()));
    let mut reply = [0u8; MAX_REPLY_LEN];
    assert_eq!(
        encode_reply(&mut reply, 0, 0, &[], Some(&mem)),
        MAX_REPLY_LEN
    );
}
