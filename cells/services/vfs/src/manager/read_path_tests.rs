//! S7-2 read path: `ListAtPage` and `FileHandleState`, end to end through
//! `dispatch::handle_request`, plus backend-level paging for FAT and CellosFS.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as HostMutex};

use api::caller_identity::CALLER_FLAG_VFS_MUTATE;
use api::dir_handles::ViDirHandle;
use api::ipc::vfs_dir_page::{self, entries, EntryKind};
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;
use types::CellId;

use crate::access::{AccessTable, PathRule};
use crate::backend::FsBackend;
use crate::caller::Caller;
use crate::dirs::MAX_HANDLES_PER_CELL;
use crate::dispatch_dirs::MAX_INLINE_PAYLOAD;
use crate::file_handles::MAX_FILE_HANDLES_PER_CALLER;
use crate::manager::VfsManager;
use crate::paths::{ERR_DENIED, ERR_HANDLE, ERR_IO, ERR_QUOTA};

const READER: Caller = Caller::principal(CellId(61), 1);
const OTHER_READER: Caller = Caller::principal(CellId(63), 1);
const WRITER: Caller = Caller {
    flags: CALLER_FLAG_VFS_MUTATE,
    ..Caller::principal(CellId(62), 1)
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Reply {
    Dir(ViDirHandle),
    File(ViVfsFileHandle),
    Page { next: u32, flags: u8, entries: Vec<(EntryKind, String)>, bytes: usize },
    State { size: u64, modified: bool },
    Data(Vec<u8>),
    Ok,
    Err(u8),
    Other(String),
}

fn manager() -> VfsManager {
    let mut vfs = VfsManager::new();
    for caller in [READER, OTHER_READER, WRITER] {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
    }
    vfs
}

/// Encode, dispatch and decode one request the way the receive loop does.
fn call(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> Reply {
    let mut request = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(req, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let resp = crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf);
    let mut wire = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(&resp, &mut wire).expect("reply fits one frame");
    match api::ipc::decode::<VfsResponse>(&wire).expect("reply decodes") {
        VfsResponse::DirHandle(dir) => Reply::Dir(dir),
        VfsResponse::FileHandle(file) => Reply::File(file),
        VfsResponse::DirPage { next, flags, entries: page } => Reply::Page {
            next,
            flags,
            entries: entries(page)
                .map(|entry| entry.map(|e| (e.kind, e.name.to_string())).expect("well-formed page"))
                .collect(),
            bytes: page.len(),
        },
        VfsResponse::FileState { size, modified } => Reply::State { size, modified },
        VfsResponse::Data(data) => Reply::Data(data.to_vec()),
        VfsResponse::Ok => Reply::Ok,
        VfsResponse::Err(code) => Reply::Err(code),
        other => Reply::Other(format!("{other:?}")),
    }
}

fn open_root(vfs: &mut VfsManager, caller: Caller, path: &str) -> ViDirHandle {
    match call(vfs, caller, &VfsRequest::OpenRootDir { path }) {
        Reply::Dir(dir) => dir,
        other => panic!("open {path}: {other:?}"),
    }
}

fn open_file(vfs: &mut VfsManager, caller: Caller, dir: ViDirHandle, name: &str) -> ViVfsFileHandle {
    match call(vfs, caller, &VfsRequest::OpenFileAt { dir, name }) {
        Reply::File(file) => file,
        other => panic!("open file {name}: {other:?}"),
    }
}

fn ok(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) {
    assert_eq!(call(vfs, caller, req), Reply::Ok, "{req:?}");
}

fn state(vfs: &mut VfsManager, caller: Caller, file: ViVfsFileHandle) -> Reply {
    call(vfs, caller, &VfsRequest::FileHandleState { file })
}

struct Walk {
    entries: Vec<(EntryKind, String)>,
    /// `(flags, packed bytes)` per page, in order.
    pages: Vec<(u8, usize)>,
}

/// Follow the cursor chain from 0 until a `COMPLETE` page.
fn walk(vfs: &mut VfsManager, caller: Caller, dir: ViDirHandle) -> Walk {
    let mut walk = Walk { entries: Vec::new(), pages: Vec::new() };
    let mut cursor = 0;
    loop {
        match call(vfs, caller, &VfsRequest::ListAtPage { dir, cursor }) {
            Reply::Page { next, flags, entries, bytes } => {
                walk.entries.extend(entries);
                walk.pages.push((flags, bytes));
                if flags & vfs_dir_page::COMPLETE != 0 {
                    return walk;
                }
                cursor = next;
            }
            other => panic!("page at {cursor}: {other:?}"),
        }
        assert!(walk.pages.len() < 64, "cursor chain does not end");
    }
}

fn names(walk: &Walk) -> Vec<&str> {
    walk.entries.iter().map(|(_, name)| name.as_str()).collect()
}

/// `/tmp/big` with `count` files whose names are 40 bytes long, so the packed
/// listing is `count * 42` bytes.
fn populate(vfs: &mut VfsManager, dir: &str, count: usize) -> Vec<String> {
    ok(vfs, WRITER, &VfsRequest::Mkdir(dir));
    let mut expected: Vec<String> = (0..count).map(|i| format!("entry-{i:04}-{}", "x".repeat(29))).collect();
    for name in &expected {
        ok(vfs, WRITER, &VfsRequest::Write { path: &format!("{dir}/{name}"), content: b"" });
    }
    expected.sort();
    expected
}

#[test]
fn paging_returns_every_entry_once_and_complete_only_on_the_last_page() {
    let mut vfs = manager();
    let expected = populate(&mut vfs, "/tmp/big", 200);
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    let walk = walk(&mut vfs, READER, dir);
    let total: usize = walk.pages.iter().map(|(_, bytes)| bytes).sum();
    assert_eq!(total, 200 * 42);
    assert!(total > MAX_INLINE_PAYLOAD, "fixture must not fit one reply");
    assert!(walk.pages.len() >= 3, "{} pages", walk.pages.len());
    for (index, (flags, bytes)) in walk.pages.iter().enumerate() {
        let last = index + 1 == walk.pages.len();
        assert_eq!(flags & vfs_dir_page::COMPLETE != 0, last, "page {index}");
        assert_eq!(flags & vfs_dir_page::CHANGED, 0, "page {index}");
        assert!(*bytes <= MAX_INLINE_PAYLOAD);
        if !last {
            // Only whole entries: the next one would not have fit.
            assert!(bytes + 42 > MAX_INLINE_PAYLOAD, "page {index} stopped early at {bytes}");
        }
    }
    assert_eq!(names(&walk), expected);
    assert!(walk.entries.iter().all(|(kind, _)| *kind == EntryKind::File));
    // A second walk over the unchanged directory is identical.
    assert_eq!(names(&self::walk(&mut vfs, READER, dir)), expected);
}

#[test]
fn an_empty_directory_is_one_complete_page() {
    let mut vfs = manager();
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/empty"));
    let dir = open_root(&mut vfs, READER, "/tmp/empty");
    assert_eq!(
        call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 0 }),
        Reply::Page { next: 0, flags: vfs_dir_page::COMPLETE, entries: Vec::new(), bytes: 0 }
    );
}

