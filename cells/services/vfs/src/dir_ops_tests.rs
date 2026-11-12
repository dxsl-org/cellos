//! `MkdirAt`, `RmdirAt` and `RenameAt` end to end through
//! `dispatch::handle_request`, from a cell that has sealed its path strings.

use alloc::boxed::Box;
use alloc::vec::Vec;
use std::sync::{Arc, Mutex as HostMutex};

use api::caller_identity::CALLER_FLAG_VFS_MUTATE;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use api::services::proto::vfs::VfsError;
use cellos_fs::disk::{BlockDevice, FsError};
use cellos_fs::format::BLOCK_SIZE;
use types::CellId;

use crate::caller::Caller;
use crate::manager::VfsManager;

const WRITER: Caller = Caller {
    flags: CALLER_FLAG_VFS_MUTATE,
    ..Caller::principal(CellId(71), 1)
};
const READER: Caller = Caller::principal(CellId(72), 1);

#[derive(Debug, PartialEq)]
enum Reply {
    Ok,
    Err(u8),
    Dir(ViDirHandle),
    Stat { is_dir: bool },
}

/// RAM block device; not `cellos_fs::disk::MemDisk`, whose `Rc` is not `Send`.
#[derive(Clone)]
struct Disk(Arc<HostMutex<Vec<[u8; BLOCK_SIZE]>>>);

impl BlockDevice for Disk {
    fn total_blocks(&self) -> u64 {
        self.0.lock().unwrap().len() as u64
    }
    fn read_block(&mut self, block: u64, out: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        out.copy_from_slice(&self.0.lock().unwrap()[block as usize]);
        Ok(())
    }
    fn write_block(&mut self, block: u64, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        self.0.lock().unwrap()[block as usize] = *data;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), FsError> {
        Ok(())
    }
}

fn manager() -> VfsManager {
    let mut vfs = VfsManager::new();
    let disk = Disk(Arc::new(HostMutex::new(alloc::vec![[0; BLOCK_SIZE]; 256])));
    let srv = crate::backend_cellosfs::CellosFsBackend::formatted_for_test("/srv", disk, 256);
    let index = vfs.mounts.add_backend(Box::new(srv));
    vfs.mounts.mount("/srv", index);
    for caller in [WRITER, READER] {
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
    }
    vfs
}

