//! `/srv/sshd` end to end through `dispatch::handle_request` over a CellosFS
//! `/srv`: path-string, handle-addressed, async, D03 and removal requests.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use std::sync::{Arc, Mutex as HostMutex};

use abi::dir_handles::ViDirHandle;
use cellos_fs::format::BLOCK_SIZE;
use wire::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use wire::proto::vfs::VfsError;

use super::tests::{lookup, OTHER_PROGRAM, RESPAWNED, SSHD, UNSIGNED_SSHD};
use crate::access::AccessTable;
use crate::caller::Caller;
use crate::manager::VfsManager;

const KEYS: &str = "/srv/sshd/authorized_keys";
const DENIED: Reply = Reply::Err(VfsError::Denied as u8);

#[derive(Debug, PartialEq)]
enum Reply {
    Ok,
    Err(u8),
    Data(Vec<u8>),
    Dir(ViDirHandle),
    Stat(u64),
    Other(String),
}

#[derive(Clone)]
struct Disk(Arc<HostMutex<Vec<[u8; BLOCK_SIZE]>>>);

impl cellos_fs::disk::BlockDevice for Disk {
    fn total_blocks(&self) -> u64 {
        self.0.lock().unwrap().len() as u64
    }
    fn read_block(
        &mut self,
        block: u64,
        out: &mut [u8; BLOCK_SIZE],
    ) -> Result<(), cellos_fs::disk::FsError> {
        out.copy_from_slice(&self.0.lock().unwrap()[block as usize]);
        Ok(())
    }
    fn write_block(
        &mut self,
        block: u64,
        data: &[u8; BLOCK_SIZE],
    ) -> Result<(), cellos_fs::disk::FsError> {
        self.0.lock().unwrap()[block as usize] = *data;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), cellos_fs::disk::FsError> {
        Ok(())
    }
}

/// `callers` meet VFS in order. A later generation of a `CellId` supersedes the
/// earlier one, exactly as a respawn does, so `RESPAWNED` goes last when used.
fn manager(callers: &[Caller]) -> VfsManager {
    let mut vfs = VfsManager::new();
    let disk = Disk(Arc::new(HostMutex::new(alloc::vec![[0; BLOCK_SIZE]; 512])));
    let srv = crate::backend_cellosfs::CellosFsBackend::formatted_for_test("/srv", disk, 512);
    let index = vfs.mounts.add_backend(Box::new(srv));
    vfs.mounts.mount("/srv", index);
    vfs.access = AccessTable::with_lookups(|_| None, lookup);
    for caller in callers {
        let _ = vfs.dirs.on_contact(*caller);
        vfs.dirs.mark_attested(*caller);
    }
    vfs
}

fn call(vfs: &mut VfsManager, caller: Caller, req: &VfsRequest<'_>) -> Reply {
    let mut request = [0u8; IPC_BUF_SIZE];
    wire::ipc::encode(req, &mut request).expect("request fits");
    let mut resp_buf = [0u8; IPC_BUF_SIZE];
    let resp = crate::dispatch::handle_request(vfs, &request, Some(caller), &mut resp_buf);
    match resp {
        VfsResponse::Ok => Reply::Ok,
        VfsResponse::Err(code) => Reply::Err(code),
        VfsResponse::Data(data) => Reply::Data(data.to_vec()),
        VfsResponse::DirHandle(dir) => Reply::Dir(dir),
        VfsResponse::Stat { size, .. } => Reply::Stat(size),
        other => Reply::Other(format!("{other:?}")),
    }
}

/// sshd's own startup: create the directory, persist a key, read it back.
fn provision(vfs: &mut VfsManager) {
    assert_eq!(call(vfs, SSHD, &VfsRequest::Mkdir("/srv/sshd")), Reply::Ok);
    let write = VfsRequest::Write {
        path: KEYS,
        content: b"ssh-ed25519 AAAA",
    };
    assert_eq!(call(vfs, SSHD, &write), Reply::Ok);
}

#[test]
fn sshd_creates_reads_lists_renames_and_unlinks() {
    let mut vfs = manager(&[SSHD]);
    provision(&mut vfs);
    assert_eq!(
        call(&mut vfs, SSHD, &VfsRequest::Stat(KEYS)),
        Reply::Stat(16)
    );
    let Reply::Data(list) = call(&mut vfs, SSHD, &VfsRequest::ListDir("/srv/sshd")) else {
        panic!("sshd lists its directory");
    };
    assert!(String::from_utf8_lossy(&list).contains("authorized_keys"));
    let open = call(
        &mut vfs,
        SSHD,
        &VfsRequest::OpenRootDir { path: "/srv/sshd" },
    );
    assert!(matches!(open, Reply::Dir(_)), "{open:?}");
    let rename = VfsRequest::Rename {
        old: KEYS,
        new: "/srv/sshd/authorized_keys.old",
    };
    assert_eq!(call(&mut vfs, SSHD, &rename), Reply::Ok);
    let unlink = VfsRequest::Unlink("/srv/sshd/authorized_keys.old");
    assert_eq!(call(&mut vfs, SSHD, &unlink), Reply::Ok);
}

#[test]
fn every_other_caller_is_refused_every_operation() {
    let mut vfs = manager(&[SSHD, OTHER_PROGRAM, UNSIGNED_SSHD, RESPAWNED]);
    provision(&mut vfs);
    for caller in [OTHER_PROGRAM, UNSIGNED_SSHD, RESPAWNED] {
        let own = format!("/srv/own-{}", caller.cell.0);
        let write_own = VfsRequest::Write {
            path: &own,
            content: b"x",
        };
        assert_eq!(call(&mut vfs, caller, &write_own), Reply::Ok, "{caller:?}");
        let requests = [
            VfsRequest::Stat(KEYS),
            VfsRequest::ListDir("/srv/sshd"),
            VfsRequest::ReadAsync { path: KEYS },
            VfsRequest::Observe { path: KEYS },
            VfsRequest::OpenRootDir { path: "/srv/sshd" },
            VfsRequest::Write {
                path: KEYS,
                content: b"ssh-ed25519 EVIL",
            },
            VfsRequest::Append {
                path: KEYS,
                content: b"\nssh-ed25519 EVIL",
            },
            VfsRequest::Mkdir("/srv/sshd/sub"),
            VfsRequest::Unlink(KEYS),
            VfsRequest::Rename {
                old: KEYS,
                new: "/srv/stolen",
            },
            VfsRequest::Rename {
                old: &own,
                new: "/srv/sshd/planted",
            },
            VfsRequest::SaveBegin { dst: KEYS },
            VfsRequest::Rmdir("/srv/sshd"),
            VfsRequest::RmdirRecursive("/srv/sshd"),
        ];
        for req in &requests {
            assert_eq!(call(&mut vfs, caller, req), DENIED, "{caller:?} {req:?}");
        }
        // A handle to `/srv` cannot be narrowed into the namespace either.
        let Reply::Dir(dir) = call(&mut vfs, caller, &VfsRequest::OpenRootDir { path: "/srv" })
        else {
            panic!("/srv opens");
        };
        let open = VfsRequest::OpenDir { dir, name: "sshd" };
        assert_eq!(call(&mut vfs, caller, &open), DENIED, "{caller:?}");
    }
    // sshd's file is untouched and nothing was planted.
    assert_eq!(
        call(&mut vfs, SSHD, &VfsRequest::Stat(KEYS)),
        Reply::Stat(16)
    );
    assert_eq!(
        call(&mut vfs, SSHD, &VfsRequest::Stat("/srv/sshd/planted")),
        Reply::Err(VfsError::Io as u8)
    );
}