#[test]
fn mount_points_are_their_own_kind_and_never_capped() {
    let mut vfs = manager();
    let root = open_root(&mut vfs, READER, "/");
    let walk = walk(&mut vfs, READER, root);
    let mut seen = names(&walk);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), walk.entries.len(), "duplicate names: {:?}", walk.entries);
    for expected in [
        (EntryKind::File, "readme.txt"),
        (EntryKind::Mount, "tmp"),
        (EntryKind::Mount, "data"),
        (EntryKind::Mount, "bin"),
        (EntryKind::Mount, "srv"),
        (EntryKind::Dir, "mnt"),
        (EntryKind::Dir, "etc"),
    ] {
        assert!(walk.entries.contains(&(expected.0, expected.1.to_string())), "{expected:?} in {:?}", walk.entries);
    }
    // A directory that exists only because a mount lies below it.
    let mnt = open_root(&mut vfs, READER, "/mnt");
    assert_eq!(walk_entries(&mut vfs, mnt), [(EntryKind::Mount, "sd".to_string())]);
}

fn walk_entries(vfs: &mut VfsManager, dir: ViDirHandle) -> Vec<(EntryKind, String)> {
    walk(vfs, READER, dir).entries
}

#[test]
fn changes_during_paging_are_reported_on_every_later_page() {
    let mut vfs = manager();
    populate(&mut vfs, "/tmp/big", 200);
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    let Reply::Page { next, flags, .. } = call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 0 }) else {
        panic!("first page");
    };
    assert_eq!(flags, 0);
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/big/aaa-new", content: b"x" });
    let mut cursor = next;
    loop {
        let Reply::Page { next, flags, .. } = call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor }) else {
            panic!("later page");
        };
        assert_ne!(flags & vfs_dir_page::CHANGED, 0);
        if flags & vfs_dir_page::COMPLETE != 0 {
            break;
        }
        cursor = next;
    }
    // Sticky for the handle; a fresh handle starts clean.
    assert_ne!(walk(&mut vfs, READER, dir).pages[0].0 & vfs_dir_page::CHANGED, 0);
    let fresh = open_root(&mut vfs, READER, "/tmp/big");
    assert!(walk(&mut vfs, READER, fresh).pages.iter().all(|(flags, _)| flags & vfs_dir_page::CHANGED == 0));
}

