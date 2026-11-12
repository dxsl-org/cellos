//! Grant-backed writes through service-issued file handles.
//!
//! The wire carries an opaque `u64`, but only an entry in `FileHandleTable`
//! gives it a path and an owner. Resolve that authority and re-check policy
//! before copying through the leased GrantSlice adapter, then commit before
//! acknowledging it.

use alloc::string::ToString;

use alloc::vec::Vec;

use api::ipc::VfsResponse;
use api::services::proto::vfs::VfsError;
use api::vfs_file_handles::ViVfsFileHandle;

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
    let Some((file_len, false)) = vfs.stat(&path) else {
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
    let new_len = file_len.max(end);
    let refunded_to_caller = if vfs.quota.writer_of(&path) == Some(caller.cell) {
        file_len as u64
    } else {
        0
    };
    let net_charge = (new_len as u64).saturating_sub(refunded_to_caller);
    if net_charge > 0 && !vfs.quota.can_charge(caller.cell, net_charge) {
        return VfsResponse::Err(VfsError::Quota as u8);
    }
    if bytes == 0 {
        return VfsResponse::GrantDone { bytes: 0 };
    }

    let old = vfs.read_to_vec(&path);
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    if old.len() != file_len {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let mut committed = Vec::with_capacity(new_len);
    committed.extend_from_slice(&old);
    committed.resize(new_len, 0);
    // GrantSlice resolution and VFS lease publication are one kernel grant-table
    // transaction. Copy into only the requested file range; a missing or short
    // grant is an I/O error and the backend remains untouched.
    if ostd::syscall::sys_grant_copy_to_slice(grant, &mut committed[offset..end]) != Some(bytes) {
        return VfsResponse::Err(VfsError::Io as u8);
    }

    if !vfs.write(&path, &committed) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    // Observed native sync is functional completion, not crash-preserving
    // durability. A failed mutation is possibly accepted and closes the instance.
    vfs.quota.release_path(&path, file_len as u64);
    let _ = vfs.quota.charge(caller.cell, new_len as u64);
    vfs.quota.set_writer(&path, caller.cell);
    VfsResponse::GrantDone { bytes }
}
