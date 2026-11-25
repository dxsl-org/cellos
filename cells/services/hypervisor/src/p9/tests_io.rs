//! File and directory request tests over the in-memory backend.

use super::codec::Reader;
use super::errno::*;
use super::fake::Fake;
use super::msg::*;
use super::tests::{call, errno, session, walk, Reply, ROOT};
use super::tests_walk::readdir;
use super::Server;

const AT_REMOVEDIR: u32 = 0x200;
const O_RDWR: u32 = 2;
const O_TRUNC: u32 = 0o1000;

/// Walk a new fid `fid` to the root and create `name` there, open.
fn create(s: &mut Server<Fake>, fid: u32, name: &str) -> Reply {
    walk(s, ROOT, fid, &[]);
    call(s, TLCREATE, |w| {
        w.u32(fid);
        w.string(name.as_bytes());
        w.u32(O_RDWR);
        w.u32(0o644);
        w.u32(0);
    })
}

fn write(s: &mut Server<Fake>, fid: u32, offset: u64, data: &[u8]) -> Reply {
    call(s, TWRITE, |w| {
        w.u32(fid);
        w.u64(offset);
        w.u32(data.len() as u32);
        w.bytes(data);
    })
}

fn setattr_size(s: &mut Server<Fake>, fid: u32, size: u64) -> Reply {
    call(s, TSETATTR, |w| {
        w.u32(fid);
        w.u32(1 << 3);
        w.bytes(&[0; 12]);
        w.u64(size);
        w.bytes(&[0; 32]);
    })
}

#[test]
fn a_write_past_the_end_fills_the_gap_with_zeros() {
    let mut s = session(Fake::new());
    assert_eq!(create(&mut s, 1, "f.crdownload").0, TLCREATE + 1);
    let written = write(&mut s, 1, 5, b"abc");
    assert_eq!(written, (TWRITE + 1, 3u32.to_le_bytes().to_vec()));
    let content = s.backend().content("f.crdownload");
    assert_eq!(content, Some(&b"\0\0\0\0\0abc"[..]));
    let (kind, body) = call(&mut s, TREAD, |w| {
        w.u32(1);
        w.u64(4);
        w.u32(100);
    });
    assert_eq!(kind, TREAD + 1);
    let mut r = Reader::new(&body);
    assert_eq!(r.u32(), Ok(4));
    assert_eq!(r.bytes(4), Ok(&b"\0abc"[..]));
}

#[test]
fn a_written_file_is_synced_and_closed_at_clunk() {
    let mut s = session(Fake::new());
    create(&mut s, 1, "f");
    write(&mut s, 1, 0, b"data");
    assert_eq!(s.backend().open, 1);
    assert_eq!(call(&mut s, TCLUNK, |w| w.u32(1)).0, TCLUNK + 1);
    assert_eq!((s.backend().open, s.backend().syncs), (0, 1));
    // A read-only open syncs nothing.
    walk(&mut s, ROOT, 2, &["f"]);
    call(&mut s, TLOPEN, |w| {
        w.u32(2);
        w.u32(0);
    });
    call(&mut s, TCLUNK, |w| w.u32(2));
    assert_eq!((s.backend().open, s.backend().syncs), (0, 1));
}

#[test]
fn setattr_size_empties_grows_and_refuses_to_shrink() {
    let mut fake = Fake::new();
    fake.put("f", Some(b"12345"));
    let mut s = session(fake);
    walk(&mut s, ROOT, 1, &["f"]);
    assert_eq!(setattr_size(&mut s, 1, 5).0, TSETATTR + 1);
    assert_eq!(errno(setattr_size(&mut s, 1, 2)), EOPNOTSUPP);
    assert_eq!(setattr_size(&mut s, 1, 7).0, TSETATTR + 1);
    assert_eq!(s.backend().content("f"), Some(&b"12345\0\0"[..]));
    assert_eq!(setattr_size(&mut s, 1, 0).0, TSETATTR + 1);
    assert_eq!(s.backend().content("f"), Some(&b""[..]));
    assert_eq!(s.backend().open, 0);
}

#[test]
fn lopen_with_o_trunc_empties_the_file() {
    let mut fake = Fake::new();
    fake.put("f", Some(b"old"));
    let mut s = session(fake);
    walk(&mut s, ROOT, 1, &["f"]);
    call(&mut s, TLOPEN, |w| {
        w.u32(1);
        w.u32(O_RDWR | O_TRUNC);
    });
    assert_eq!(s.backend().content("f"), Some(&b""[..]));
}