#[test]
fn child_create_remove_and_mkdir_mark_changed_but_content_and_reads_do_not() {
    let mut vfs = manager();
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/watch"));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/watch/a", content: b"one" });
    let changed = |vfs: &mut VfsManager, dir| walk(vfs, READER, dir).pages[0].0 & vfs_dir_page::CHANGED != 0;

    let dir = open_root(&mut vfs, READER, "/tmp/watch");
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/watch/a", content: b"two" });
    ok(&mut vfs, WRITER, &VfsRequest::Append { path: "/tmp/watch/a", content: b"!" });
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ReadAt { dir, name: "a" }), Reply::Data(b"two!".to_vec()));
    assert!(matches!(call(&mut vfs, READER, &VfsRequest::StatAt { dir, name: "a" }), Reply::Other(_)));
    assert!(!changed(&mut vfs, dir), "content writes and reads leave the listing alone");

    let mutations: [&dyn Fn(&mut VfsManager); 5] = [
        &|vfs| ok(vfs, WRITER, &VfsRequest::Write { path: "/tmp/watch/b", content: b"" }),
        &|vfs| ok(vfs, WRITER, &VfsRequest::Unlink("/tmp/watch/b")),
        &|vfs| ok(vfs, WRITER, &VfsRequest::Append { path: "/tmp/watch/c", content: b"c" }),
        &|vfs| ok(vfs, WRITER, &VfsRequest::Mkdir("/tmp/watch/d")),
        &|vfs| ok(vfs, WRITER, &VfsRequest::Rmdir("/tmp/watch/d")),
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let dir = open_root(&mut vfs, READER, "/tmp/watch");
        assert!(!changed(&mut vfs, dir));
        mutate(&mut vfs);
        assert!(changed(&mut vfs, dir), "mutation {index}");
    }
    // A handle on a removed subtree reports CHANGED too.
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/watch/e"));
    let inner = open_root(&mut vfs, READER, "/tmp/watch/e");
    let outer = open_root(&mut vfs, READER, "/tmp/watch");
    ok(&mut vfs, WRITER, &VfsRequest::RmdirRecursive("/tmp/watch/e"));
    assert!(changed(&mut vfs, outer));
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir: inner, cursor: 0 }), Reply::Err(ERR_IO));
}

