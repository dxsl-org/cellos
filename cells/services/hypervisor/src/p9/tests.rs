//! Session and codec tests over the in-memory backend.

use alloc::string::String;
use alloc::vec::Vec;

use super::codec::{header, Qid, Reader, Writer, NOTAG, QT_DIR};
use super::errno::*;
use super::fake::Fake;
use super::msg::*;
use super::{Server, MAX_MSIZE};

pub(super) const ROOT: u32 = 0;
const ROOM: usize = 1 << 20;

/// A message of `kind` whose body `body` writes.
pub(super) fn message(kind: u8, body: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::message(kind, 1);
    body(&mut w);
    w.finish()
}

/// A reply's type and body.
pub(super) type Reply = (u8, Vec<u8>);
/// A readdir entry's name and the offset after it.
pub(super) type Listed = (String, u64);

/// Send one request; the reply's type and body.
pub(super) fn call(s: &mut Server<Fake>, kind: u8, body: impl FnOnce(&mut Writer)) -> Reply {
    split(&s.handle(&message(kind, body), ROOM))
}

pub(super) fn split(reply: &[u8]) -> Reply {
    let (size, kind, _) = header(reply).expect("reply header");
    assert_eq!(size as usize, reply.len());
    (kind, reply[7..].to_vec())
}

/// The errno of an Rlerror reply.
pub(super) fn errno(reply: Reply) -> u32 {
    assert_eq!(reply.0, RLERROR, "expected Rlerror, got type {}", reply.0);
    Reader::new(&reply.1).u32().unwrap()
}

/// A session attached as fid [`ROOT`].
pub(super) fn session(fake: Fake) -> Server<Fake> {
    let mut s = Server::new(fake);
    let (kind, _) = call(&mut s, TVERSION, |w| {
        w.u32(MAX_MSIZE);
        w.string(b"9P2000.L");
    });
    assert_eq!(kind, TVERSION + 1);
    let (kind, _) = call(&mut s, TATTACH, |w| {
        w.u32(ROOT);
        w.u32(!0);
        w.string(b"");
        w.string(b"");
        w.u32(!0);
    });
    assert_eq!(kind, TATTACH + 1);
    s
}

pub(super) fn walk(s: &mut Server<Fake>, fid: u32, newfid: u32, names: &[&str]) -> Reply {
    call(s, TWALK, |w| {
        w.u32(fid);
        w.u32(newfid);
        w.u16(names.len() as u16);
        for name in names {
            w.string(name.as_bytes());
        }
    })
}

#[test]
fn codec_round_trips_every_field() {
    let qid = Qid {
        kind: QT_DIR,
        version: 7,
        path: 0x1122_3344_5566_7788,
    };
    let msg = message(TGETATTR, |w| {
        w.u8(0xAB);
        w.u16(0xBEEF);
        w.u32(0xDEAD_BEEF);
        w.u64(u64::MAX - 1);
        w.string(b"name");
        w.qid(qid);
    });
    assert_eq!(header(&msg), Some((msg.len() as u32, TGETATTR, 1)));
    let mut r = Reader::new(&msg[7..]);
    assert_eq!(r.u8(), Ok(0xAB));
    assert_eq!(r.u16(), Ok(0xBEEF));
    assert_eq!(r.u32(), Ok(0xDEAD_BEEF));
    assert_eq!(r.u64(), Ok(u64::MAX - 1));
    assert_eq!(r.string(), Ok(&b"name"[..]));
    assert_eq!(r.qid(), Ok(qid));
    assert_eq!(r.u8(), Err(EINVAL));
}

#[test]
fn version_negotiates_msize_and_names_only_9p2000_l() {
    let mut s = Server::new(Fake::new());
    let (kind, body) = call(&mut s, TVERSION, |w| {
        w.u32(1 << 24);
        w.string(b"9P2000.L");
    });
    assert_eq!(kind, TVERSION + 1);
    let mut r = Reader::new(&body);
    assert_eq!(r.u32(), Ok(MAX_MSIZE));
    assert_eq!(r.string(), Ok(&b"9P2000.L"[..]));
    let (_, body) = call(&mut s, TVERSION, |w| {
        w.u32(8192);
        w.string(b"9P2000.u");
    });
    let mut r = Reader::new(&body);
    assert_eq!((r.u32(), r.string()), (Ok(8192), Ok(&b"unknown"[..])));
    assert_eq!(s.msize(), 8192);
    let tiny = call(&mut s, TVERSION, |w| {
        w.u32(16);
        w.string(b"9P2000.L");
    });
    assert_eq!(errno(tiny), EINVAL);
}

#[test]
fn malformed_and_over_long_messages_are_refused() {
    let mut s = session(Fake::new());
    // No complete header: the reply cannot carry the tag.
    let reply = s.handle(&[9, 0, 0], ROOM);
    assert_eq!(header(&reply).map(|h| h.2), Some(NOTAG));
    assert_eq!(errno(split(&reply)), EINVAL);
    // Size smaller than a header, and larger than the bytes sent.
    let mut msg = message(TCLUNK, |w| w.u32(ROOT));
    msg[..4].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(errno(split(&s.handle(&msg, ROOM))), EINVAL);
    msg[..4].copy_from_slice(&64u32.to_le_bytes());
    assert_eq!(errno(split(&s.handle(&msg, ROOM))), EINVAL);
    // A body shorter than its fields: a walk of two names carrying one.
    let short = call(&mut s, TWALK, |w| {
        w.u32(ROOT);
        w.u32(1);
        w.u16(2);
        w.string(b"a");
    });
    assert_eq!(errno(short), EINVAL);
    // Longer than the negotiated msize.
    call(&mut s, TVERSION, |w| {
        w.u32(4096);
        w.string(b"9P2000.L");
    });
    let long = call(&mut s, TWRITE, |w| {
        w.u32(ROOT);
        w.u64(0);
        w.u32(5000);
        w.bytes(&[0; 5000]);
    });
    assert_eq!(errno(long), EINVAL);
}

#[test]
fn unknown_fids_are_ebadf() {
    let mut s = session(Fake::new());
    let getattr = call(&mut s, TGETATTR, |w| {
        w.u32(99);
        w.u64(!0)
    });
    assert_eq!(errno(getattr), EBADF);
    assert_eq!(errno(call(&mut s, TCLUNK, |w| w.u32(99))), EBADF);
    let read = call(&mut s, TREAD, |w| {
        w.u32(99);
        w.u64(0);
        w.u32(10)
    });
    assert_eq!(errno(read), EBADF);
    assert_eq!(errno(walk(&mut s, 99, 100, &[])), EBADF);
}

#[test]
fn attach_without_a_share_fails_with_eio() {
    let mut fake = Fake::new();
    fake.attach_fails = true;
    let mut s = Server::new(fake);
    let reply = call(&mut s, TATTACH, |w| {
        w.u32(ROOT);
        w.u32(!0);
        w.string(b"");
        w.string(b"");
        w.u32(!0);
    });
    assert_eq!(errno(reply), EIO);
}
