use super::super::path::FileReadPlan;
use super::super::session::ReadSession;
use super::super::wire::{
    vfs_err_from_code, MAX_READ_CHUNK, VFS_ERR_DENIED, VFS_ERR_HANDLE, VFS_ERR_IO, VFS_ERR_QUOTA,
};
use super::support::{MockOps, MockReply};
use crate::ViError;
use alloc::vec;

#[test]
fn wire_error_codes_map_to_typed_errors() {
    assert_eq!(vfs_err_from_code(VFS_ERR_IO), ViError::IO);
    assert_eq!(vfs_err_from_code(VFS_ERR_QUOTA), ViError::OutOfMemory);
    assert_eq!(vfs_err_from_code(VFS_ERR_DENIED), ViError::PermissionDenied);
    assert_eq!(vfs_err_from_code(VFS_ERR_HANDLE), ViError::NotFound);
}

#[test]
fn read_chunks_until_short_reply_and_closes_handles() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(1),
        MockReply::Dir(2),
        MockReply::File(9),
        MockReply::Data(vec![b'a'; MAX_READ_CHUNK as usize]),
        MockReply::Data(b"tail".to_vec()),
        MockReply::Ok,
        MockReply::Ok,
        MockReply::Ok,
    ]);
    let mut session = ReadSession::new(&mut ops);
    let bytes = session
        .read(&FileReadPlan::parse("/etc/hosts").expect("plan"), 8192)
        .expect("read");
    let cleanup = session.cleanup();
    assert_eq!(bytes.len(), MAX_READ_CHUNK as usize + 4);
    assert_eq!(&bytes[MAX_READ_CHUNK as usize..], b"tail");
    assert_eq!(cleanup, Ok(()));
    assert!(ops.calls[0].contains("OpenRootDir"));
    assert!(ops.calls[1].contains("OpenDir"));
    assert!(ops.calls[2].contains("OpenFileAt"));
    assert!(ops.calls[3].contains("ReadFileHandle"));
    assert!(ops.calls[5].contains("CloseFile"));
    assert!(ops.calls[6].contains("CloseDir { dir: ViDirHandle(2) }"));
    assert!(ops.calls[7].contains("CloseDir { dir: ViDirHandle(1) }"));
}

#[test]
fn cleanup_closes_file_after_read_error() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(1),
        MockReply::File(9),
        MockReply::Err(2),
        MockReply::Ok,
        MockReply::Ok,
    ]);
    let mut session = ReadSession::new(&mut ops);
    let result = session.read(&FileReadPlan::parse("/hosts").expect("plan"), 64);
    let cleanup = session.cleanup();
    assert_eq!(result, Err(ViError::OutOfMemory));
    assert_eq!(cleanup, Ok(()));
    assert!(ops.calls[2].contains("ReadFileHandle"));
    assert!(ops.calls[3].contains("CloseFile"));
}

#[test]
fn cleanup_closes_directories_after_close_file_error() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(1),
        MockReply::File(9),
        MockReply::Data(b"ok".to_vec()),
        MockReply::Err(VFS_ERR_HANDLE),
        MockReply::Ok,
    ]);
    let mut session = ReadSession::new(&mut ops);
    let result = session.read(&FileReadPlan::parse("/hosts").expect("plan"), 64);
    let cleanup = session.cleanup();
    assert_eq!(result, Ok(b"ok".to_vec()));
    assert_eq!(cleanup, Err(ViError::NotFound));
    assert!(ops.calls[3].contains("CloseFile"));
    assert!(ops.calls[4].contains("CloseDir"));
}

#[test]
fn read_refuses_to_grow_past_bound_and_still_closes_handles() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(1),
        MockReply::File(9),
        MockReply::DataLen(MAX_READ_CHUNK as usize, b'a'),
        MockReply::DataLen(1, b'b'),
        MockReply::Ok,
        MockReply::Ok,
    ]);
    let mut session = ReadSession::new(&mut ops);
    let result = session.read(&FileReadPlan::parse("/hosts").expect("plan"), 4000);
    let cleanup = session.cleanup();
    assert_eq!(result, Err(ViError::OutOfMemory));
    assert_eq!(cleanup, Ok(()));
    assert!(ops.calls[2].contains("ReadFileHandle"));
    assert!(ops.calls[3].contains("max: 1"));
    assert!(ops.calls[4].contains("CloseFile"));
}

#[test]
fn read_uses_requested_bound_for_followup_chunks() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(1),
        MockReply::File(9),
        MockReply::DataLen(MAX_READ_CHUNK as usize, b'a'),
        MockReply::DataLen(17, b'b'),
        MockReply::Ok,
        MockReply::Ok,
    ]);
    let mut session = ReadSession::new(&mut ops);
    let bytes = session
        .read(
            &FileReadPlan::parse("/hosts").expect("plan"),
            MAX_READ_CHUNK as usize + 17,
        )
        .expect("read");
    let cleanup = session.cleanup();
    assert_eq!(bytes.len(), MAX_READ_CHUNK as usize + 17);
    assert_eq!(cleanup, Ok(()));
    assert!(ops.calls[4].contains("max: 17"));
}

#[test]
fn read_accepts_exact_limit_after_empty_probe() {
    let mut ops = MockOps::new(vec![
        MockReply::Dir(1),
        MockReply::File(9),
        MockReply::DataLen(MAX_READ_CHUNK as usize, b'a'),
        MockReply::DataLen(0, b'z'),
        MockReply::Ok,
        MockReply::Ok,
    ]);
    let mut session = ReadSession::new(&mut ops);
    let bytes = session
        .read(
            &FileReadPlan::parse("/hosts").expect("plan"),
            MAX_READ_CHUNK as usize,
        )
        .expect("read");
    let cleanup = session.cleanup();
    assert_eq!(bytes.len(), MAX_READ_CHUNK as usize);
    assert_eq!(cleanup, Ok(()));
    assert!(ops.calls[3].contains("max: 1"));
    assert!(ops.calls[4].contains("CloseFile"));
}