#[test]
fn modified_after_every_content_or_binding_mutation_and_not_after_reads() {
    let mut vfs = manager();
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/m"));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/m/f", content: b"12345" });
    let dir = open_root(&mut vfs, READER, "/tmp/m");
    let writer_dir = open_root(&mut vfs, WRITER, "/tmp/m");

    let file = open_file(&mut vfs, READER, dir, "f");
    assert_eq!(state(&mut vfs, READER, file), Reply::State { size: 5, modified: false });
    let read = VfsRequest::ReadFileHandle { file, offset: 1, max: 3 };
    assert_eq!(call(&mut vfs, READER, &read), Reply::Data(b"234".to_vec()));
    let _ = call(&mut vfs, READER, &VfsRequest::StatAt { dir, name: "f" });
    let _ = walk(&mut vfs, READER, dir);
    let _ = call(&mut vfs, READER, &VfsRequest::ReadAt { dir, name: "f" });
    assert_eq!(state(&mut vfs, READER, file), Reply::State { size: 5, modified: false });

    // Same bytes still count: the flag records that a mutation reached the
    // path, not a content comparison.
    let mutations: [&dyn Fn(&mut VfsManager); 3] = [
        &|vfs| ok(vfs, WRITER, &VfsRequest::Write { path: "/tmp/m/f", content: b"12345" }),
        &|vfs| ok(vfs, WRITER, &VfsRequest::Append { path: "/tmp/m/f", content: b"" }),
        &|vfs| ok(vfs, WRITER, &VfsRequest::WriteAt { dir: writer_dir, name: "f", content: b"12345" }),
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let file = open_file(&mut vfs, READER, dir, "f");
        let other_dir = open_root(&mut vfs, OTHER_READER, "/tmp/m");
        let other = open_file(&mut vfs, OTHER_READER, other_dir, "f");
        mutate(&mut vfs);
        assert!(matches!(state(&mut vfs, READER, file), Reply::State { modified: true, .. }), "mutation {index}");
        assert!(matches!(state(&mut vfs, OTHER_READER, other), Reply::State { modified: true, .. }), "mutation {index}");
    }

    // Unlink: the handle cannot report a size for a path that is gone, and a
    // file recreated with identical bytes (ABA) is still reported modified.
    let file = open_file(&mut vfs, READER, dir, "f");
    ok(&mut vfs, WRITER, &VfsRequest::Unlink("/tmp/m/f"));
    assert_eq!(state(&mut vfs, READER, file), Reply::Err(ERR_IO));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/m/f", content: b"12345" });
    assert_eq!(state(&mut vfs, READER, file), Reply::State { size: 5, modified: true });

    // RmdirRecursive marks everything below the removed prefix.
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/m/sub"));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/m/sub/g", content: b"g" });
    let sub = open_root(&mut vfs, READER, "/tmp/m/sub");
    let below = open_file(&mut vfs, READER, sub, "g");
    let beside = open_file(&mut vfs, READER, dir, "f");
    ok(&mut vfs, WRITER, &VfsRequest::RmdirRecursive("/tmp/m/sub"));
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/m/sub"));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/m/sub/g", content: b"g" });
    assert_eq!(state(&mut vfs, READER, below), Reply::State { size: 1, modified: true });
    assert_eq!(state(&mut vfs, READER, beside), Reply::State { size: 5, modified: false });
}

static DENY_ALL: &[PathRule] = &[PathRule { prefix: "/", allow_read_all: false, allow_write_all: false }];

#[test]
fn permission_is_rechecked_on_every_page_and_state_query() {
    let mut vfs = manager();
    populate(&mut vfs, "/tmp/big", 200);
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    let file = open_file(&mut vfs, READER, dir, "entry-0000-xxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    let Reply::Page { next, .. } = call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 0 }) else {
        panic!("first page");
    };
    vfs.access = AccessTable::with_rules(&[], DENY_ALL);
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: next }), Reply::Err(ERR_DENIED));
    assert_eq!(state(&mut vfs, READER, file), Reply::Err(ERR_DENIED));
    vfs.access = AccessTable::new();
    assert!(matches!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: next }), Reply::Page { .. }));
    assert_eq!(state(&mut vfs, READER, file), Reply::State { size: 0, modified: false });
    // Another cell cannot use the handles, and learns nothing about them.
    assert_eq!(call(&mut vfs, OTHER_READER, &VfsRequest::ListAtPage { dir, cursor: 0 }), Reply::Err(ERR_HANDLE));
    assert_eq!(state(&mut vfs, OTHER_READER, file), Reply::Err(ERR_HANDLE));
    // A denied subtree is refused even through a handle that names it.
    let denied = vfs.dirs.open_root(READER, "/srv/cellos").expect("handle");
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir: denied, cursor: 0 }), Reply::Err(ERR_DENIED));
    // Both requests are read-only and handle-addressed: a sealed cell without
    // VfsMutate keeps them.
    ok(&mut vfs, READER, &VfsRequest::SealPaths);
    assert!(matches!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 0 }), Reply::Page { .. }));
    assert!(matches!(state(&mut vfs, READER, file), Reply::State { .. }));
}

