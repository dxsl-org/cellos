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
    let max_read = size.min(4096);
    let mut chunk = alloc::vec![0u8; max_read];
    let n = if max_read == 0 {
        0
    } else {
        vfs.read_at(&path, offset, &mut chunk)
    };
    match ostd::syscall::sys_grant_copy_from_slice(grant, &chunk[..n]) {
        Some(copied) => VfsResponse::GrantDone { bytes: copied },
        None => VfsResponse::Err(ERR_IO),
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
    if bytes == 0 {
        return VfsResponse::GrantDone { bytes: 0 };
    }
    if bytes > 4096 {
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

    let mut chunk = alloc::vec![0u8; bytes];
    if ostd::syscall::sys_grant_copy_to_slice(grant, &mut chunk) != Some(bytes) {
        return VfsResponse::Err(ERR_IO);
    }
    if vfs.write_at(&path, offset, &chunk) {
        if !is_guest_disk {
            vfs.quota.release_path(&path, file_len);
            let _ = vfs.quota.charge(caller.cell, new_len);
            vfs.quota.set_writer(&path, caller.cell);
        }
        VfsResponse::GrantDone { bytes }
    } else {
        VfsResponse::Err(ERR_IO)
    }
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
    let n = vfs.list_dir(&path, &mut resp_buf[..MAX_INLINE_PAYLOAD]);
    VfsResponse::Data(&resp_buf[..n])
}
