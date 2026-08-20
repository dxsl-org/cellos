use alloc::string::String;

use api::ipc::VfsResponse;
use api::vfs_file_handles::ViVfsFileHandle;
use types::CellId;

use super::{FileHandleError, FileHandleTable, MAX_FILE_HANDLES_PER_CALLER};
use crate::caller::Caller;
use crate::dir_admission;
use crate::manager::VfsManager;

const CELL_A: Caller = Caller::principal(CellId(11), 1);
const CELL_B: Caller = Caller::principal(CellId(22), 1);
const CELL_B_NEW: Caller = Caller::principal(CellId(22), 2);
const CELL_C: Caller = Caller::principal(CellId(33), 1);

pub fn run() {
    wrong_owner_preserves_entry();
    quota_is_32();
    ids_do_not_reuse_and_fail_closed();
    purge_is_exact_to_generation();
    parent_revoke_reaps_cross_owner_children();
    owner_watch_reaps_file_handles();
    higher_generation_reaps_predecessor_and_cross_owner_files();
}

fn wrong_owner_preserves_entry() {
    let mut files = FileHandleTable::new();
    let file = files.insert(CELL_B, "/tmp/b.txt", 7).expect("file");
    assert!(!files.close(CELL_A, file));
    assert_eq!(
        files.begin_sync_read(CELL_A, file),
        Err(FileHandleError::UnknownHandle)
    );
    assert!(files.contains(file));
    assert_eq!(
        files.begin_sync_read(CELL_B, file).as_deref(),
        Ok("/tmp/b.txt")
    );
    assert!(files.finish_sync_read(CELL_B, file));
    ostd::io::println("[vfs-file-handle] wrong-owner-read-close-preserves-entry PASS");
}

fn quota_is_32() {
    let mut files = FileHandleTable::new();
    for i in 0..MAX_FILE_HANDLES_PER_CALLER {
        let path = alloc::format!("/tmp/{i}");
        files.insert(CELL_A, &path, 1).expect("within quota");
    }
    assert_eq!(files.held_by(CELL_A), MAX_FILE_HANDLES_PER_CALLER);
    assert_eq!(
        files.insert(CELL_A, "/tmp/overflow", 1),
        Err(FileHandleError::TooManyHandles)
    );
    ostd::io::println("[vfs-file-handle] quota-32-per-owner PASS");
}

fn ids_do_not_reuse_and_fail_closed() {
    let mut files = FileHandleTable::new();
    let first = files.insert(CELL_A, "/tmp/a", 1).expect("first");
    assert!(files.close(CELL_A, first));
    let second = files.insert(CELL_A, "/tmp/b", 1).expect("second");
    assert_ne!(first, second);
    files.set_next_for_test(u64::MAX, false);
    let last = files.insert(CELL_B, "/tmp/last", 2).expect("last");
    assert_eq!(last, ViVfsFileHandle(u64::MAX));
    assert_eq!(
        files.insert(CELL_B, "/tmp/exhausted", 2),
        Err(FileHandleError::Exhausted)
    );
    ostd::io::println("[vfs-file-handle] nonreuse-and-u64-exhaustion PASS");
}

fn purge_is_exact_to_generation() {
    let mut files = FileHandleTable::new();
    let old = files.insert(CELL_B, "/tmp/old", 1).expect("old");
    let new = files.insert(CELL_B_NEW, "/tmp/new", 2).expect("new");
    assert_eq!(files.purge_owner(CELL_B), 1);
    assert!(!files.contains(old));
    assert!(files.contains(new));
    ostd::io::println("[vfs-file-handle] exact-generation-purge PASS");
}

fn parent_revoke_reaps_cross_owner_children() {
    let mut files = FileHandleTable::new();
    let keep = files.insert(CELL_A, "/tmp/keep", 1).expect("keep");
    let child_a = files.insert(CELL_B, "/tmp/child-a", 7).expect("child_a");
    let child_b = files.insert(CELL_C, "/tmp/child-b", 8).expect("child_b");
    assert_eq!(files.revoke_by_parent_dirs(&[7, 8]), 2);
    assert!(files.contains(keep));
    assert!(!files.contains(child_a));
    assert!(!files.contains(child_b));
    ostd::io::println("[vfs-file-handle] parent-cross-owner-transitive-revoke PASS");
}

fn owner_watch_reaps_file_handles() {
    let mut vfs = VfsManager::new();
    let _ = vfs.dirs.on_contact(CELL_B);
    vfs.dirs.mark_attested(CELL_B);
    let dir = vfs.dirs.open_root(CELL_B, "/tmp").expect("dir");
    let file = vfs
        .files
        .insert(CELL_B, "/tmp/watched", dir.0)
        .expect("file");
    assert_eq!(
        vfs.should_watch_after_response(CELL_B, &VfsResponse::FileHandle(file)),
        Some(CELL_B.cell.0 as usize)
    );
    assert!(vfs.handle_unattributed_owner_death(CELL_B.cell.0 as usize));
    assert!(!vfs.files.contains(file));
    ostd::io::println("[vfs-file-handle] owner-watch-filehandle-cleanup PASS");
}

fn higher_generation_reaps_predecessor_and_cross_owner_files() {
    let mut vfs = VfsManager::new();
    let _ = vfs.dirs.on_contact(CELL_B);
    vfs.dirs.mark_attested(CELL_B);
    let _ = vfs.dirs.on_contact(CELL_C);
    vfs.dirs.mark_attested(CELL_C);
    let root = vfs.dirs.open_root(CELL_B, "/tmp").expect("root");
    let inherited = vfs
        .dirs
        .insert(CELL_C, String::from("/tmp"), Some(root.0))
        .expect("inherited");
    let old = vfs.files.insert(CELL_B, "/tmp/old", root.0).expect("old");
    let cross = vfs
        .files
        .insert(CELL_C, "/tmp/cross", inherited.0)
        .expect("cross");
    assert!(dir_admission::prepare_contact(&mut vfs, CELL_B_NEW));
    assert!(!vfs.files.contains(old));
    assert!(!vfs.files.contains(cross));
    assert!(vfs.dirs.dir_path(CELL_C, inherited).is_none());
    ostd::io::println("[vfs-file-handle] higher-generation-cleanup PASS");
}