#[test]
fn close_dir_revokes_paging_and_derived_file_state() {
    let mut vfs = manager();
    populate(&mut vfs, "/tmp/big", 3);
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    let file = open_file(&mut vfs, READER, dir, "entry-0001-xxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    ok(&mut vfs, READER, &VfsRequest::CloseDir { dir });
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 0 }), Reply::Err(ERR_HANDLE));
    assert_eq!(state(&mut vfs, READER, file), Reply::Err(ERR_HANDLE));
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    let file = open_file(&mut vfs, READER, dir, "entry-0001-xxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
    ok(&mut vfs, READER, &VfsRequest::CloseFile { file });
    assert_eq!(state(&mut vfs, READER, file), Reply::Err(ERR_HANDLE));
}

#[test]
fn paging_holds_no_handles_and_the_handle_caps_still_apply() {
    let mut vfs = manager();
    let expected = populate(&mut vfs, "/tmp/big", MAX_FILE_HANDLES_PER_CALLER + 1);
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    let (dirs_before, files_before) = (vfs.dirs.held_by(READER), vfs.files.held_by(READER));
    assert_eq!(names(&walk(&mut vfs, READER, dir)), expected);
    assert_eq!((vfs.dirs.held_by(READER), vfs.files.held_by(READER)), (dirs_before, files_before));

    let files: Vec<_> = expected[..MAX_FILE_HANDLES_PER_CALLER].iter().map(|name| open_file(&mut vfs, READER, dir, name)).collect();
    let overflow = VfsRequest::OpenFileAt { dir, name: &expected[MAX_FILE_HANDLES_PER_CALLER] };
    assert_eq!(call(&mut vfs, READER, &overflow), Reply::Err(ERR_QUOTA));
    // At the cap, paging and state queries keep working.
    assert_eq!(names(&walk(&mut vfs, READER, dir)), expected);
    for file in &files {
        assert_eq!(state(&mut vfs, READER, *file), Reply::State { size: 0, modified: false });
    }
    ok(&mut vfs, READER, &VfsRequest::CloseFile { file: files[0] });
    assert!(matches!(call(&mut vfs, READER, &overflow), Reply::File(_)));

    while vfs.dirs.held_by(READER) < MAX_HANDLES_PER_CELL {
        let _ = open_root(&mut vfs, READER, "/tmp");
    }
    assert_eq!(call(&mut vfs, READER, &VfsRequest::OpenRootDir { path: "/tmp/big" }), Reply::Err(ERR_QUOTA));
    assert_eq!(names(&walk(&mut vfs, READER, dir)), expected);
}

#[test]
fn stale_cursors_and_vanished_directories_are_io_errors_not_empty_pages() {
    let mut vfs = manager();
    populate(&mut vfs, "/tmp/big", 3);
    let dir = open_root(&mut vfs, READER, "/tmp/big");
    for cursor in [4, 1 << 29, (1 << 31) | 9] {
        assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor }), Reply::Err(ERR_IO), "{cursor}");
    }
    // Exactly at the end is an empty final page, not an error.
    assert!(matches!(
        call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 3 }),
        Reply::Page { flags: vfs_dir_page::COMPLETE, bytes: 0, .. }
    ));
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/tmp/gone"));
    let gone = open_root(&mut vfs, READER, "/tmp/gone");
    ok(&mut vfs, WRITER, &VfsRequest::Rmdir("/tmp/gone"));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/tmp/gone", content: b"now a file" });
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir: gone, cursor: 0 }), Reply::Err(ERR_IO));
}

/// A mounted directory whose backend cannot page (the GuestDisk stub's answer).
struct NoPaging;

