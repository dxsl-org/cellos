//! Walk, readdir and unserved-request tests over the in-memory backend.

use alloc::string::String;
use alloc::vec::Vec;

use super::codec::{Qid, Reader};
use super::errno::*;
use super::fake::Fake;
use super::msg::*;
use super::tests::{call, errno, session, walk, Listed, Reply, ROOT};
use super::Server;

fn walk_qids(reply: Reply) -> Vec<Qid> {
    assert_eq!(reply.0, TWALK + 1);
    let mut r = Reader::new(&reply.1);
    (0..r.u16().unwrap()).map(|_| r.qid().unwrap()).collect()
}

fn root_qid(s: &mut Server<Fake>) -> Qid {
    walk_qids(walk(s, ROOT, 50, &[".."]))[0]
}

#[test]
fn walk_never_leaves_the_root() {
    let mut fake = Fake::new();
    fake.put("sub", None);
    let mut s = session(fake);
    let root = root_qid(&mut s);
    let qids = walk_qids(walk(&mut s, ROOT, 1, &["..", "..", ".."]));
    assert_eq!(qids, [root, root, root]);
    let qids = walk_qids(walk(&mut s, ROOT, 2, &["sub", "..", "..", "sub"]));
    assert_eq!((qids[1], qids[2]), (root, root));
    assert_eq!(qids[3], qids[0]);
    for name in ["sub/..", "../sub", "a\\b", "", "x\0y"] {
        assert_eq!(errno(walk(&mut s, ROOT, 3, &[name])), EINVAL, "{name:?}");
    }
    let names = [".."; 17];
    assert_eq!(errno(walk(&mut s, ROOT, 3, &names)), EINVAL);
    assert_eq!(errno(walk(&mut s, ROOT, 1, &[])), EBADF, "fid 1 is in use");
}

#[test]
fn walk_stops_at_the_first_missing_name() {
    let mut fake = Fake::new();
    fake.put("sub", None);
    fake.put("sub/file", Some(b"x"));
    let mut s = session(fake);
    assert_eq!(errno(walk(&mut s, ROOT, 1, &["missing"])), ENOENT);
    assert_eq!(walk_qids(walk(&mut s, ROOT, 1, &["sub", "nope"])).len(), 1);
    assert_eq!(errno(call(&mut s, TCLUNK, |w| w.u32(1))), EBADF);
    let qids = walk_qids(walk(&mut s, ROOT, 1, &["sub", "file", "x"]));
    assert_eq!(qids.len(), 2);
    assert_eq!(walk_qids(walk(&mut s, ROOT, 1, &["sub", "file"])).len(), 2);
    assert_eq!(errno(walk(&mut s, 1, 2, &["x"])), ENOTDIR);
}

/// Names and next offsets of one Rreaddir.
pub(super) fn readdir(s: &mut Server<Fake>, fid: u32, offset: u64, count: u32) -> Vec<Listed> {
    let (kind, body) = call(s, TREADDIR, |w| {
        w.u32(fid);
        w.u64(offset);
        w.u32(count);
    });
    assert_eq!(kind, TREADDIR + 1);
    let mut r = Reader::new(&body);
    let len = r.u32().unwrap() as usize;
    let mut r = Reader::new(r.bytes(len).unwrap());
    let mut out = Vec::new();
    while let Ok(_qid) = r.qid() {
        let next = r.u64().unwrap();
        r.u8().unwrap();
        let name = String::from_utf8(r.string().unwrap().to_vec()).unwrap();
        out.push((name, next));
    }
    out
}

#[test]
fn readdir_cookies_are_stable_across_pages() {
    let mut fake = Fake::new();
    let mut names: Vec<String> = (0..40)
        .map(|i| alloc::format!("download-{i:02}.bin"))
        .collect();
    for name in &names {
        fake.put(name, Some(b""));
    }
    let mut s = session(fake);
    walk(&mut s, ROOT, 1, &[]);
    call(&mut s, TLOPEN, |w| {
        w.u32(1);
        w.u32(0)
    });
    let mut seen = Vec::new();
    let mut offset = 0;
    loop {
        let page = readdir(&mut s, 1, offset, 200);
        if page.is_empty() {
            break;
        }
        assert!(page.len() < names.len(), "a page is bounded by count");
        let again = readdir(&mut s, 1, offset, 200);
        assert_eq!(again, page, "a page reads the same twice");
        offset = page.last().unwrap().1;
        seen.extend(page.into_iter().map(|(name, _)| name));
    }
    names.sort();
    assert_eq!(seen, names);
}

#[test]
fn unserved_requests_are_eopnotsupp_and_locks_are_granted() {
    let mut s = session(Fake::new());
    let unserved = [
        TXATTRWALK, TREADLINK, TSYMLINK, TMKNOD, TLINK, TGETLOCK, 200,
    ];
    for kind in unserved {
        let reply = call(&mut s, kind, |w| w.u32(ROOT));
        assert_eq!(errno(reply), EOPNOTSUPP, "type {kind}");
    }
    let (kind, body) = call(&mut s, TLOCK, |w| w.u32(ROOT));
    assert_eq!((kind, body), (TLOCK + 1, alloc::vec![0]));
    let (kind, body) = call(&mut s, TFLUSH, |w| w.u16(5));
    assert_eq!((kind, body.len()), (TFLUSH + 1, 0));
}
