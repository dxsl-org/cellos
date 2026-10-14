//! Serving grant-addressed operations (read/write/sync over memory grants).

use alloc::string::String;
use api::dir_handles::ViDirHandle;
use api::ipc::{VfsResponse, IPC_BUF_SIZE};
use api::vfs_file_handles::ViVfsFileHandle;

use crate::caller::Caller;
use crate::dispatch_dirs::MAX_INLINE_PAYLOAD;
use crate::manager::VfsManager;
use crate::paths::{ERR_DENIED, ERR_HANDLE, ERR_IO, ERR_QUOTA};

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
        None => return VfsResponse::Err(ERR_HANDLE),
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    if !vfs.healthy(&path) { return VfsResponse::Err(ERR_IO); }
    if !matches!(vfs.stat(&path), Some((_, false))) { return VfsResponse::Err(ERR_IO); }
    let max_read = size.min(api::ipc::VFS_GRANT_CHUNK);
    match ostd::grant::with_shared_bytes_mut(grant, |buf| {
        let n = max_read.min(buf.len());
        if n == 0 {
            0
        } else {
            vfs.read_at(&path, offset, &mut buf[..n])
        }
    }) {
        Some(bytes) if vfs.healthy(&path) => VfsResponse::GrantDone { bytes },
        _ => VfsResponse::Err(ERR_IO),
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
        None => return VfsResponse::Err(ERR_HANDLE),
    };
    if !vfs.access.can_write(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    if !vfs.healthy(&path) { return VfsResponse::Err(ERR_IO); }
    if bytes == 0 {
        return VfsResponse::GrantDone { bytes: 0 };
    }
    if bytes > api::ipc::VFS_GRANT_CHUNK {
        return VfsResponse::Err(ERR_IO);
    }
    let file_len = match vfs.stat(&path) {
        Some((len, false)) => len,
        _ => return VfsResponse::Err(ERR_IO),
    };
    let end = match offset.checked_add(bytes as u64) {
        Some(e) => e,
        None => return VfsResponse::Err(ERR_IO),
    };
    let is_guest_disk = crate::access::is_guest_disk_path(&path);
    if is_guest_disk && end > file_len {
        return VfsResponse::Err(ERR_DENIED);
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
        return VfsResponse::Err(ERR_QUOTA);
    }

    let written = ostd::grant::with_shared_bytes(grant, |buf| {
        buf.len() >= bytes && vfs.write_at(&path, offset, &buf[..bytes])
    });
    if written != Some(true) {
        return VfsResponse::Err(ERR_IO);
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
        None => return VfsResponse::Err(ERR_HANDLE),
    };
    if !vfs.access.can_write(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    if vfs.sync(&path) {
        VfsResponse::Ok
    } else {
        VfsResponse::Err(ERR_IO)
    }
}

pub(crate) fn stat_path<'a>(vfs: &VfsManager, caller: Caller, path: &str) -> VfsResponse<'a> {
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    match vfs.stat(path) {
        Some((size, is_dir)) => VfsResponse::Stat { size, is_dir },
        None => VfsResponse::Err(ERR_IO),
    }
}

pub(crate) fn list_at<'a>(
    vfs: &VfsManager,
    caller: Caller,
    dir: ViDirHandle,
    resp_buf: &'a mut [u8; IPC_BUF_SIZE],
) -> VfsResponse<'a> {
    let Some(path) = vfs.dirs.dir_path(caller, dir).map(String::from) else {
        return VfsResponse::Err(ERR_HANDLE);
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(ERR_DENIED);
    }
    if !vfs.healthy(&path) { return VfsResponse::Err(ERR_IO); }
    if !matches!(vfs.stat(&path), Some((_, true))) { return VfsResponse::Err(ERR_IO); }
    let n = vfs.list_dir(&path, &mut resp_buf[..MAX_INLINE_PAYLOAD]);
    if !vfs.healthy(&path) { return VfsResponse::Err(ERR_IO); }
    VfsResponse::Data(&resp_buf[..n])
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
            if self.0.load(Ordering::SeqCst) { BackendHealth::Poisoned } else { BackendHealth::Ready }
        }
        fn get_file_ptr(&self, _: &str) -> Option<(usize, usize)> { None }
        fn list(&self, _: &str, _: &mut [u8]) -> usize { self.0.store(true, Ordering::SeqCst); 0 }
        fn stat(&self, path: &str) -> Option<(u64, bool)> { Some((8, path == "/data")) }
        fn file_size(&self, _: &str) -> u64 { 8 }
        fn read_to_vec(&self, _: &str) -> Vec<u8> { self.0.store(true, Ordering::SeqCst); Vec::new() }
        fn read_at(&self, _: &str, _: u64, _: &mut [u8]) -> usize { self.0.store(true, Ordering::SeqCst); 0 }
        fn write(&mut self, _: &str, _: &[u8]) -> bool { false }
        fn append(&mut self, _: &str, _: &[u8]) -> bool { false }
        fn mkdir(&mut self, _: &str) -> bool { false }
        fn rmdir(&mut self, _: &str) -> bool { false }
        fn unlink(&mut self, _: &str) -> bool { false }
        fn rmdir_recursive(&mut self, _: &str) -> bool { false }
    }
    #[test]
    fn faulting_ranged_read_unwinds_pin_and_does_not_fallback_or_issue_empty_data() {
        let mut vfs = VfsManager::new();
        let poisoned = Arc::new(AtomicBool::new(false));
        let backend = vfs.mounts.add_backend(Box::new(Faulted(poisoned.clone())));
        vfs.mounts.mount("/data", backend);
        let caller = Caller { flags: api::caller_identity::CALLER_FLAG_VFS_MUTATE, ..Caller::principal(types::CellId(44), 1) };
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        let dir = vfs.dirs.open_root(caller, "/data").unwrap();
        let file = vfs.files.insert(caller, "/data/a", dir.0).unwrap();
        let mut buffer = [0; IPC_BUF_SIZE];
        assert!(matches!(crate::dispatch_file_handles::read_file(
            &mut vfs, caller, file, 0, 8, &mut buffer
        ), VfsResponse::Err(ERR_IO)));
        assert!(poisoned.load(Ordering::SeqCst));
        assert!(matches!(write_handle_grant(&mut vfs, caller, file, 0, 0, 0), VfsResponse::Err(ERR_IO)));
        assert!(matches!(stat_path(&vfs, caller, "/data/a"), VfsResponse::Err(ERR_IO)));
        assert!(matches!(crate::dispatch_file_handles::close_file(&mut vfs, caller, file), VfsResponse::Ok));
    }
    #[test]
    fn faulting_list_is_io_not_empty_and_authorization_still_wins() {
        let mut vfs = VfsManager::new();
        let backend = vfs.mounts.add_backend(Box::new(Faulted(Arc::new(AtomicBool::new(false)))));
        vfs.mounts.mount("/data", backend);
        let caller = Caller::principal(types::CellId(44), 1);
        let _ = vfs.dirs.on_contact(caller);
        vfs.dirs.mark_attested(caller);
        let dir = vfs.dirs.open_root(caller, "/data").unwrap();
        let mut buffer = [0; IPC_BUF_SIZE];
        assert!(matches!(list_at(&vfs, caller, dir, &mut buffer), VfsResponse::Err(ERR_IO)));
        let other = Caller::principal(types::CellId(45), 1);
        assert!(matches!(list_at(&vfs, other, dir, &mut buffer), VfsResponse::Err(ERR_HANDLE)));
        assert!(matches!(stat_path(&vfs, caller, "unmounted"), VfsResponse::Err(ERR_DENIED)));
    }
    #[test]
    fn directory_whole_file_reads_are_errors_not_empty_success() {
        let mut vfs = VfsManager::new();
        let backend = vfs.mounts.add_backend(Box::new(Faulted(Arc::new(AtomicBool::new(false)))));
        vfs.mounts.mount("/data", backend);
        let caller = Caller::principal(types::CellId(44), 1);
        let _ = vfs.dirs.on_contact(caller); vfs.dirs.mark_attested(caller);
        let root = vfs.dirs.open_root(caller, "/").unwrap();
        let mut buffer = [0; IPC_BUF_SIZE];
        assert!(matches!(crate::dispatch_dirs::handle(&mut vfs, caller,
            &api::ipc::VfsRequest::ReadAt { dir: root, name: "data" }, &mut buffer),
            VfsResponse::Err(ERR_IO)));
        assert!(matches!(crate::grant_read::read_file_grant(&vfs, caller, "/data", 0, 0),
            VfsResponse::Err(ERR_IO)));
        let mut request = [0; IPC_BUF_SIZE];
        api::ipc::encode(&api::ipc::VfsRequest::ReadAsync { path: "/data" }, &mut request).unwrap();
        assert!(matches!(crate::dispatch::handle_request(&mut vfs, &request, Some(caller), &mut buffer),
            VfsResponse::Err(ERR_IO)));
    }
    #[test]
    fn poisoned_pending_read_is_consumed_only_by_its_authorized_owner() {
        let mut vfs = VfsManager::new();
        let backend = vfs.mounts.add_backend(Box::new(Faulted(Arc::new(AtomicBool::new(true)))));
        vfs.mounts.mount("/data", backend);
        let caller = Caller::principal(types::CellId(44), 1);
        let _ = vfs.dirs.on_contact(caller); vfs.dirs.mark_attested(caller);
        let handle = vfs.pending.insert(caller, "/data/a", alloc::vec![1, 2], None);
        let mut request = [0; IPC_BUF_SIZE]; let mut buffer = [0; IPC_BUF_SIZE];
        api::ipc::encode(&api::ipc::VfsRequest::Poll { handle }, &mut request).unwrap();
        let other = Caller::principal(types::CellId(45), 1);
        assert!(matches!(crate::dispatch::handle_request(&mut vfs, &request, Some(other), &mut buffer),
            VfsResponse::Err(ERR_HANDLE)));
        assert_eq!(vfs.pending.owned_path(caller, handle), Some("/data/a"));
        assert!(matches!(crate::dispatch::handle_request(&mut vfs, &request, Some(caller), &mut buffer),
            VfsResponse::Err(ERR_IO)));
        assert_eq!(vfs.pending.owned_path(caller, handle), None);
    }
}