impl FsBackend for NoPaging {
    fn get_file_ptr(&self, _: &str) -> Option<(usize, usize)> { None }
    fn list(&self, _: &str, _: &mut [u8]) -> usize { 0 }
    fn list_page(&self, _: &str, _: u32, _: &mut [u8]) -> Option<crate::backend::ListPage> { None }
    fn stat(&self, _: &str) -> Option<(u64, bool)> { Some((0, true)) }
    fn file_size(&self, _: &str) -> u64 { 0 }
    fn read_to_vec(&self, _: &str) -> Vec<u8> { Vec::new() }
    fn write(&mut self, _: &str, _: &[u8]) -> bool { false }
    fn append(&mut self, _: &str, _: &[u8]) -> bool { false }
    fn mkdir(&mut self, _: &str) -> bool { false }
    fn rmdir(&mut self, _: &str) -> bool { false }
    fn unlink(&mut self, _: &str) -> bool { false }
    fn rmdir_recursive(&mut self, _: &str) -> bool { false }
}

#[test]
fn a_backend_that_cannot_page_answers_io() {
    let mut vfs = manager();
    let backend = vfs.mounts.add_backend(Box::new(NoPaging));
    vfs.mounts.mount("/data", backend);
    let dir = open_root(&mut vfs, READER, "/data");
    assert_eq!(call(&mut vfs, READER, &VfsRequest::ListAtPage { dir, cursor: 0 }), Reply::Err(ERR_IO));
}

// ── Backend-level paging ─────────────────────────────────────────────────────

/// Page `backend` through `out_len`-byte buffers until the listing completes.
fn page_all(backend: &dyn FsBackend, path: &str, out_len: usize) -> Option<(Vec<(EntryKind, String)>, usize)> {
    let mut out = alloc::vec![0u8; out_len];
    let (mut found, mut pages, mut cursor) = (Vec::new(), 0, 0);
    loop {
        let page = backend.list_page(path, cursor, &mut out)?;
        pages += 1;
        for entry in entries(&out[..page.written]) {
            let entry = entry.expect("well-formed");
            found.push((entry.kind, entry.name.to_string()));
        }
        match page.next {
            Some(next) => cursor = next,
            None => return Some((found, pages)),
        }
    }
}

/// In-memory FAT image; reads fail once `reads_left` reaches zero (negative
/// means unlimited).
struct MemDisk {
    data: Vec<u8>,
    pos: usize,
    reads_left: Arc<AtomicI64>,
}

impl fatfs::IoBase for MemDisk {
    type Error = ();
}

impl fatfs::Read for MemDisk {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        if self.reads_left.fetch_sub(1, Ordering::SeqCst) == 0 {
            self.reads_left.store(0, Ordering::SeqCst);
            return Err(());
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl fatfs::Write for MemDisk {
    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        let n = buf.len().min(self.data.len() - self.pos);
        self.data[self.pos..self.pos + n].copy_from_slice(&buf[..n]);
        self.pos += n;
        Ok(n)
    }

