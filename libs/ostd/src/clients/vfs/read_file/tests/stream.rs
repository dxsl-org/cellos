use super::super::super::stream::{
    close_dir, close_file, file_state, list_page, open_file_at, open_root_dir, read_chunk,
    VfsFileState, VFS_DIR_PAGE_MAX, VFS_READ_CHUNK_MAX,
};
use super::support::{MockOps, MockReply};
use crate::ViError;
use alloc::vec;
use alloc::vec::Vec;
use api::dir_handles::ViDirHandle;
use api::ipc::vfs_dir_page::{push_entry, EntryKind, CHANGED, COMPLETE};
use api::vfs_file_handles::ViVfsFileHandle;

fn packed(entries: &[(EntryKind, &str)]) -> Vec<u8> {
    let mut out = vec![0u8; VFS_DIR_PAGE_MAX];
    let mut pos = 0;
    for (kind, name) in entries {
        pos = push_entry(&mut out, pos, *kind, name.as_bytes()).unwrap().unwrap();
    }
    out.truncate(pos);
    out
}

#[test]
fn list_pages_follow_the_cursor_and_surface_both_flags() {
    let mut ops = MockOps::new(vec![
        MockReply::Page { next: 7, flags: 0, entries: packed(&[(EntryKind::File, "a.txt"), (EntryKind::Mount, "tmp")]) },
        MockReply::Page { next: 0, flags: COMPLETE | CHANGED, entries: packed(&[(EntryKind::Dir, "src")]) },
    ]);
    let dir = ViDirHandle(3);
    let mut out = [0u8; VFS_DIR_PAGE_MAX];
    let first = list_page(&mut ops, dir, 0, &mut out).expect("first page");
    assert_eq!((first.next, first.complete, first.changed), (7, false, false));
    let names: Vec<(EntryKind, &str)> = first.entries().map(|e| (e.kind, e.name)).collect();
    assert_eq!(names, [(EntryKind::File, "a.txt"), (EntryKind::Mount, "tmp")]);
    let next = first.next;
    let second = list_page(&mut ops, dir, next, &mut out).expect("second page");
    assert!(second.complete && second.changed);
    assert_eq!(second.entries().map(|e| e.name).collect::<Vec<_>>(), ["src"]);
    assert_eq!(ops.calls, [
        "ListAtPage { dir: ViDirHandle(3), cursor: 0 }",
        "ListAtPage { dir: ViDirHandle(3), cursor: 7 }",
    ]);
}

#[test]
fn a_page_that_is_malformed_or_does_not_fit_is_an_error_not_a_short_listing() {
    let mut ops = MockOps::new(vec![
        MockReply::Page { next: 1, flags: 0, entries: b"\x00\x02..".to_vec() },
        MockReply::Page { next: 1, flags: 0, entries: b"\x09\x01a".to_vec() },
        MockReply::Page { next: 1, flags: 0, entries: packed(&[(EntryKind::File, "abcdef")]) },
        MockReply::Err(3),
        MockReply::Err(4),
        MockReply::Err(1),
        MockReply::Ok,
    ]);
    let dir = ViDirHandle(1);
    let mut out = [0u8; VFS_DIR_PAGE_MAX];
    assert_eq!(list_page(&mut ops, dir, 0, &mut out).err(), Some(ViError::IO));
    assert_eq!(list_page(&mut ops, dir, 0, &mut out).err(), Some(ViError::IO));
    assert_eq!(list_page(&mut ops, dir, 0, &mut [0u8; 4]).err(), Some(ViError::OutOfMemory));
    assert_eq!(list_page(&mut ops, dir, 0, &mut out).err(), Some(ViError::PermissionDenied));
    assert_eq!(list_page(&mut ops, dir, 0, &mut out).err(), Some(ViError::NotFound));
    assert_eq!(list_page(&mut ops, dir, 0, &mut out).err(), Some(ViError::IO));
    assert_eq!(list_page(&mut ops, dir, 0, &mut out).err(), Some(ViError::IO));
}

