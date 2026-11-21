//! Grant-backed writes through service-issued file handles.
//!
//! The wire carries an opaque `u64`, but only an entry in `FileHandleTable`
//! gives it a path and an owner. Resolve that authority and re-check policy
//! before copying through the leased GrantSlice adapter, then commit before
//! acknowledging it.

use alloc::string::ToString;

use wire::ipc::VfsResponse;
use wire::proto::vfs::VfsError;
use wire::vfs_file_handles::ViVfsFileHandle;

use crate::caller::Caller;
use crate::manager::VfsManager;

const MAX_GRANT_WRITE: usize = 4096;

pub(crate) fn write<'a>(
    vfs: &mut VfsManager,
    caller: Caller,
    cap: u64,
    offset: u64,
    grant: usize,
    bytes: usize,
) -> VfsResponse<'a> {
    // This lookup makes an unknown cap and one held by another caller exactly
    // the same failure. It also rejects a handle concurrently being closed or
    // read through; no stale authority reaches grant memory.
    let Some(path) = vfs
        .files
        .path_of(caller, ViVfsFileHandle(cap))
        .map(ToString::to_string)
    else {
        return VfsResponse::Err(VfsError::Denied as u8);
    };
    if crate::access::is_guest_disk_path(&path) || !vfs.access.can_write(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    let Ok(Some((file_len, false))) = vfs.stat(&path) else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    let Ok(file_len) = usize::try_from(file_len) else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    let Ok(offset) = usize::try_from(offset) else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    if bytes > MAX_GRANT_WRITE || offset > file_len {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let Some(end) = offset.checked_add(bytes) else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    let (old_len, new_len) = (file_len as u64, file_len.max(end) as u64);
    if !vfs.quota.can_replace(caller.cell, &path, old_len, new_len) {
        return VfsResponse::Err(VfsError::Quota as u8);
    }
    if bytes == 0 {
        return VfsResponse::GrantDone { bytes: 0 };
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    // GrantSlice resolution and VFS lease publication are one kernel grant-table
    // transaction. Stage only the requested range: a missing or short grant is
    // an I/O error and the backend remains untouched.
    let mut chunk = alloc::vec![0u8; bytes];
    if ostd::syscall::sys_grant_copy_to_slice(grant, &mut chunk) != Some(bytes) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    // Only the written range reaches the backend, never a copy of the file.
    if vfs.write_at(&path, offset as u64, &chunk).is_err() {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    // Observed native sync is functional completion, not crash-preserving
    // durability. A failed mutation is possibly accepted and closes the instance.
    vfs.quota
        .commit_replace(caller.cell, &path, old_len, new_len);
    VfsResponse::GrantDone { bytes }
}