#[test]
fn renameat_keeps_the_qid_and_an_open_fid_follows() {
    let mut s = session(Fake::new());
    create(&mut s, 1, "f.crdownload");
    let before = readdir(&mut s, ROOT, 0, 4096);
    let reply = call(&mut s, TRENAMEAT, |w| {
        w.u32(ROOT);
        w.string(b"f.crdownload");
        w.u32(ROOT);
        w.string(b"f");
    });
    assert_eq!(reply.0, TRENAMEAT + 1);
    write(&mut s, 1, 0, b"after");
    assert_eq!(s.backend().content("f"), Some(&b"after"[..]));
    assert_eq!(s.backend().content("f.crdownload"), None);
    assert_eq!(readdir(&mut s, ROOT, 0, 4096).len(), before.len());
    // Into itself, and from a missing name.
    call(&mut s, TMKDIR, |w| {
        w.u32(ROOT);
        w.string(b"d");
        w.u32(0o755);
        w.u32(0);
    });
    walk(&mut s, ROOT, 2, &["d"]);
    let into_itself = call(&mut s, TRENAME, |w| {
        w.u32(2);
        w.u32(2);
        w.string(b"e");
    });
    assert_eq!(errno(into_itself), EINVAL);
    let missing = call(&mut s, TRENAMEAT, |w| {
        w.u32(ROOT);
        w.string(b"nope");
        w.u32(ROOT);
        w.string(b"x");
    });
    assert_eq!(errno(missing), ENOENT);
}

#[test]
fn unlinkat_checks_the_kind_and_emptiness() {
    let mut fake = Fake::new();
    fake.put("d", None);
    fake.put("d/f", Some(b"x"));
    fake.put("g", Some(b"y"));
    let mut s = session(fake);
    let unlink = |s: &mut Server<Fake>, name: &str, flags: u32| {
        call(s, TUNLINKAT, |w| {
            w.u32(ROOT);
            w.string(name.as_bytes());
            w.u32(flags);
        })
    };
    assert_eq!(errno(unlink(&mut s, "d", AT_REMOVEDIR)), ENOTEMPTY);
    assert_eq!(errno(unlink(&mut s, "d", 0)), EISDIR);
    assert_eq!(errno(unlink(&mut s, "g", AT_REMOVEDIR)), ENOTDIR);
    assert_eq!(errno(unlink(&mut s, "missing", 0)), ENOENT);
    assert_eq!(unlink(&mut s, "g", 0).0, TUNLINKAT + 1);
    assert_eq!(s.backend().content("g"), None);
}

#[test]
fn creating_an_existing_or_fat_invalid_name_is_refused() {
    let mut fake = Fake::new();
    fake.fat = true;
    fake.put("taken", Some(b""));
    let mut s = session(fake);
    assert_eq!(errno(create(&mut s, 1, "taken")), EEXIST);
    for name in ["a:b", "what?", "trailing.", "x|y"] {
        assert_eq!(errno(create(&mut s, 2, name)), EINVAL, "{name}");
        call(&mut s, TCLUNK, |w| w.u32(2));
    }
    let mkdir = call(&mut s, TMKDIR, |w| {
        w.u32(ROOT);
        w.string(b"taken");
        w.u32(0o755);
        w.u32(0);
    });
    assert_eq!(errno(mkdir), EEXIST);
}

#[test]
fn a_read_never_exceeds_the_reply_room() {
    let mut fake = Fake::new();
    fake.put("big", Some(&[7u8; 1000]));
    let mut s = session(fake);
    walk(&mut s, ROOT, 1, &["big"]);
    call(&mut s, TLOPEN, |w| {
        w.u32(1);
        w.u32(0);
    });
    let request = super::tests::message(TREAD, |w| {
        w.u32(1);
        w.u64(0);
        w.u32(1000);
    });
    let reply = s.handle(&request, 111);
    assert_eq!(reply.len(), 111);
    assert_eq!(Reader::new(&reply[7..]).u32(), Ok(100));
}

#[test]
fn a_new_version_closes_every_open_file() {
    let mut s = session(Fake::new());
    create(&mut s, 1, "f");
    write(&mut s, 1, 0, b"x");
    call(&mut s, TVERSION, |w| {
        w.u32(8192);
        w.string(b"9P2000.L");
    });
    assert_eq!((s.backend().open, s.backend().syncs), (0, 1));
    assert_eq!(errno(call(&mut s, TCLUNK, |w| w.u32(ROOT))), EBADF);
}
