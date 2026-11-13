// SPDX-License-Identifier: MPL-2.0
//! The one VFS request the kernel sends itself: `Syscall::Rename`.
//!
//! The kernel links neither `wire` nor postcard (Phase 13 A3), so it writes
//! the postcard bytes of `wire::ipc::VfsRequest::Rename { old, new }` and reads
//! `wire::ipc::VfsResponse::{Ok, Err(u8)}` by hand. The host test below pins
//! both to postcard's own encoding of the `wire` types (`wire::ipc::encode`).
//!
//! Postcard layout: an enum is its variant index as a varint `u32`, then the
//! fields in order; a `&str` is its byte length as a varint `usize`, then the
//! UTF-8 bytes; a `u8` is one raw byte. A varint is LEB128: seven bits per
//! byte, least significant first, high bit set on every byte but the last.

/// Variant index of `VfsRequest::Rename`.
const VFS_REQUEST_RENAME: u32 = 29;
/// Variant index of `VfsResponse::Ok`.
const VFS_RESPONSE_OK: u32 = 3;
/// Variant index of `VfsResponse::Err(u8)`.
const VFS_RESPONSE_ERR: u32 = 4;

/// Longest varint postcard writes for a `u32`.
const VARINT_U32_MAX_LEN: usize = 5;

/// A decoded VFS reply to `Rename`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenameReply {
    Ok,
    Err(u8),
    /// Any other variant, or bytes postcard would not decode.
    Other,
}

/// Writes `value` as a varint at `buf[*at..]`; `None` when it does not fit.
fn put_varint(buf: &mut [u8], at: &mut usize, mut value: u64) -> Option<()> {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let slot = buf.get_mut(*at)?;
        *at += 1;
        if value == 0 {
            *slot = byte;
            return Some(());
        }
        *slot = byte | 0x80;
    }
}

fn put_str(buf: &mut [u8], at: &mut usize, s: &str) -> Option<()> {
    put_varint(buf, at, s.len() as u64)?;
    let end = at.checked_add(s.len())?;
    buf.get_mut(*at..end)?.copy_from_slice(s.as_bytes());
    *at = end;
    Some(())
}

/// Encodes `VfsRequest::Rename { old, new }` into `buf` as postcard does and
/// returns the encoded prefix; `None` when it does not fit (postcard's
/// `SerializeBufferFull`).
pub(super) fn encode_rename<'b>(old: &str, new: &str, buf: &'b mut [u8]) -> Option<&'b [u8]> {
    let mut at = 0;
    put_varint(buf, &mut at, u64::from(VFS_REQUEST_RENAME))?;
    put_str(buf, &mut at, old)?;
    put_str(buf, &mut at, new)?;
    Some(&buf[..at])
}

/// Reads a varint `u32` the way postcard does: at most five bytes, and the
/// fifth may carry only the top four bits.
fn take_varint_u32(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let mut value: u32 = 0;
    for i in 0..VARINT_U32_MAX_LEN {
        let byte = *bytes.get(i)?;
        if i == VARINT_U32_MAX_LEN - 1 && byte > 0x0f {
            return None;
        }
        value |= u32::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, &bytes[i + 1..]));
        }
    }
    None
}

/// Decodes the start of a VFS reply (trailing bytes ignored, as
/// `wire::ipc::decode` does).
pub(super) fn decode_reply(bytes: &[u8]) -> RenameReply {
    match take_varint_u32(bytes) {
        Some((VFS_RESPONSE_OK, _)) => RenameReply::Ok,
        Some((VFS_RESPONSE_ERR, rest)) => match rest.first() {
            Some(&code) => RenameReply::Err(code),
            None => RenameReply::Other,
        },
        _ => RenameReply::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};

    #[test]
    fn rename_encoding_matches_postcard() {
        let long_a = "a".repeat(127);
        let long_b = "b".repeat(128);
        let longest = "c".repeat(IPC_BUF_SIZE / 2 - 8);
        let pairs: [(&str, &str); 7] = [
            ("", ""),
            ("a", "b"),
            ("/srv/old.txt", "/srv/new.txt"),
            ("/tmp/ünï", "/tmp/çødé"),
            (&long_a, &long_b),
            (&long_b, "/x"),
            (&longest, &longest),
        ];
        for (old, new) in pairs {
            let mut ours = [0u8; IPC_BUF_SIZE];
            let mut theirs = [0u8; IPC_BUF_SIZE];
            let expected = wire::ipc::encode(&VfsRequest::Rename { old, new }, &mut theirs)
                .expect("postcard encodes");
            let got = encode_rename(old, new, &mut ours).expect("fits");
            assert_eq!(
                got,
                &expected[..],
                "Rename({} B, {} B)",
                old.len(),
                new.len()
            );
        }
    }

    #[test]
    fn rename_encoding_overflow_matches_postcard() {
        let big = "d".repeat(IPC_BUF_SIZE);
        let mut ours = [0u8; IPC_BUF_SIZE];
        let mut theirs = [0u8; IPC_BUF_SIZE];
        assert!(wire::ipc::encode(
            &VfsRequest::Rename {
                old: &big,
                new: "x"
            },
            &mut theirs
        )
        .is_err());
        assert!(encode_rename(&big, "x", &mut ours).is_none());
    }

    #[test]
    fn reply_decoding_matches_wire() {
        let mut buf = [0u8; IPC_BUF_SIZE];
        let ok = wire::ipc::encode(&VfsResponse::Ok, &mut buf).unwrap();
        assert_eq!(decode_reply(ok), RenameReply::Ok);
        for code in [0u8, 1, 0x7f, 0x80, 0xff] {
            let mut buf = [0u8; IPC_BUF_SIZE];
            let err = wire::ipc::encode(&VfsResponse::Err(code), &mut buf).unwrap();
            assert_eq!(decode_reply(err), RenameReply::Err(code));
        }
        // Trailing bytes are ignored, as `wire::ipc::decode` ignores them.
        let mut buf = [0u8; IPC_BUF_SIZE];
        let len = wire::ipc::encode(&VfsResponse::Ok, &mut buf).unwrap().len();
        assert_eq!(decode_reply(&buf[..len + 4]), RenameReply::Ok);
        // Every other reply is neither Ok nor Err.
        for other in [
            VfsResponse::Pending,
            VfsResponse::PendingHandle(3),
            VfsResponse::Data(b"x"),
            VfsResponse::Stat {
                size: 1,
                is_dir: false,
            },
        ] {
            let mut buf = [0u8; IPC_BUF_SIZE];
            let bytes = wire::ipc::encode(&other, &mut buf).unwrap();
            assert_eq!(decode_reply(bytes), RenameReply::Other);
        }
        assert_eq!(decode_reply(&[]), RenameReply::Other);
        // A truncated `Err` does not decode in postcard either.
        let mut buf = [0u8; IPC_BUF_SIZE];
        let err = wire::ipc::encode(&VfsResponse::Err(9), &mut buf).unwrap();
        let short = &err[..err.len() - 1];
        assert!(wire::ipc::decode::<VfsResponse>(short).is_err());
        assert_eq!(decode_reply(short), RenameReply::Other);
    }
}
