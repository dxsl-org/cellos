//! Boot self-test (`test-hooks`) of the S7-2 read path on the target: paging,
//! `CHANGED` and `modified`, run against a private RamFS `/tmp` through the
//! handle-addressed dispatcher. No kernel attestation is consumed.

use alloc::format;
use alloc::vec::Vec;

use api::dir_handles::ViDirHandle;
use api::ipc::vfs_dir_page::{self, entries};
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use types::CellId;

use crate::caller::Caller;
use crate::dispatch_dirs::{handle, MAX_INLINE_PAYLOAD};
use crate::manager::VfsManager;

const CELL: Caller = Caller::principal(CellId(71), 1);
const ENTRIES: usize = 120;
/// 40-byte names: 120 entries pack to 5,040 bytes, more than one reply.
fn name(i: usize) -> alloc::string::String {
    format!("selftest-{i:04}-{}", "z".repeat(26))
}

pub fn run() {
    let mut vfs = VfsManager::new();
    let _ = vfs.dirs.on_contact(CELL);
    vfs.dirs.mark_attested(CELL);
    assert!(vfs.mkdir("/tmp/s72"));
    for i in 0..ENTRIES {
        assert!(vfs.write(&format!("/tmp/s72/{}", name(i)), b""));
    }
    let dir = vfs.dirs.open_root(CELL, "/tmp/s72").expect("dir");

    let pages = walk(&mut vfs, dir);
    let total: usize = pages.iter().map(|page| page.1.len()).sum();
    assert!(pages.len() >= 2 && total > MAX_INLINE_PAYLOAD);
    for (index, (flags, _)) in pages.iter().enumerate() {
        assert_eq!(flags & vfs_dir_page::COMPLETE != 0, index + 1 == pages.len());
        assert_eq!(flags & vfs_dir_page::CHANGED, 0);
    }
    let mut seen: Vec<Vec<u8>> = pages.iter().flat_map(|page| names_of(&page.1)).collect();
    seen.sort();
    let mut expected: Vec<Vec<u8>> = (0..ENTRIES).map(|i| name(i).into_bytes()).collect();
    expected.sort();
    assert_eq!(seen, expected);
    ostd::io::println("[vfs-dir-page] multi-page-listing-complete-only-on-last PASS");

    assert!(vfs.write("/tmp/s72/created-after-open", b""));
    let pages = walk(&mut vfs, dir);
    assert!(pages.iter().all(|(flags, _)| flags & vfs_dir_page::CHANGED != 0));
    ostd::io::println("[vfs-dir-page] child-create-sets-changed PASS");

    let first = name(0);
    let mut resp = [0u8; IPC_BUF_SIZE];
    let file = match handle(&mut vfs, CELL, &VfsRequest::OpenFileAt { dir, name: &first }, &mut resp) {
        VfsResponse::FileHandle(file) => file,
        _ => panic!("open file"),
    };
    let state = |vfs: &mut VfsManager| {
        let mut resp = [0u8; IPC_BUF_SIZE];
        match handle(vfs, CELL, &VfsRequest::FileHandleState { file }, &mut resp) {
            VfsResponse::FileState { size, modified } => (size, modified),
            _ => panic!("file state"),
        }
    };
    assert_eq!(state(&mut vfs), (0, false));
    assert!(vfs.write(&format!("/tmp/s72/{first}"), b"new"));
    assert_eq!(state(&mut vfs), (3, true));
    ostd::io::println("[vfs-dir-page] write-sets-file-modified PASS");
}

/// `(flags, packed entries)` per page, following the cursor to `COMPLETE`.
fn walk(vfs: &mut VfsManager, dir: ViDirHandle) -> Vec<(u8, Vec<u8>)> {
    let mut pages = Vec::new();
    let mut cursor = 0;
    loop {
        let mut resp = [0u8; IPC_BUF_SIZE];
        let VfsResponse::DirPage { next, flags, entries } =
            handle(vfs, CELL, &VfsRequest::ListAtPage { dir, cursor }, &mut resp)
        else {
            panic!("list page");
        };
        pages.push((flags, entries.to_vec()));
        if flags & vfs_dir_page::COMPLETE != 0 {
            return pages;
        }
        assert!(pages.len() < 16);
        cursor = next;
    }
}

fn names_of(page: &[u8]) -> Vec<Vec<u8>> {
    entries(page).map(|entry| entry.expect("well-formed").name.as_bytes().to_vec()).collect()
}