#[test]
fn read_chunk_is_bounded_by_the_buffer_and_the_inline_limit() {
    let mut ops = MockOps::new(vec![
        MockReply::DataLen(VFS_READ_CHUNK_MAX, b'r'),
        MockReply::Data(b"tail".to_vec()),
        MockReply::Data(Vec::new()),
        MockReply::DataLen(9, b'x'),
        MockReply::Err(3),
        MockReply::Ok,
    ]);
    let file = ViVfsFileHandle(5);
    let mut big = vec![0u8; 3 * VFS_READ_CHUNK_MAX];
    assert_eq!(read_chunk(&mut ops, file, 0, &mut big), Ok(VFS_READ_CHUNK_MAX));
    assert!(big[..VFS_READ_CHUNK_MAX].iter().all(|&b| b == b'r'));
    let mut small = [0u8; 8];
    assert_eq!(read_chunk(&mut ops, file, 4000, &mut small), Ok(4));
    assert_eq!(&small[..4], b"tail");
    assert_eq!(read_chunk(&mut ops, file, 4004, &mut small), Ok(0));
    // More than asked for is a protocol error, never a buffer overrun.
    assert_eq!(read_chunk(&mut ops, file, 0, &mut small), Err(ViError::IO));
    assert_eq!(read_chunk(&mut ops, file, 0, &mut small), Err(ViError::PermissionDenied));
    assert_eq!(read_chunk(&mut ops, file, 0, &mut small), Err(ViError::IO));
    // An empty buffer asks the service nothing.
    assert_eq!(read_chunk(&mut ops, file, 0, &mut []), Ok(0));
    assert_eq!(ops.calls.len(), 6);
    assert_eq!(ops.calls[0], "ReadFileHandle { file: ViVfsFileHandle(5), offset: 0, max: 4000 }");
    assert_eq!(ops.calls[1], "ReadFileHandle { file: ViVfsFileHandle(5), offset: 4000, max: 8 }");
}

#[test]
fn file_state_and_handle_lifecycle_map_replies_and_errors() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(2),
        MockReply::File(9),
        MockReply::State { size: 25 << 20, modified: false },
        MockReply::State { size: 1, modified: true },
        MockReply::Err(4),
        MockReply::Data(Vec::new()),
        MockReply::Ok,
        MockReply::Err(4),
        MockReply::Ok,
        MockReply::Err(3),
    ]);
    let dir = open_root_dir(&mut ops, "/data/rg").expect("dir");
    assert_eq!(dir, ViDirHandle(2));
    let file = open_file_at(&mut ops, dir, "big.txt").expect("file");
    assert_eq!(file_state(&mut ops, file), Ok(VfsFileState { size: 25 << 20, modified: false }));
    assert_eq!(file_state(&mut ops, file), Ok(VfsFileState { size: 1, modified: true }));
    assert_eq!(file_state(&mut ops, file), Err(ViError::NotFound));
    assert_eq!(file_state(&mut ops, file), Err(ViError::IO));
    assert_eq!(close_file(&mut ops, file), Ok(()));
    assert_eq!(close_file(&mut ops, file), Err(ViError::NotFound));
    assert_eq!(close_dir(&mut ops, dir), Ok(()));
    assert_eq!(open_root_dir(&mut ops, "/srv/cellos"), Err(ViError::PermissionDenied));
    assert_eq!(ops.calls[0], "OpenRootDir { path: \"/data/rg\" }");
    assert_eq!(ops.calls[1], "OpenFileAt { dir: ViDirHandle(2), name: \"big.txt\" }");
    assert_eq!(ops.calls[2], "FileHandleState { file: ViVfsFileHandle(9) }");
    assert_eq!(ops.calls[6], "CloseFile { file: ViVfsFileHandle(9) }");
    assert_eq!(ops.calls[8], "CloseDir { dir: ViDirHandle(2) }");
}
