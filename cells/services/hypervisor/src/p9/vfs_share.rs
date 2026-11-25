//! The share root in the VFS service (`vfs_backend` serves 9P over it).
//!
//! The root is one directory handle, opened at the first attach and kept. An
//! operation opens the directories below it name by name (`OpenDir`), acts
//! on the last, and closes the first one it derived, which revokes the rest;
//! an open file keeps its chain until it is closed (closing a directory
//! handle revokes the files opened from it).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use abi::dir_handles::ViDirHandle;
use ostd::io::println;
use wire::ipc::vfs_dir_page::{self, EntryKind};
use wire::ipc::{VfsRequest, VfsResponse};

use super::errno::{EIO, ENOENT};
use super::vfs_io::{vfs_errno, DIR_TICKS};

/// The share's directory inside the disk (or `/tmp`).
const SHARE_DIR: &str = "Chrome";
/// Data disks mount at `/mnt/disk/<device>`.
const DISKS: &str = "/mnt/disk";
const FALLBACK: &str = "/tmp";

pub struct VfsShare {
    pub(super) tid: usize,
    pub(super) root: Option<ViDirHandle>,
    /// The root is on a FAT data disk: names follow FAT's rules.
    fat: bool,
    /// The root is in RamFS `/tmp`, which has nothing to sync.
    pub(super) volatile: bool,
    /// "unavailable" has been logged.
    told: bool,
    pub(super) send: Vec<u8>,
    pub(super) recv: Vec<u8>,
}

impl VfsShare {
    pub fn new() -> Self {
        Self {
            tid: 0,
            root: None,
            fat: false,
            volatile: false,
            told: false,
            send: vec![0; wire::ipc::IPC_BUF_SIZE],
            recv: vec![0; wire::ipc::IPC_BUF_SIZE],
        }
    }

    pub(super) fn call(
        &mut self,
        req: &VfsRequest<'_>,
        ticks: u64,
    ) -> Result<VfsResponse<'_>, u32> {
        if self.tid == 0 {
            return Err(EIO);
        }
        ostd::ipc::service_call_typed_bounded(self.tid, req, &mut self.send, &mut self.recv, ticks)
            .map_err(|_| EIO)
    }

    /// `Ok` → `()`; a refusal → its errno, `Io` read as `io`.
    pub(super) fn done(&mut self, req: &VfsRequest<'_>, io: u32) -> Result<(), u32> {
        match self.call(req, DIR_TICKS)? {
            VfsResponse::Ok => Ok(()),
            other => Err(refusal(&other, io)),
        }
    }

    fn open_root(&mut self, path: &str) -> Option<ViDirHandle> {
        match self.call(&VfsRequest::OpenRootDir { path }, DIR_TICKS) {
            Ok(VfsResponse::DirHandle(dir)) => Some(dir),
            _ => None,
        }
    }

    pub(super) fn close_dir(&mut self, dir: Option<ViDirHandle>) {
        if let Some(dir) = dir {
            let _ = self.call(&VfsRequest::CloseDir { dir }, DIR_TICKS);
        }
    }

    /// The first data disk's name, when `/mnt/disk` lists one.
    fn first_disk(&mut self) -> Option<String> {
        let dir = self.open_root(DISKS)?;
        let request = VfsRequest::ListAtPage { dir, cursor: 0 };
        let name = match self.call(&request, DIR_TICKS) {
            Ok(VfsResponse::DirPage { entries, .. }) => vfs_dir_page::entries(entries)
                .map_while(Result::ok)
                .find(|entry| matches!(entry.kind, EntryKind::Dir | EntryKind::Mount))
                .map(|entry| entry.name.to_string()),
            _ => None,
        };
        self.close_dir(Some(dir));
        name
    }

    /// Open `<base>/Chrome`, creating it when missing.
    fn open_share(&mut self, base: &str) -> Option<(ViDirHandle, String)> {
        let dir = self.open_root(base)?;
        let name = SHARE_DIR;
        let ready = match self.call(&VfsRequest::StatAt { dir, name }, DIR_TICKS) {
            Ok(VfsResponse::Stat { is_dir, .. }) => is_dir,
            _ => self.done(&VfsRequest::MkdirAt { dir, name }, EIO).is_ok(),
        };
        self.close_dir(Some(dir));
        if !ready {
            return None;
        }
        let path = alloc::format!("{base}/{SHARE_DIR}");
        Some((self.open_root(&path)?, path))
    }

    /// Open `dir` below the root: its handle, and the first handle derived
    /// on the way (`None` for the root), which the caller closes.
    pub(super) fn open_dir(
        &mut self,
        dir: &[String],
    ) -> Result<(ViDirHandle, Option<ViDirHandle>), u32> {
        let mut at = self.root.ok_or(EIO)?;
        let mut first = None;
        for name in dir {
            let opened = match self.call(&VfsRequest::OpenDir { dir: at, name }, DIR_TICKS) {
                Ok(VfsResponse::DirHandle(next)) => Ok(next),
                Ok(other) => Err(refusal(&other, ENOENT)),
                Err(errno) => Err(errno),
            };
            match opened {
                Ok(next) => {
                    at = next;
                    first.get_or_insert(next);
                }
                Err(errno) => {
                    self.close_dir(first);
                    return Err(errno);
                }
            }
        }
        Ok((at, first))
    }

    /// Run `act` on the parent directory of `path` and `path`'s last name.
    pub(super) fn in_parent<T>(
        &mut self,
        path: &[String],
        act: impl FnOnce(&mut Self, ViDirHandle, &str) -> Result<T, u32>,
    ) -> Result<T, u32> {
        let (name, parent) = path.split_last().ok_or(EIO)?;
        let (dir, first) = self.open_dir(parent)?;
        let result = act(self, dir, name);
        self.close_dir(first);
        result
    }

    /// Open the share root once: `/mnt/disk/<first disk>/Chrome`, else
    /// `/tmp/Chrome`.
    pub(super) fn attach_root(&mut self) -> Result<(), u32> {
        if self.root.is_some() {
            return Ok(());
        }
        self.tid = ostd::service::provider_tid(abi::syscall::service::VFS).unwrap_or(0);
        let share = match self.first_disk() {
            Some(disk) => self.open_share(&alloc::format!("{DISKS}/{disk}")),
            None => None,
        };
        let Some((root, path)) = share.or_else(|| self.open_share(FALLBACK)) else {
            if !self.told {
                println("[hv-x86] downloads share unavailable");
                self.told = true;
            }
            return Err(EIO);
        };
        println(&alloc::format!("[hv-x86] downloads share: {path}"));
        self.fat = path.starts_with(DISKS);
        self.volatile = !self.fat;
        self.root = Some(root);
        Ok(())
    }

    pub(super) fn fat(&self) -> bool {
        self.fat
    }
}

/// The errno of a response that is not the expected one.
pub(super) fn refusal(response: &VfsResponse<'_>, io: u32) -> u32 {
    match response {
        VfsResponse::Err(code) => vfs_errno(*code, io),
        _ => EIO,
    }
}
