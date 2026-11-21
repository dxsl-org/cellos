//! Serving grant-addressed operations (read/write/sync over memory grants).

use abi::dir_handles::ViDirHandle;
use alloc::string::String;
use wire::ipc::{vfs_dir_page, VfsResponse, IPC_BUF_SIZE};
use wire::proto::vfs::VfsError;
use wire::vfs_file_handles::ViVfsFileHandle;

use crate::caller::Caller;
use crate::dispatch_dirs::MAX_INLINE_PAYLOAD;
use crate::fs_error::FsError;
use crate::manager::VfsManager;

pub(crate) fn read_handle_grant<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    file: ViVfsFileHandle,
    offset: u64,
    size: usize,
    grant: usize,
) -> VfsResponse<'a> {
    let path = match vfs.files.path_of(caller, file) {
        Some(p) => alloc::string::String::from(p),
        None => return VfsResponse::Err(VfsError::Handle as u8),
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if !matches!(vfs.stat(&path), Ok(Some((_, false)))) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let max_read = size.min(wire::ipc::VFS_GRANT_CHUNK);
    match ostd::grant::with_shared_bytes_mut(grant, |buf| {
        let n = max_read.min(buf.len());
        if n == 0 {
            Ok(0)
        } else {
            // A backend without ranged reads moves nothing, as it always has;
            // a device failure is an error, never a short grant.
            match vfs.read_at(&path, offset, &mut buf[..n]) {
                Err(FsError::Unsupported) => Ok(0),
                read => read,
            }
        }
    }) {
        Some(Ok(bytes)) if vfs.healthy(&path) => {
            if bytes < max_read && crate::access::is_guest_disk_path(&path) {
                ostd::io::println(&alloc::format!(
                    "[vfs] guest_disk short read: path={} offset={} got={} want={}",
                    path,
                    offset,
                    bytes,
                    max_read
                ));
            }
            VfsResponse::GrantDone { bytes }
        }
        _ => VfsResponse::Err(VfsError::Io as u8),
    }
}