fn call(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> Reply {
    let mut request = [0u8; IPC_BUF_SIZE];
    api::ipc::encode(req, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    match crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf) {
        VfsResponse::Ok => Reply::Ok,
        VfsResponse::Err(code) => Reply::Err(code),
        VfsResponse::DirHandle(dir) => Reply::Dir(dir),
        VfsResponse::Stat { is_dir, .. } => Reply::Stat { is_dir },
        other => panic!("unexpected reply {other:?}"),
    }
}

/// A handle to `path`, then the seal: from here only handles reach the tree.
fn sealed_root(vfs: &mut VfsManager, caller: Caller, path: &str) -> ViDirHandle {
    let Reply::Dir(dir) = call(vfs, caller, &VfsRequest::OpenRootDir { path }) else {
        panic!("open {path}");
    };
    assert_eq!(call(vfs, caller, &VfsRequest::SealPaths), Reply::Ok);
    dir
}

fn stat(vfs: &mut VfsManager, dir: ViDirHandle, name: &str) -> Reply {
    call(vfs, WRITER, &VfsRequest::StatAt { dir, name })
}

fn open_dir(vfs: &mut VfsManager, dir: ViDirHandle, name: &str) -> ViDirHandle {
    match call(vfs, WRITER, &VfsRequest::OpenDir { dir, name }) {
        Reply::Dir(sub) => sub,
        other => panic!("open {name}: {other:?}"),
    }
}

#[test]
fn a_sealed_cell_creates_and_removes_directories_through_its_handle() {
    let mut vfs = manager();
    let srv = sealed_root(&mut vfs, WRITER, "/srv");
    // The path-string forms stay refused; sealing is unchanged.
    let by_path = VfsRequest::Mkdir("/srv/d");
    assert_eq!(
        call(&mut vfs, WRITER, &by_path),
        Reply::Err(VfsError::Denied as u8)
    );
    let (dir, name) = (srv, "d");
    let mkdir = VfsRequest::MkdirAt { dir, name };
    assert_eq!(call(&mut vfs, WRITER, &mkdir), Reply::Ok);
    assert_eq!(stat(&mut vfs, srv, "d"), Reply::Stat { is_dir: true });
    assert_ne!(call(&mut vfs, WRITER, &mkdir), Reply::Ok, "duplicate mkdir");

    // Non-empty directories and regular files are not removed by rmdir.
    let d = open_dir(&mut vfs, srv, "d");
    let (content, name) = (b"x".as_slice(), "f");
    let child = VfsRequest::WriteAt {
        dir: d,
        name,
        content,
    };
    assert_eq!(call(&mut vfs, WRITER, &child), Reply::Ok);
    let rmdir = VfsRequest::RmdirAt { dir, name: "d" };
    assert_eq!(
        call(&mut vfs, WRITER, &rmdir),
        Reply::Err(VfsError::Io as u8)
    );
    let rmdir_file = VfsRequest::RmdirAt { dir: d, name };
    assert_ne!(call(&mut vfs, WRITER, &rmdir_file), Reply::Ok);
    assert_eq!(stat(&mut vfs, d, "f"), Reply::Stat { is_dir: false });

    let unlink = VfsRequest::UnlinkAt { dir: d, name };
    assert_eq!(call(&mut vfs, WRITER, &unlink), Reply::Ok);
    assert_eq!(call(&mut vfs, WRITER, &rmdir), Reply::Ok);
    assert_eq!(stat(&mut vfs, srv, "d"), Reply::Err(VfsError::Io as u8));
    assert_ne!(call(&mut vfs, WRITER, &rmdir), Reply::Ok, "second rmdir");
}

#[test]
fn a_sealed_cell_renames_between_handles_and_both_names_are_checked() {
    let mut vfs = manager();
    let srv = sealed_root(&mut vfs, WRITER, "/srv");
    let (dir, name, content) = (srv, "to", b"a".as_slice());
    assert_eq!(
        call(&mut vfs, WRITER, &VfsRequest::MkdirAt { dir, name }),
        Reply::Ok
    );
    let to = open_dir(&mut vfs, srv, "to");
    let write = VfsRequest::WriteAt {
        dir,
        name: "a",
        content,
    };
    assert_eq!(call(&mut vfs, WRITER, &write), Reply::Ok);
    let rename = |old: &'static str, new: &'static str| VfsRequest::RenameAt {
        old_dir: srv,
        old,
        new_dir: to,
        new,
    };
    let mut renamed = |old, new| call(&mut vfs, WRITER, &rename(old, new));
    // A name that is not one component cannot leave the handle on either side.
    assert_eq!(renamed("a", "../b"), Reply::Err(VfsError::Denied as u8));
    assert_eq!(renamed("../a", "b"), Reply::Err(VfsError::Denied as u8));
    assert_ne!(renamed("missing", "b"), Reply::Ok);
    assert_eq!(renamed("a", "b"), Reply::Ok);
    assert_eq!(stat(&mut vfs, srv, "a"), Reply::Err(VfsError::Io as u8));
    assert_eq!(stat(&mut vfs, to, "b"), Reply::Stat { is_dir: false });
}

#[test]
fn handle_addressed_lifecycle_needs_mutation_authority_and_an_owned_handle() {
    let mut vfs = manager();
    let reader = sealed_root(&mut vfs, READER, "/srv");
    let writer = sealed_root(&mut vfs, WRITER, "/srv");
    let (dir, name, old, new) = (reader, "r", "r", "s");
    let (old_dir, new_dir) = (reader, reader);
    for req in [
        VfsRequest::MkdirAt { dir, name },
        VfsRequest::RmdirAt { dir, name },
        VfsRequest::RenameAt {
            old_dir,
            old,
            new_dir,
            new,
        },
    ] {
        let refused = call(&mut vfs, READER, &req);
        assert_eq!(refused, Reply::Err(VfsError::Denied as u8), "{req:?}");
    }
    // A handle the caller does not hold is unknown, not merely forbidden.
    let foreign = VfsRequest::MkdirAt { dir, name };
    assert_eq!(
        call(&mut vfs, WRITER, &foreign),
        Reply::Err(VfsError::Handle as u8)
    );
    let old_dir = writer;
    let mixed = VfsRequest::RenameAt {
        old_dir,
        old,
        new_dir,
        new,
    };
    assert_eq!(
        call(&mut vfs, WRITER, &mixed),
        Reply::Err(VfsError::Handle as u8)
    );
}
