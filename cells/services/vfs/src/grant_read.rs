//! Serving grant read operations (ReadGrant and ReadFileGrant).
//!
//! Both write file bytes into a grant the caller owns and shared to this cell.
//! The copy goes through `ostd::syscall::sys_grant_copy_from_slice`, never a raw
//! pointer held here. That adapter's single `GrantSlice` resolves the grant and
//! publishes this request's kernel lease in one grant-table transaction
//! (`resolve_and_lease_grant`): while the lease stands, the owner's `GrantFree`
//! and `GrantUnregister` refuse and an owner death quarantines the frames rather
//! than freeing them. The lease ends only at the reply to the owner or at this
//! cell's next wildcard receive. The grant is therefore resolved *after* the file
//! is read, immediately before the copy, so no backend I/O sits between the
//! lease and the write. A second `GrantSlice` in one request is refused as a
//! duplicate lease, so each path resolves the grant exactly once.

use alloc::string::ToString;
use alloc::vec;

use ostd::syscall::sys_grant_copy_from_slice;
use wire::ipc::VfsResponse;
use wire::proto::vfs::VfsError;
use wire::vfs_file_handles::ViVfsFileHandle;

use crate::caller::Caller;
use crate::fs_error::FsError;
use crate::manager::VfsManager;

/// Largest `ReadGrant` transfer: the one page `ostd::fs` allocates per call.
const MAX_GRANT_READ: usize = 4096;

/// `ReadGrant`: up to `size` bytes at `offset` of the file behind the
/// VFS-issued handle `cap`, into `grant`.
///
/// Same authority as `WriteGrant`: only an open entry in `FileHandleTable`
/// names a path. An unknown handle and one held by another caller are the same
/// answer — zero bytes — so sweeping handle values reveals nothing.
pub(crate) fn read_grant<'a>(
    vfs: &VfsManager,
    caller: Caller,
    cap: u64,
    offset: u64,
    size: usize,
    grant: usize,
) -> VfsResponse<'a> {
    let Some(path) = vfs
        .files
        .path_of(caller, ViVfsFileHandle(cap))
        .map(ToString::to_string)
    else {
        return VfsResponse::GrantDone { bytes: 0 };
    };
    if !vfs.access.can_read(caller, &path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let Ok(Some((file_len, false))) = vfs.stat(&path) else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    let file_len = usize::try_from(file_len).unwrap_or(usize::MAX);
    let start = match usize::try_from(offset) {
        Ok(start) if start < file_len => start,
        Ok(_) | Err(_) => return VfsResponse::GrantDone { bytes: 0 },
    };
    let n = size.min(file_len - start).min(MAX_GRANT_READ);
    if n == 0 {
        return VfsResponse::GrantDone { bytes: 0 };
    }
    let mut buf = vec![0u8; n];
    let mut read = match vfs.read_at(&path, offset, &mut buf) {
        Ok(read) => read,
        Err(FsError::Unsupported) => 0,
        // A device failure is an error, never a short grant.
        Err(_) => return VfsResponse::Err(VfsError::Io as u8),
    };
    if read == 0 {
        // A backend without ranged reads returns nothing; the whole-file copy
        // keeps it correct instead of silently answering zero bytes.
        let Ok(data) = vfs.read_to_vec(&path) else {
            return VfsResponse::Err(VfsError::Io as u8);
        };
        if let Some(tail) = data.get(start..) {
            read = n.min(tail.len());
            buf[..read].copy_from_slice(&tail[..read]);
        }
    }
    if !vfs.healthy(&path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    match sys_grant_copy_from_slice(grant, &buf[..read]) {
        Some(bytes) => VfsResponse::GrantDone { bytes },
        None => VfsResponse::Err(VfsError::Io as u8),
    }
}

pub(crate) fn read_file_grant<'a>(
    vfs: &VfsManager,
    caller: Caller,
    path: &str,
    grant: usize,
    max: usize,
) -> VfsResponse<'a> {
    #[cfg(feature = "recovery-fixture")]
    if path == HOLD_LEASE_PATH {
        return hold_lease(grant);
    }
    if !vfs.access.can_read(caller, path) {
        return VfsResponse::Err(VfsError::Denied as u8);
    }
    if !vfs.healthy(path) || !matches!(vfs.stat(path), Ok(Some((_, false)))) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    let data = vfs.read_to_vec(path);
    if !vfs.healthy(path) {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    // A read that failed part way is an error, never the bytes before it.
    let Ok(data) = data else {
        return VfsResponse::Err(VfsError::Io as u8);
    };
    // The adapter also caps the copy at the kernel-registered grant length.
    match sys_grant_copy_from_slice(grant, &data[..data.len().min(max)]) {
        Some(bytes) => VfsResponse::GrantDone { bytes },
        None => VfsResponse::Err(VfsError::Io as u8),
    }
}

/// `recovery-fixture` (Phase 09B evidence images only): the borrower that does
/// not acknowledge. Resolving the grant publishes this request's lease; the
/// reply that releases it is withheld for `HOLD_LEASE_MS`, so a grant owner
/// that dies meanwhile must leave its pages quarantined, not freed.
#[cfg(feature = "recovery-fixture")]
const HOLD_LEASE_PATH: &str = "/tmp/recovery-hold-lease";
#[cfg(feature = "recovery-fixture")]
const HOLD_LEASE_MS: u64 = 4_000;

#[cfg(feature = "recovery-fixture")]
fn hold_lease<'a>(grant: usize) -> VfsResponse<'a> {
    use ostd::syscall::{sys_get_time_ms, sys_grant_slice_with_len, sys_yield};
    if sys_grant_slice_with_len(grant).is_none() {
        return VfsResponse::Err(VfsError::Io as u8);
    }
    ostd::io::println("[vfs] recovery-fixture: holding grant lease without acknowledgement");
    let start = sys_get_time_ms().unwrap_or(0);
    while sys_get_time_ms().is_some_and(|now| now.saturating_sub(start) < HOLD_LEASE_MS) {
        sys_yield();
    }
    ostd::io::println("[vfs] recovery-fixture: lease released");
    VfsResponse::Err(VfsError::Io as u8)
}