pub(crate) fn write_handle_grant<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    file: ViVfsFileHandle,
    offset: u64,
    grant: usize,
    bytes: usize,
) -> VfsResponse<'a> {
    let path = match vfs.files.path_of(caller, file) {
        Some(p) => alloc::string::String::from(p),
        None => return VfsResponse::Err(VfsError::Handle as u8),
    };
    if !vfs.access.can_write(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if bytes == 0 {
        return VfsResponse::GrantDone { bytes: 0 };
    }
    if bytes > wire::ipc::VFS_GRANT_CHUNK {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let file_len = match vfs.stat(&path) {
        Ok(Some((len, false))) => len,
        _ => return VfsResponse::Err(VfsError::Io as u8),
    };
    let end = match offset.checked_add(bytes as u64) {
        Some(e) => e,
        None => return VfsResponse::Err(VfsError::Io as u8),
    };
    let is_guest_disk = crate::access::is_guest_disk_path(&path);
    if is_guest_disk && end > file_len {
        ostd::io::println(&alloc::format!(
            "[vfs] guest_disk write beyond capacity: path={} offset={} bytes={} end={} file_len={}",
            path,
            offset,
            bytes,
            end,
            file_len
        ));
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    let new_len = file_len.max(end);
    let refunded = if vfs.quota.writer_of(&path) == Some(caller.cell) {
        file_len
    } else {
        0
    };
    let net_charge = if is_guest_disk {
        0
    } else {
        new_len.saturating_sub(refunded)
    };
    if net_charge > 0 && !vfs.quota.can_charge(caller.cell, net_charge) {
        return VfsResponse::Err(VfsError::Quota as u8);
    }

    let written = ostd::grant::with_shared_bytes(grant, |buf| {
        buf.len() >= bytes && vfs.write_at(&path, offset, &buf[..bytes]).is_ok()
    });
    if written != Some(true) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if !is_guest_disk {
        vfs.quota.release_path(&path, file_len);
        let _ = vfs.quota.charge(caller.cell, new_len);
        vfs.quota.set_writer(&path, caller.cell);
    }
    VfsResponse::GrantDone { bytes }
}

pub(crate) fn sync_handle<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    file: ViVfsFileHandle,
) -> VfsResponse<'a> {
    let path = match vfs.files.path_of(caller, file) {
        Some(p) => alloc::string::String::from(p),
        None => return VfsResponse::Err(VfsError::Handle as u8),
    };
    if !vfs.access.can_write(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if vfs.sync(&path).is_ok() {
        VfsResponse::Ok
    } else {
        VfsResponse::Err(VfsError::Io as u8)
    }
}

pub(crate) fn stat_path<'a>(vfs: &VfsManager, caller: Caller, path: &str) -> VfsResponse<'a> {
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    match vfs.stat(path) {
        Ok(Some((size, is_dir))) => VfsResponse::Stat { size, is_dir },
        Ok(None) | Err(_) => VfsResponse::Err(VfsError::Io as u8),
    }
}

pub(crate) fn list_at<'a>(
    vfs: &VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    let Some(path) = vfs.dirs.dir_path(caller, dir).map(String::from) else {
        return VfsResponse::Err(VfsError::Handle as u8);
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if !matches!(vfs.stat(&path), Ok(Some((_, true)))) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let listed = vfs.list_dir(&path, &mut resp_buf[..MAX_INLINE_PAYLOAD]);
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let Ok(n) = listed else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    VfsResponse::Data(&resp_buf[..n])
}

/// One `ListAtPage` reply. Authorization, health and directory kind are
/// checked again on every page, so a listing cannot outlive a policy change,
/// and only a complete walk of the cursor chain reports `COMPLETE`.
pub(crate) fn list_at_page<'a>(
    vfs: &VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    cursor: u32,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    let Some(path) = vfs.dirs.dir_path(caller, dir).map(String::from) else {
        return VfsResponse::Err(VfsError::Handle as u8);
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if !matches!(vfs.stat(&path), Ok(Some((_, true)))) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let Some(page) = vfs.list_page(&path, cursor, &mut resp_buf[..MAX_INLINE_PAYLOAD]) else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let changed = vfs
        .dirs
        .owned(caller, dir)
        .is_some_and(|entry| entry.changed);
    let mut flags = if changed { vfs_dir_page::CHANGED } else { 0 };
    let next = match page.next {
        Some(next) => next,
        None => {
            flags |= vfs_dir_page::COMPLETE;
            0
        }
    };
    VfsResponse::DirPage {
        next,
        flags,
        entries: &resp_buf[..page.written],
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;
    use crate::backend::{BackendHealth, FsBackend};
    use alloc::{boxed::Box, vec::Vec};
    use core::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    struct Faulted(Arc<AtomicBool>);
    impl FsBackend for Faulted {
        fn health(&self) -> BackendHealth {
            if self.0.load(Ordering::SeqCst) {
                BackendHealth::Poisoned
            } else {
                BackendHealth::Ready
            }
        }
        fn list(&self, _: &str, _: &mut [u8]) -> Result<usize, FsError> {
            self.0.store(true, Ordering::SeqCst);
            Err(FsError::Io)
        }
        fn list_page(
            &self,
            _: &str,
            _: u32,
            _: &mut [u8],
        ) -> Result<crate::backend::ListPage, FsError> {
            self.0.store(true, Ordering::SeqCst);
            Err(FsError::Io)
        }
        fn stat(&self, path: &str) -> Result<Option<(u64, bool)>, FsError> {
            Ok(Some((8, path == "/data")))
        }
        fn file_size(&self, _: &str) -> Result<u64, FsError> {
            Ok(8)
        }
        fn read_to_vec(&self, _: &str) -> Result<Vec<u8>, FsError> {
            self.0.store(true, Ordering::SeqCst);
            Err(FsError::Io)
        }
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> Result<usize, FsError> {
            self.0.store(true, Ordering::SeqCst);
            Err(FsError::Io)
        }
        fn write(&mut self, _: &str, _: &[u8]) -> Result<(), FsError> {
            Err(FsError::Unsupported)
        }
        fn append(&mut self, _: &str, _: &[u8]) -> Result<(), FsError> {
            Err(FsError::Unsupported)
        }
        fn mkdir(&mut self, _: &str) -> Result<(), FsError> {
            Err(FsError::Unsupported)
        }
        fn rmdir(&mut self, _: &str) -> Result<(), FsError> {
            Err(FsError::Unsupported)
        }
        fn unlink(&mut self, _: &str) -> Result<(), FsError> {
            Err(FsError::Unsupported)
        }
        fn rmdir_recursive(&mut self, _: &str) -> Result<(), FsError> {
            Err(FsError::Unsupported)
        }
    }
    #[test]
    fn faulting_ranged_read_unwinds_pin_and_does_not_fallback_or_issue_empty_data() {
        let mut vfs = VfsManager::new();
        let poisoned = Arc::new(AtomicBool::new(false));
        let backend = vfs.mounts.add_backend(Box::new(Faulted(poisoned.clone())));
        vfs.mounts.mount("/data", backend);
        let caller = Caller {
            flags: abi::caller_identity::CALLER_FLAG_VFS_MUTATE,
            ..Caller::principal(types::CellId(44), 1)
        };
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        let dir = vfs.dirs.open_root(caller, "/data").unwrap();
        let file = vfs.files.insert(caller, "/data/a", dir.0).unwrap();
        let mut buffer = [0; IPC_BUF_SIZE];
        assert!(matches!(
            crate::dispatch_file_handles::read_file(&mut vfs, caller, file, 0, 8, &mut buffer),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        assert!(poisoned.load(Ordering::SeqCst));
        assert!(matches!(
            write_handle_grant(&mut vfs, caller, file, 0, 0, 0),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        assert!(matches!(
            stat_path(&vfs, caller, "/data/a"),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        assert!(matches!(
            crate::dispatch_file_handles::close_file(&mut vfs, caller, file),
            VfsResponse::Ok
        ));
    }
    #[test]
    fn faulting_list_is_io_not_empty_and_authorization_still_wins() {
        let mut vfs = VfsManager::new();
        let backend = vfs
            .mounts
            .add_backend(Box::new(Faulted(Arc::new(AtomicBool::new(false)))));
        vfs.mounts.mount("/data", backend);
        let caller = Caller::principal(types::CellId(44), 1);
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        let dir = vfs.dirs.open_root(caller, "/data").unwrap();
        let mut buffer = [0; IPC_BUF_SIZE];
        assert!(matches!(
            list_at(&vfs, caller, dir, &mut buffer),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        let other = Caller::principal(types::CellId(45), 1);
        assert!(matches!(
            list_at(&vfs, other, dir, &mut buffer),
            VfsResponse::Err(code) if code == VfsError::Handle as u8
        ));
        assert!(matches!(
            stat_path(&vfs, caller, "unmounted"),
            VfsResponse::Err(code) if code == VfsError::Denied as u8
        ));
    }
    #[test]
    fn directory_whole_file_reads_are_errors_not_empty_success() {
        let mut vfs = VfsManager::new();
        let backend = vfs
            .mounts
            .add_backend(Box::new(Faulted(Arc::new(AtomicBool::new(false)))));
        vfs.mounts.mount("/data", backend);
        let caller = Caller::principal(types::CellId(44), 1);
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        let root = vfs.dirs.open_root(caller, "/").unwrap();
        let mut buffer = [0; IPC_BUF_SIZE];
        assert!(matches!(
            crate::dispatch_dirs::handle(
                &mut vfs,
                caller,
                &wire::ipc::VfsRequest::ReadAt {
                    dir: root,
                    name: "data"
                },
                &mut buffer
            ),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        assert!(matches!(
            crate::grant_read::read_file_grant(&vfs, caller, "/data", 0, 0),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        let mut request = [0; IPC_BUF_SIZE];
        wire::ipc::encode(
            &wire::ipc::VfsRequest::ReadAsync { path: "/data" },
            &mut request,
        )
        .unwrap();
        assert!(matches!(
            crate::dispatch::handle_request(&mut vfs, &request, Some(caller), &mut buffer),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
    }
    #[test]
    fn poisoned_pending_read_is_consumed_only_by_its_authorized_owner() {
        let mut vfs = VfsManager::new();
        let backend = vfs
            .mounts
            .add_backend(Box::new(Faulted(Arc::new(AtomicBool::new(true)))));
        vfs.mounts.mount("/data", backend);
        let caller = Caller::principal(types::CellId(44), 1);
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        let handle = vfs
            .pending
            .insert(caller, "/data/a", alloc::vec![1, 2], None);
        let mut request = [0; IPC_BUF_SIZE];
        let mut buffer = [0; IPC_BUF_SIZE];
        wire::ipc::encode(&wire::ipc::VfsRequest::Poll { handle }, &mut request).unwrap();
        let other = Caller::principal(types::CellId(45), 1);
        assert!(matches!(
            crate::dispatch::handle_request(&mut vfs, &request, Some(other), &mut buffer),
            VfsResponse::Err(code) if code == VfsError::Handle as u8
        ));
        assert_eq!(vfs.pending.owned_path(caller, handle), Some("/data/a"));
        assert!(matches!(
            crate::dispatch::handle_request(&mut vfs, &request, Some(caller), &mut buffer),
            VfsResponse::Err(code) if code == VfsError::Io as u8
        ));
        assert_eq!(vfs.pending.owned_path(caller, handle), None);
    }
}