    fn flush(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl fatfs::Seek for MemDisk {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, ()> {
        let target = match pos {
            fatfs::SeekFrom::Start(offset) => i64::try_from(offset).map_err(|_| ())?,
            fatfs::SeekFrom::End(delta) => self.data.len() as i64 + delta,
            fatfs::SeekFrom::Current(delta) => self.pos as i64 + delta,
        };
        if target < 0 || target as usize > self.data.len() {
            return Err(());
        }
        self.pos = target as usize;
        Ok(self.pos as u64)
    }
}

#[test]
fn fat_paging_covers_every_entry_and_an_iteration_error_is_never_a_short_listing() {
    use fatfs::Seek;
    let reads_left = Arc::new(AtomicI64::new(-1));
    let mut disk = MemDisk { data: alloc::vec![0; 4 << 20], pos: 0, reads_left: reads_left.clone() };
    fatfs::format_volume(&mut disk, fatfs::FormatVolumeOptions::new()).expect("format");
    disk.seek(fatfs::SeekFrom::Start(0)).expect("rewind");
    let fs = fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).expect("mount");
    let big = fs.root_dir().create_dir("big").expect("dir");
    // Long names: each takes several LFN slots on disk.
    let mut expected: Vec<String> = (0..150).map(|i| format!("Long Name {i:03} {}", "y".repeat(24))).collect();
    for name in &expected {
        big.create_file(name).expect("file");
    }
    big.create_dir("Sub Dir").expect("subdir");
    expected.push("Sub Dir".to_string());

    let mut out = alloc::vec![0u8; MAX_INLINE_PAYLOAD];
    let (mut found, mut pages, mut cursor) = (Vec::new(), 0, 0);
    loop {
        let page = crate::backend_fat::page_dir(&big, cursor, &mut out, &|_| false).expect("page");
        pages += 1;
        found.extend(entries(&out[..page.written]).map(|e| e.expect("well-formed")).map(|e| (e.kind, e.name.to_string())));
        match page.next {
            Some(next) => cursor = next,
            None => break,
        }
    }
    assert!(pages >= 2, "{pages} pages");
    let mut names: Vec<String> = found.iter().map(|(_, name)| name.clone()).collect();
    names.sort();
    expected.sort();
    assert_eq!(names, expected);
    assert!(found.contains(&(EntryKind::Dir, "Sub Dir".to_string())));
    assert_eq!(found.iter().filter(|(kind, _)| *kind == EntryKind::File).count(), 150);
    // Entries the overlay shadows take no cursor index.
    let skip_first = |name: &[u8]| name.starts_with(b"Long Name 000");
    let page = crate::backend_fat::page_dir(&big, 0, &mut out, &skip_first).expect("page");
    let first = entries(&out[..page.written]).next().unwrap().unwrap();
    assert_ne!(first.name, found[0].1);

    // Fail the device partway through the directory.
    reads_left.store(40, Ordering::SeqCst);
    assert_eq!(crate::backend_fat::page_dir(&big, 0, &mut out, &|_| false), None);
    reads_left.store(-1, Ordering::SeqCst);
    // Past the end is stale.
    let total = u32::try_from(found.len()).unwrap();
    assert_eq!(crate::backend_fat::page_dir(&big, total + 1, &mut out, &|_| false), None);
    assert_eq!(
        crate::backend_fat::page_dir(&big, total, &mut out, &|_| false).map(|p| (p.written, p.next)),
        Some((0, None))
    );
}

#[derive(Clone)]
struct BlockDisk(Arc<HostMutex<Vec<[u8; cellos_fs::format::BLOCK_SIZE]>>>);

impl cellos_fs::disk::BlockDevice for BlockDisk {
    fn total_blocks(&self) -> u64 {
        self.0.lock().unwrap().len() as u64
    }
    fn read_block(&mut self, block: u64, out: &mut [u8; cellos_fs::format::BLOCK_SIZE]) -> Result<(), cellos_fs::disk::FsError> {
        out.copy_from_slice(&self.0.lock().unwrap()[block as usize]);
        Ok(())
    }
    fn write_block(&mut self, block: u64, data: &[u8; cellos_fs::format::BLOCK_SIZE]) -> Result<(), cellos_fs::disk::FsError> {
        self.0.lock().unwrap()[block as usize] = *data;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), cellos_fs::disk::FsError> {
        Ok(())
    }
}

fn cellosfs(prefix: &'static str) -> crate::backend_cellosfs::CellosFsBackend<BlockDisk> {
    let disk = BlockDisk(Arc::new(HostMutex::new(alloc::vec![[0; cellos_fs::format::BLOCK_SIZE]; 128])));
    crate::backend_cellosfs::CellosFsBackend::formatted_for_test(prefix, disk, 128)
}

#[test]
fn small_buffers_page_ramfs_and_cellosfs_without_losing_entries() {
    let mut ram = crate::backend_ramfs::RamFsBackend::new();
    assert!(ram.mkdir("/tmp/d"));
    let mut srv = cellosfs("/srv");
    let wanted = ["a", "bb", "ccc", "dddd", "eeeee", "ffffff"];
    for name in wanted {
        assert!(ram.write(&format!("/tmp/d/{name}"), b"x"));
        assert!(srv.write(&format!("/srv/{name}"), b"x"));
    }
    assert!(srv.mkdir("/srv/sub"));
    for (backend, path, dirs) in [(&ram as &dyn FsBackend, "/tmp/d", 0), (&srv as &dyn FsBackend, "/srv", 1)] {
        // Ten bytes hold one or two entries, so every listing spans pages.
        let (found, pages) = page_all(backend, path, 10).expect("listing");
        assert!(pages >= 4, "{path}: {pages} pages");
        let mut names: Vec<&str> = found.iter().map(|(_, name)| name.as_str()).collect();
        names.sort();
        let mut expect: Vec<&str> = wanted.to_vec();
        if dirs == 1 {
            expect.push("sub");
        }
        expect.sort();
        assert_eq!(names, expect, "{path}");
        assert_eq!(found.iter().filter(|(kind, _)| *kind == EntryKind::Dir).count(), dirs);
        assert_eq!(backend.list_page(&format!("{path}/a"), 0, &mut [0; 64]), None, "a file is not listable");
        assert_eq!(backend.list_page(&format!("{path}/missing"), 0, &mut [0; 64]), None);
    }
}

#[test]
fn rename_marks_both_parents_and_an_open_srv_file_cannot_be_renamed_under_its_reader() {
    let mut vfs = manager();
    let srv = vfs.mounts.add_backend(Box::new(cellosfs("/srv")));
    vfs.mounts.mount("/srv", srv);
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/srv/one"));
    ok(&mut vfs, WRITER, &VfsRequest::Mkdir("/srv/two"));
    ok(&mut vfs, WRITER, &VfsRequest::Write { path: "/srv/one/f", content: b"f" });
    let one = open_root(&mut vfs, READER, "/srv/one");
    let two = open_root(&mut vfs, READER, "/srv/two");
    assert_eq!(walk_entries(&mut vfs, one), [(EntryKind::File, "f".to_string())]);
    ok(&mut vfs, WRITER, &VfsRequest::Rename { old: "/srv/one/f", new: "/srv/two/f" });
    for dir in [one, two] {
        assert_ne!(walk(&mut vfs, READER, dir).pages[0].0 & vfs_dir_page::CHANGED, 0);
    }
    // The namespace lease an open `/srv` file holds refuses the rename, so the
    // reader's handle is never renamed away from under it.
    let two = open_root(&mut vfs, READER, "/srv/two");
    let file = open_file(&mut vfs, READER, two, "f");
    assert_eq!(call(&mut vfs, WRITER, &VfsRequest::Rename { old: "/srv/two/f", new: "/srv/one/g" }), Reply::Err(ERR_DENIED));
    assert_eq!(state(&mut vfs, READER, file), Reply::State { size: 1, modified: false });
    // The positional write `WriteHandleGrant` performs reaches every handle on
    // the path, the writer's own included.
    let writer_two = open_root(&mut vfs, WRITER, "/srv/two");
    let writer_file = open_file(&mut vfs, WRITER, writer_two, "f");
    assert!(vfs.write_at("/srv/two/f", 0, b"F"));
    assert_eq!(state(&mut vfs, READER, file), Reply::State { size: 1, modified: true });
    assert_eq!(state(&mut vfs, WRITER, writer_file), Reply::State { size: 1, modified: true });
}

#[test]
fn marks_follow_fat_name_matching_and_component_boundaries() {
    use crate::paths::{may_alias, may_alias_within};
    // FAT resolves ASCII case-insensitively, so a write to either spelling is
    // a write to the file an open handle names.
    assert!(may_alias("/mnt/sd/Notes.TXT", "/mnt/sd/notes.txt"));
    assert!(!may_alias("/mnt/sd/notes.txt", "/mnt/sd/notes.txt2"));
    assert!(may_alias_within("/MNT/SD/dir/x", "/mnt/sd/dir"));
    assert!(may_alias_within("/mnt/sd/dir", "/mnt/sd/dir"));
    assert!(!may_alias_within("/mnt/sd/dirx/y", "/mnt/sd/dir"));
    assert!(!may_alias_within("/mnt/sd", "/mnt/sd/dir"));
    assert!(may_alias_within("/tmp/é/x", "/"));
    assert!(!may_alias_within("/tmp/é", "/tmp/e"));
}
